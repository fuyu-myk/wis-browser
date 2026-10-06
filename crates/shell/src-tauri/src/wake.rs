//! React to the system waking from sleep (macOS).
//!
//! Tunnels through the workspace proxies can die while asleep without either
//! end noticing: the page's side is loopback, so a hung connection only clears
//! on a kernel timeout, and pages that wait on one look frozen. Waking cuts
//! them all so pages reconnect. It also lifts any hover guard whose exit event
//! was lost across the sleep, which would otherwise leave tabs ignoring clicks.

use tauri::{AppHandle, Manager};

use crate::state::AppState;

pub fn install(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    imp::install(app.clone());

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn on_wake(app: &AppHandle) {
    crate::hover_guard::release_all();

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let proxies = state.proxies.lock().await;
        for server in proxies.values() {
            server.sever_open_connections();
        }
        log::info!(
            "woke from sleep; cut the tunnels of {} proxies",
            proxies.len()
        );
    });
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CString;
    use std::sync::OnceLock;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass,
    };
    use tauri::AppHandle;

    static APP: OnceLock<AppHandle> = OnceLock::new();

    declare_class!(
        struct WakeObserver;

        unsafe impl ClassType for WakeObserver {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisWakeObserver";
        }

        impl DeclaredClass for WakeObserver {
            type Ivars = ();
        }

        unsafe impl WakeObserver {
            #[method(didWake:)]
            fn did_wake(&self, _note: *mut AnyObject) {
                if let Some(app) = APP.get() {
                    super::on_wake(app);
                }
            }
        }
    );

    pub(super) fn install(app: AppHandle) {
        if APP.set(app).is_err() {
            return;
        }
        unsafe { register() }
    }

    pub(super) unsafe fn wake_name() -> *mut AnyObject {
        let name = CString::new("NSWorkspaceDidWakeNotification").unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()]
    }

    pub(super) unsafe fn workspace_center() -> *mut AnyObject {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        msg_send![workspace, notificationCenter]
    }

    pub(super) unsafe fn register() {
        let observer: Id<WakeObserver> = msg_send_id![WakeObserver::alloc(), init];
        let _: () = msg_send![
            workspace_center(),
            addObserver: &*observer,
            selector: sel!(didWake:),
            name: wake_name(),
            object: std::ptr::null_mut::<AnyObject>(),
        ];
        // The center does not retain its observers, and this one lives as long
        // as the app.
        std::mem::forget(observer);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    /// A wrong message type aborts at runtime only, so drive every send once.
    #[test]
    fn the_observer_registers_and_handles_a_wake() {
        unsafe {
            super::imp::register();
            let center = super::imp::workspace_center();
            let _: () = msg_send![
                center,
                postNotificationName: super::imp::wake_name(),
                object: std::ptr::null_mut::<AnyObject>(),
            ];
        }
    }
}
