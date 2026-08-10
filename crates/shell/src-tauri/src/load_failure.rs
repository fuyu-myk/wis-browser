//! Native page-load failure hooks for Windows and Linux.
//!
//! WebView2 and WebKitGTK expose their own per-webview failure callbacks, so
//! each registers one here and reports the failure into tab state. The chrome
//! then renders an error page in the tab's place (`SplitPaneHost`).

use tauri::{AppHandle, Manager};
use wis_core::{LoadError, TabId};

use crate::state::AppState;

/// Set (or clear) a tab's load error from a native callback. `failure`
/// carries the normalized fault with a placeholder URL; the failing URL is
/// filled from tab state (the target URL, written before navigation). Runs
/// the state mutation off the callback.
pub fn report(app: &AppHandle, tab_id: TabId, failure: Option<LoadError>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let result = crate::commands::mutate_unit(&app, &state, |s| {
            let error = failure.map(|err| {
                let url = s.tab(tab_id).map(|t| t.url.to_string()).unwrap_or_default();
                LoadError { url, ..err }
            });

            s.with_tab_load_error(tab_id, error)
        })
        .await;

        if let Err(err) = result {
            log::warn!("load-failure state update for {tab_id} failed: {err}");
        }
    });
}

/// Windows: a second `NavigationCompleted` handler reads `IsSuccess`/`WebErrorStatus`
/// and reports a failure. It never clears on success. Called per tab webview.
#[cfg(windows)]
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, tab_id: TabId) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2NavigationCompletedEventArgs,
        COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN,
    };
    use webview2_com::NavigationCompletedEventHandler;

    let app = webview.app_handle().clone();
    crate::win_webview2::with_core_webview2(webview, move |core| {
        let handler = NavigationCompletedEventHandler::create(Box::new(
            move |_sender: Option<ICoreWebView2>,
                  args: Option<ICoreWebView2NavigationCompletedEventArgs>|
                  -> windows::core::Result<()> {
                let Some(args) = args else {
                    return Ok(());
                };

                let mut success = windows::core::BOOL(0);
                // SAFETY: UI thread (with_core_webview2); out-params are stack locals.
                unsafe { args.IsSuccess(&mut success)? };
                if success.as_bool() {
                    return Ok(());
                }

                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
                // SAFETY: as above.
                unsafe { args.WebErrorStatus(&mut status)? };
                if let Some(err) = LoadError::from_web_error_status(status.0 as u32, String::new())
                {
                    report(&app, tab_id, Some(err));
                }

                Ok(())
            },
        ));
        let mut token = 0_i64;

        // SAFETY: runs on the UI thread via with_core_webview2.
        if let Err(err) = unsafe { core.add_NavigationCompleted(&handler, &mut token) } {
            log::warn!("load_failure: add_NavigationCompleted failed: {err}");
        }

        install_certificate_bypass(core, tab_id);
    });
}

/// Windows: on a certificate error, freeze the presented certificate (PEM)
/// against the tab. Set `ALWAYS_ALLOW` once pinned, and when the presented
/// certificate matches the pin. Every other certificate error keeps the
/// default cancel, which surfaces through `NavigationCompleted` above.
#[cfg(windows)]
fn install_certificate_bypass(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    tab_id: TabId,
) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2ServerCertificateErrorDetectedEventArgs, ICoreWebView2_14,
        COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW,
    };
    use webview2_com::{CoTaskMemPWSTR, ServerCertificateErrorDetectedEventHandler};
    use windows::core::Interface;

    let Ok(c14) = core.cast::<ICoreWebView2_14>() else {
        return;
    };

    let handler = ServerCertificateErrorDetectedEventHandler::create(Box::new(
        move |_sender, args: Option<ICoreWebView2ServerCertificateErrorDetectedEventArgs>| {
            let Some(args) = args else {
                return Ok(());
            };

            let mut uri = windows::core::PWSTR::null();
            // SAFETY: UI thread callback; out-param is a stack local the
            // CoTaskMemPWSTR wrapper frees.
            unsafe { args.RequestUri(&mut uri)? };
            let host = url::Url::parse(&CoTaskMemPWSTR::from(uri).to_string())
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned));

            // SAFETY: as above; the certificate getter and PEM out-param are
            // UI-thread COM calls.
            let pem = unsafe { args.ServerCertificate() }.ok().and_then(|cert| {
                let mut pem = windows::core::PWSTR::null();
                unsafe { cert.ToPemEncoding(&mut pem) }
                    .ok()
                    .map(|()| CoTaskMemPWSTR::from(pem).to_string())
            });

            if let (Some(host), Some(pem)) = (host, pem) {
                crate::tls_bypass::record_candidate(tab_id, &host, pem.clone().into_bytes());
                if crate::tls_bypass::is_pinned(&host, pem.as_bytes()) {
                    // SAFETY: as above.
                    unsafe {
                        args.SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW)?
                    };
                }
            }

            Ok(())
        },
    ));
    let mut token = 0_i64;

    // SAFETY: runs on the UI thread via with_core_webview2.
    if let Err(err) = unsafe { c14.add_ServerCertificateErrorDetected(&handler, &mut token) } {
        log::warn!("load_failure: add_ServerCertificateErrorDetected failed: {err}");
    }
}

