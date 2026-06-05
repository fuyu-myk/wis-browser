//! Touch ID (and password-fallback) gate for the password vault, via macOS
//! `LocalAuthentication`.
//!
//! `evaluatePolicy:localizedReason:reply:` is asynchronous (it shows the system
//! Touch ID sheet and replies on a private queue). It is called from a blocking
//! task and the reply is awaited, so the API is a simple `async fn -> bool`.

/// Prompt the device owner to authenticate (Touch ID, Apple Watch, or the login
/// password as fallback). Returns `true` only on success. Non-macOS builds and
/// machines without any enrolled authentication return `false`.
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

#[cfg(not(target_os = "macos"))]
fn authenticate_blocking(_reason: &str) -> bool {
    false
}
