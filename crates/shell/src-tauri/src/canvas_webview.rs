//! Canvas tabs' WebKit configuration (macOS).

#[cfg(target_os = "macos")]
pub use imp::with_canvas_configuration;

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use objc2_06::runtime::{AnyClass, AnyObject};
    use objc2_06::{msg_send, MainThreadMarker};
    use objc2_web_kit::{WKPreferences, WKWebViewConfiguration, WKWebsiteDataStore};
    use tauri::webview::WebviewBuilder;
    use tauri::{AppHandle, Wry};
    use wis_core::{WisError, WisResult};

    const FRAME_CAP_FEATURE: &str = "PreferPageRenderingUpdatesNear60FPSEnabled";

    /// Its own throwaway store, so the workspace proxy never reaches a shared
    /// one, and pages drawn at the display's full rate.
    pub async fn with_canvas_configuration(
        app: &AppHandle,
        builder: WebviewBuilder<Wry>,
    ) -> WisResult<WebviewBuilder<Wry>> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        app.run_on_main_thread(move || {
            let mtm = MainThreadMarker::new().expect("run_on_main_thread runs on the main thread");
            // SAFETY: class factories and setters, on the main thread.
            let config = unsafe {
                let config = WKWebViewConfiguration::new(mtm);
                config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
                if !lift_frame_cap(&config.preferences()) {
                    log::warn!("{FRAME_CAP_FEATURE} is gone; canvases render at 60 fps");
                }
                config
            };
            if tx.send(builder.with_webview_configuration(config)).is_err() {
                log::debug!("canvas tab creation was abandoned");
            }
        })
        .map_err(|err| WisError::InvalidInput {
            message: format!("run_on_main_thread: {err}"),
        })?;

        rx.await.map_err(|_| WisError::InvalidInput {
            message: "canvas configuration dropped".to_string(),
        })
    }

    /// WebKit holds pages near 60 fps unless this internal feature is off.
    unsafe fn lift_frame_cap(prefs: &WKPreferences) -> bool {
        let Some(class) = AnyClass::get(c"WKPreferences") else {
            return false;
        };
        let features: *mut AnyObject = msg_send![class, _features];
        if features.is_null() {
            return false;
        }

        let count: usize = msg_send![features, count];
        for i in 0..count {
            let feature: *mut AnyObject = msg_send![features, objectAtIndex: i];
            let key: *mut AnyObject = msg_send![feature, key];
            if key.is_null() {
                continue;
            }

            let utf8: *const c_char = msg_send![key, UTF8String];
            if !utf8.is_null() && CStr::from_ptr(utf8).to_bytes() == FRAME_CAP_FEATURE.as_bytes() {
                let _: () = msg_send![prefs, _setEnabled: false, forFeature: feature];
                return true;
            }
        }

        false
    }

    #[cfg(test)]
    mod tests {
        use objc2::runtime::{AnyObject, Bool};
        use objc2::{class, sel};

        type Obj = *mut AnyObject;

        /// A wrong message type aborts at runtime only.
        #[test]
        fn every_send_matches_its_method() {
            let prefs = class!(WKPreferences);
            prefs
                .metaclass()
                .verify_sel::<(), Obj>(sel!(_features))
                .unwrap();
            prefs
                .verify_sel::<(Bool, Obj), ()>(sel!(_setEnabled:forFeature:))
                .unwrap();
            class!(_WKFeature).verify_sel::<(), Obj>(sel!(key)).unwrap();
            class!(NSArray)
                .verify_sel::<(), usize>(sel!(count))
                .unwrap();
            class!(NSArray)
                .verify_sel::<(usize,), Obj>(sel!(objectAtIndex:))
                .unwrap();
            class!(NSString)
                .verify_sel::<(), *const std::os::raw::c_char>(sel!(UTF8String))
                .unwrap();
        }
    }
}