/// Linux: the failing `(host, certificate)` for each tab's current TLS
/// interstitial, stashed on the GTK main thread by the TLS-error signal so a
/// later user-approved bypass can hand the exact certificate to
/// `allow_tls_certificate_for_host`. Keyed by tab so a concurrent failure on
/// another tab cannot mutate this tab's frozen candidate; the entry is
/// removed when consumed.
#[cfg(target_os = "linux")]
thread_local! {
    static PENDING_TLS_CERTS: std::cell::RefCell<
        std::collections::HashMap<TabId, (String, webkit2gtk::gio::TlsCertificate)>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Linux: connect WebKitGTK's `load-failed` and `load-failed-with-tls-errors`
/// signals. Returning `true` suppresses the engine's built-in error page so
/// the chrome error page is the only one shown. Called per tab webview.
#[cfg(target_os = "linux")]
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, tab_id: TabId) {
    use webkit2gtk::{NetworkError, WebViewExt};

    let app = webview.app_handle().clone();
    let app_tls = app.clone();
    let _ = webview.with_webview(move |w| {
        let wv = w.inner();

        wv.connect_load_failed(move |_wv, _event, uri, error| {
            let cancelled = error.matches(NetworkError::Cancelled);
            if let Some(err) = LoadError::from_glib(cancelled, None, uri.to_string()) {
                report(&app, tab_id, Some(err));
            }

            // Suppress the engine's default error page only when we showed ours.
            !cancelled
        });

        wv.connect_load_failed_with_tls_errors(move |_wv, uri, cert, flags| {
            if let Some(host) = url::Url::parse(uri)
                .ok()
                .and_then(|u| u.host_str().map(crate::tls_bypass::normalize_host))
            {
                PENDING_TLS_CERTS.with(|c| c.borrow_mut().insert(tab_id, (host, cert.clone())));
            }

            if let Some(err) = LoadError::from_glib(false, Some(flags.bits()), uri.to_string()) {
                report(&app_tls, tab_id, Some(err));
            }
            true
        });
    });
}

/// Linux: allow the certificate frozen for this tab's interstitial on its web
/// context, consuming the stash entry. `host` is ignored in favor of the
/// stashed failing host. Runs on the GTK main thread.
#[cfg(target_os = "linux")]
pub fn allow_stashed_certificate(app: &AppHandle, tab_id: TabId, _host: &str) {
    use webkit2gtk::{WebContextExt, WebViewExt};

    let label = format!("tab-{}", tab_id.as_uuid());
    let Some(webview) = app.webviews().get(&label).cloned() else {
        log::warn!("tls bypass: no webview for {tab_id}; certificate allowance skipped");
        return;
    };

    let result = webview.with_webview(move |w| {
        let taken = PENDING_TLS_CERTS.with(|c| c.borrow_mut().remove(&tab_id));
        if let (Some((host, cert)), Some(ctx)) = (taken, w.inner().context()) {
            ctx.allow_tls_certificate_for_host(&cert, &host);
        }
    });
    if let Err(err) = result {
        log::warn!("tls bypass: certificate allowance dispatch failed: {err}");
    }
}
