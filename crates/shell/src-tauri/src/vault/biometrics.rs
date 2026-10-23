//! Biometric gate for the password vault.
//!
//! - **macOS:** `LocalAuthentication`'s `evaluatePolicy:localizedReason:reply:`
//!   shows the Touch ID sheet (with login-password fallback) and replies on a
//!   private queue, awaited from a blocking task.
//! - **Windows:** Windows Hello via `UserConsentVerifier`. The gate is applied
//!   only when Hello is actually available; on a machine with no configured PIN
//!   or biometric the unlock proceeds without a prompt (the key stays
//!   DPAPI-sealed at rest), so the vault is never permanently locked out.
//!
//! Either way the API is a simple `async fn -> bool`.

/// Prompt the device owner to authenticate (Touch ID / Windows Hello, with the
/// OS password/PIN fallback where the platform offers one). Returns `true` only
/// on success. Unsupported platforms return `false`.
pub async fn authenticate(reason: String) -> bool {
    tokio::task::spawn_blocking(move || authenticate_blocking(&reason))
        .await
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn authenticate_blocking(reason: &str) -> bool {
    use block2::RcBlock;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use std::ffi::CString;
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::Duration;

    // LAPolicyDeviceOwnerAuthentication: biometrics, Watch, or login password.
    const LA_POLICY_DEVICE_OWNER_AUTHENTICATION: i64 = 2;

    unsafe {
        let ctx: *mut AnyObject = msg_send![class!(LAContext), new];
        if ctx.is_null() {
            return false;
        }

        let reason_c = match CString::new(reason) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let reason_ns: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: reason_c.as_ptr()];

        let (tx, rx) = mpsc::channel::<bool>();
        let tx = Mutex::new(Some(tx));
        let reply = RcBlock::new(move |success: Bool, _error: *mut AnyObject| {
            if let Ok(mut guard) = tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(success.as_bool());
                }
            }
        });

        let _: () = msg_send![
            ctx,
            evaluatePolicy: LA_POLICY_DEVICE_OWNER_AUTHENTICATION,
            localizedReason: reason_ns,
            reply: &*reply,
        ];

        let result = rx.recv_timeout(Duration::from_secs(120)).unwrap_or(false);
        let _: () = msg_send![ctx, release];
        result
    }
}

#[cfg(windows)]
fn authenticate_blocking(reason: &str) -> bool {
    use windows::core::{factory, HSTRING};
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    };
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    use windows_future::IAsyncOperation;

    // WinRT calls need a COM apartment on this worker thread. UserConsentVerifier
    // shows system UI, so initialize an STA.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let outcome = (|| -> windows::core::Result<bool> {
        // Gate only when Hello is actually available; otherwise allow the unlock.
        let availability = UserConsentVerifier::CheckAvailabilityAsync()?.get()?;
        if availability != UserConsentVerifierAvailability::Available {
            log::warn!(
                "vault: Windows Hello unavailable ({availability:?}); unlocking without a biometric gate"
            );
            return Ok(true);
        }

        // Desktop (non-packaged) apps must drive the consent dialog through the
        // interop interface with a parent window handle.
        let interop: IUserConsentVerifierInterop =
            factory::<UserConsentVerifier, IUserConsentVerifierInterop>()?;
        let hwnd = unsafe { GetForegroundWindow() };
        let message = HSTRING::from(reason);
        let op: IAsyncOperation<UserConsentVerificationResult> =
            unsafe { interop.RequestVerificationForWindowAsync(hwnd, &message)? };

        Ok(op.get()? == UserConsentVerificationResult::Verified)
    })();

    outcome.unwrap_or(false)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn authenticate_blocking(_reason: &str) -> bool {
    false
}
