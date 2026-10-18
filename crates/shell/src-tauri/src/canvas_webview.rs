//! Canvas tabs' WebKit configuration (macOS).

#[cfg(target_os = "macos")]
pub use imp::with_canvas_configuration;

#[cfg(target_os = "macos")]
mod imp {
    use objc2_06::MainThreadMarker;
    use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};
    use tauri::webview::WebviewBuilder;
    use tauri::{AppHandle, Wry};
    use wis_core::{WisError, WisResult};

    /// Internal WebKit features canvas tabs turn off: the 60 fps cap, and the
    /// connections an HTML card could open past its CSP (WebRTC, preconnect,
    /// DNS prefetch). `canvas_frame.html` refuses to run cards if any survive.
    pub(super) const DISABLED_FEATURES: [&str; 4] = [
        crate::webkit_features::FRAME_RATE_CAP,
        "PeerConnectionEnabled",
        "LinkPreconnect",
        "LinkDNSPrefetchEnabled",
    ];

    /// Its own throwaway store, so the workspace proxy never reaches a shared
    /// one, pages drawn at the display's full rate, and no side channels.
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
                for name in
                    crate::webkit_features::disable(&config.preferences(), &DISABLED_FEATURES)
                {
                    log::warn!("WebKit feature {name} is gone; canvas tabs keep its default");
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

    #[cfg(test)]
    mod tests {
        #[test]
        fn cards_lose_the_connections_their_csp_cannot_stop() {
            for name in [
                "PeerConnectionEnabled",
                "LinkPreconnect",
                "LinkDNSPrefetchEnabled",
            ] {
                assert!(super::DISABLED_FEATURES.contains(&name), "{name}");
            }
        }
    }
}
