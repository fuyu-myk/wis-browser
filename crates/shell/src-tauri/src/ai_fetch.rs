//! Hidden web-access webviews for the AI assistant's tools.
//!
//! `ai_web_search` and `ai_web_fetch` load a page in a throwaway webview that
//! is incognito, proxied through the calling window's workspace, and hidden
//! before it ever paints. A narrow `wisAiFetch` message handler (mirroring
//! `page_text.rs`) posts the extracted text back; the webview is torn down on
//! every exit path.
//!
//! Trust model:
//! - Model-supplied and page-supplied URLs are an SSRF vector. The entry URL
//!   must pass `is_public_web_url`, every navigation (redirects included)
//!   re-checks the same predicate, and search-result URLs are re-validated
//!   before they reach the model.
//! - Everything the page posts is untrusted data: it is length-capped again on
//!   the Rust side and never trusted as markup.
//! - The webview carries no Tauri capabilities, refuses downloads, and gets no
//!   permission prompts.
//!
//! Teardown rides `terminate_page` + `close` like every remote-content
//! webview and frees the whole shell: close drops wry's `Retained` fields,
//! releasing the webview, its content controller, and the `wisAiFetch`
//! handler with it. The handler logs its dealloc at debug level.
//!
//! The extraction posts through `wisAiFetch` on every platform with the same
//! script bytes: a native `WKScriptMessageHandler` on macOS, a fetch-scoped
//! `win_dispatch` receiver behind its `AI_FETCH_SHIM` on Windows, and a
//! `UserContentManager` registration in `gtk_dispatch` on Linux.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// One search hit handed to the model. All fields are page-provided and
/// untrusted; `url` has passed `is_public_web_url`.
#[derive(Debug, Serialize, Clone)]
pub struct AiSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// A fetched page handed to the model. `url` is read natively from the
/// webview; `title` and `text` are page-provided and untrusted.
#[derive(Debug, Serialize, Clone)]
pub struct AiFetchResponse {
    pub url: String,
    pub title: String,
    pub text: String,
}

/// Search the web for the assistant. Scrapes the no-JS DuckDuckGo results
/// page inside the hidden webview; result URLs form the fetch allowlist on
/// the chrome side, so only public ones survive.
#[tauri::command]
pub async fn ai_web_search(
    window: tauri::Window,
    state: State<'_, AppState>,
    query: String,
) -> wis_core::WisResult<Vec<AiSearchResult>> {
    require_web_access(&state).await?;
    let query = query.trim().to_string();
    if query.is_empty() || query.chars().count() > 400 {
        return Err(wis_core::WisError::InvalidInput {
            message: "search query must be 1 to 400 characters".into(),
        });
    }

    imp::search(window, state, &query).await
}

/// Fetch one public web page's readable text for the assistant.
#[tauri::command]
pub async fn ai_web_fetch(
    window: tauri::Window,
    state: State<'_, AppState>,
    url: String,
) -> wis_core::WisResult<AiFetchResponse> {
    require_web_access(&state).await?;
    let parsed = url::Url::parse(url.trim()).map_err(|_| wis_core::WisError::InvalidInput {
        message: "not a valid URL".into(),
    })?;
    if !wis_network::is_public_web_url(&parsed) {
        return Err(wis_core::WisError::InvalidInput {
            message: "only public web URLs can be fetched".into(),
        });
    }

    imp::fetch_page(window, state, parsed).await
}

async fn require_web_access(state: &State<'_, AppState>) -> wis_core::WisResult<()> {
    let allowed = state.browser.lock().await.settings.ai.web_access;
    if allowed {
        Ok(())
    } else {
        Err(wis_core::WisError::InvalidInput {
            message: "web access is turned off in settings".into(),
        })
    }
}

/// Shape the search script posts; validated and re-capped before use.
#[derive(Debug, Deserialize)]
struct RawSearchResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    snippet: String,
}

fn cap_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Windows/Linux dispatcher entry: their `wisAiFetch` receivers route here.
/// `label` identifies the fetch webview, whose URL is read natively.
#[cfg(any(windows, target_os = "linux"))]
pub(crate) use imp::dispatch_message;

/// App handle for the Windows/Linux dispatchers to resolve a fetch webview's
/// URL. Recorded once during setup.
#[cfg(any(windows, target_os = "linux"))]
pub fn set_app_handle(app: tauri::AppHandle) {
    let _ = imp::APP.set(app);
}

mod imp {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use tauri::webview::{PageLoadEvent, WebviewBuilder};
    use tauri::{Manager, State, WebviewUrl};
    use tokio::sync::oneshot;
    use url::Url;

    use super::{cap_chars, AiFetchResponse, AiSearchResult, RawSearchResult};
    use crate::state::AppState;

    const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
    /// Logical size of the hidden webview.
    const FETCH_VIEWPORT: (f64, f64) = (1024.0, 768.0);
    /// Beat between load-finished and extraction, for client-side rendering.
    const SETTLE_DELAY: Duration = Duration::from_millis(400);
    /// Untrusted-boundary re-cap in bytes.
    const MAX_DELIVERED_BYTES: usize = 64 * 1024;
    const MAX_SEARCH_RESULTS: usize = 6;

    /// What the `wisAiFetch` handler delivers for one hidden fetch.
    pub(super) struct Fetched {
        pub url: String,
        pub title: String,
        pub text: String,
    }

    type FetchResult = Result<Fetched, String>;

    /// In-flight fetches by webview label.
    static PENDING: OnceLock<Mutex<HashMap<String, oneshot::Sender<FetchResult>>>> =
        OnceLock::new();

    fn pending() -> &'static Mutex<HashMap<String, oneshot::Sender<FetchResult>>> {
        PENDING.get_or_init(Mutex::default)
    }

    #[cfg(any(windows, target_os = "linux"))]
    pub(super) static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

    /// Deliver a Windows/Linux `wisAiFetch` message. The URL comes from the
    /// webview itself, never the page, mirroring the macOS handler.
    #[cfg(any(windows, target_os = "linux"))]
    pub(crate) fn dispatch_message(label: &str, text: String, title: String) {
        if !label.starts_with("wis-fetch-") {
            return;
        }
        let Some(app) = APP.get() else {
            return;
        };
        let Some(webview) = tauri::Manager::webviews(app).get(label).cloned() else {
            return;
        };
        let Some(url) = webview.url().ok().map(|u| u.to_string()) else {
            return;
        };

        deliver(
            label,
            Ok(Fetched {
                url,
                title,
                text: cap_bytes(&text),
            }),
        );
    }

    fn deliver(label: &str, result: FetchResult) {
        let sender = pending()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(label);
        if let Some(tx) = sender {
            let _ = tx.send(result);
        }
    }

    pub(super) async fn search(
        window: tauri::Window,
        state: State<'_, AppState>,
        query: &str,
    ) -> wis_core::WisResult<Vec<AiSearchResult>> {
        let target = Url::parse_with_params("https://html.duckduckgo.com/html/", [("q", query)])
            .expect("static search URL with encoded query is valid");
        let fetched = run_fetch(window, state, target, search_script()).await?;

        let raw: Vec<RawSearchResult> = serde_json::from_str(&fetched.text).unwrap_or_default();
        let results = raw
            .into_iter()
            .filter_map(|r| {
                let url = Url::parse(&r.url).ok()?;
                wis_network::is_public_web_url(&url).then(|| AiSearchResult {
                    title: cap_chars(r.title.trim(), 300),
                    url: url.to_string(),
                    snippet: cap_chars(r.snippet.trim(), 500),
                })
            })
            .take(MAX_SEARCH_RESULTS)
            .collect();

        Ok(results)
    }

    pub(super) async fn fetch_page(
        window: tauri::Window,
        state: State<'_, AppState>,
        target: Url,
    ) -> wis_core::WisResult<AiFetchResponse> {
        let fetched = run_fetch(window, state, target, page_script()).await?;
        if fetched.text.trim().is_empty() {
            return Err(wis_core::WisError::InvalidInput {
                message: "the page had no readable text".into(),
            });
        }

        Ok(AiFetchResponse {
            url: fetched.url,
            title: cap_chars(fetched.title.trim(), 300),
            text: fetched.text,
        })
    }

    /// Load `target` in a hidden incognito webview, run `script` after the
    /// page settles, and await the text it posts to `wisAiFetch`.
    async fn run_fetch(
        window: tauri::Window,
        state: State<'_, AppState>,
        target: Url,
        script: String,
    ) -> wis_core::WisResult<Fetched> {
        let app = tauri::Manager::app_handle(&window).clone();
        let label = format!("wis-fetch-{}", uuid::Uuid::new_v4());

        let (active_ws, user_agent) = {
            let browser = state.browser.lock().await;
            let active_ws = state
                .window_workspace(window.label())
                .unwrap_or(browser.active_workspace_id);
            let user_agent = wis_core::ua_for_host(
                target.host_str().unwrap_or_default(),
                &browser.settings.ua_site_overrides,
                &browser.settings.user_agent_profile,
                wis_core::UaPlatform::host(),
            );

            (active_ws, user_agent)
        };
        let proxy_port = state
            .ensure_proxy_for(active_ws)
            .await
            .map_err(wis_core::WisError::from)?;
        let proxy_url = format!("http://127.0.0.1:{proxy_port}")
            .parse::<Url>()
            .expect("loopback proxy URL is valid");

        let (tx, rx) = oneshot::channel();
        pending()
            .lock()
            .expect("ai_fetch pending poisoned")
            .insert(label.clone(), tx);

        let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(target))
            .incognito(true)
            .proxy_url(proxy_url)
            .on_navigation(wis_network::is_public_web_url)
            .on_download(|_, _| false);
        if let Some(ua) = &user_agent {
            builder = builder.user_agent(ua);
        }

        #[cfg(windows)]
        {
            builder = builder
                .initialization_script(crate::win_dispatch::AI_FETCH_SHIM)
                .data_directory(crate::commands::ephemeral_webview_data_dir(&app, active_ws));
        }

        {
            let app_for_load = app.clone();
            let label_for_load = label.clone();
            builder = builder.on_page_load(move |_, payload| {
                if !matches!(payload.event(), PageLoadEvent::Finished) {
                    return;
                }

                #[cfg(target_os = "macos")]
                if let Some((code, domain, _)) = wry::take_last_page_load_error() {
                    let url = payload.url().to_string();
                    if wis_core::LoadError::from_ns_error(&domain, code, url).is_some() {
                        deliver(
                            &label_for_load,
                            Err(format!("page failed to load ({domain} {code})")),
                        );
                        return;
                    }
                }

                let app = app_for_load.clone();
                let label = label_for_load.clone();
                let script = script.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(SETTLE_DELAY).await;
                    if let Some(wv) = app.webviews().get(&label) {
                        if let Err(err) = wv.eval(&script) {
                            deliver(&label, Err(format!("extraction failed: {err}")));
                        }
                    }
                });
            });
        }

        let child = window
            .add_child(
                builder,
                tauri::LogicalPosition::new(-(FETCH_VIEWPORT.0 + 100.0), 0.0),
                tauri::LogicalSize::new(FETCH_VIEWPORT.0, FETCH_VIEWPORT.1),
            )
            .map_err(|err| {
                pending()
                    .lock()
                    .expect("ai_fetch pending poisoned")
                    .remove(&label);
                wis_core::WisError::InvalidInput {
                    message: format!("fetch webview failed: {err}"),
                }
            })?;
        let _ = child.hide();

        #[cfg(target_os = "macos")]
        macos::install(&child, label.clone());
        #[cfg(windows)]
        crate::win_dispatch::install_ai_fetch_only(&child);
        #[cfg(target_os = "linux")]
        crate::gtk_dispatch::install_ai_fetch_only(&child);

        let outcome = tokio::time::timeout(FETCH_TIMEOUT, rx).await;

        pending()
            .lock()
            .expect("ai_fetch pending poisoned")
            .remove(&label);
        crate::webview_close::terminate_page(&child);
        let _ = child.close();

        match outcome {
            Ok(Ok(Ok(fetched))) => Ok(fetched),
            Ok(Ok(Err(reason))) => Err(wis_core::WisError::InvalidInput { message: reason }),
            Ok(Err(_)) => Err(wis_core::WisError::InvalidInput {
                message: "fetch interrupted".into(),
            }),
            Err(_) => Err(wis_core::WisError::InvalidInput {
                message: format!("timed out after {}s", FETCH_TIMEOUT.as_secs()),
            }),
        }
    }

    /// Truncate to at most [`MAX_DELIVERED_BYTES`], on a char boundary.
    fn cap_bytes(text: &str) -> String {
        if text.len() <= MAX_DELIVERED_BYTES {
            return text.to_string();
        }

        let mut end = MAX_DELIVERED_BYTES;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }

        text[..end].to_string()
    }

    /// Readable-text extraction, posting to `wisAiFetch`. Caps at 8000 chars.
    fn page_script() -> String {
        format!(
            "{}\n{}",
            crate::reader::MAIN_CONTENT_FINDER,
            r#"(function () {
  try {
    var best = __wisFindMainContent();
    var text = ((best && (best.innerText || best.textContent)) || '').replace(/\s+/g, ' ').trim();
    if (text.length > 8000) text = text.slice(0, 8000);
    var title = (document.title || '').trim();
    if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisAiFetch) {
      window.webkit.messageHandlers.wisAiFetch.postMessage({ text: text, title: title });
    }
  } catch (e) {}
})();"#
        )
    }

    /// Scrape the no-JS DuckDuckGo results page into a JSON array and post it
    /// as the text payload. Ad rows are skipped.
    fn search_script() -> String {
        r#"(function () {
  function post(text) {
    if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisAiFetch) {
      window.webkit.messageHandlers.wisAiFetch.postMessage({ text: text, title: '' });
    }
  }
  try {
    var out = [];
    var rows = document.querySelectorAll('.result');
    for (var i = 0; i < rows.length && out.length < 6; i++) {
      var row = rows[i];
      if (row.className.indexOf('result--ad') !== -1) continue;
      var a = row.querySelector('.result__a');
      if (!a) continue;
      var href = a.getAttribute('href') || '';
      var m = href.match(/[?&]uddg=([^&]+)/);
      if (m) { try { href = decodeURIComponent(m[1]); } catch (e) {} }
      else if (href.slice(0, 2) === '//') { href = 'https:' + href; }
      var sn = row.querySelector('.result__snippet');
      out.push({
        title: (a.textContent || '').trim().slice(0, 300),
        url: href.slice(0, 2000),
        snippet: ((sn && sn.textContent) || '').trim().slice(0, 500)
      });
    }
    post(JSON.stringify(out));
  } catch (e) { post('[]'); }
})();"#
            .to_string()
    }

    /// The narrow `wisAiFetch` handler on a fetch webview. Same ObjC glue as
    /// `page_text.rs`, delivering to the pending map instead of an event.
    #[cfg(target_os = "macos")]
    mod macos {
        use std::ffi::{CStr, CString};
        use std::os::raw::c_char;

        use objc2::rc::Id;
        use objc2::runtime::{AnyObject, NSObject};
        use objc2::{
            class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
        };

        use super::{cap_bytes, deliver, Fetched};

        struct HandlerIvars {
            label: String,
        }

        impl Drop for HandlerIvars {
            fn drop(&mut self) {
                log::debug!("wisAiFetch handler dropped for {}", self.label);
            }
        }

        declare_class!(
            struct AiFetchHandler;

            unsafe impl ClassType for AiFetchHandler {
                type Super = NSObject;
                type Mutability = mutability::InteriorMutable;
                const NAME: &'static str = "WisAiFetchHandler";
            }

            impl DeclaredClass for AiFetchHandler {
                type Ivars = HandlerIvars;
            }

            unsafe impl AiFetchHandler {
                #[method(userContentController:didReceiveScriptMessage:)]
                fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                    let label = self.ivars().label.clone();
                    unsafe { handle_message(&label, message) }
                }
            }
        );

        impl AiFetchHandler {
            fn new(label: String) -> Id<Self> {
                let this = Self::alloc().set_ivars(HandlerIvars { label });
                unsafe { msg_send_id![super(this), init] }
            }
        }

        unsafe fn ns_string(s: &str) -> *mut AnyObject {
            let c = CString::new(s).unwrap_or_default();
            msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
        }

        unsafe fn nsstring_to_string(ns: *mut AnyObject) -> Option<String> {
            if ns.is_null() {
                return None;
            }

            let is_str: bool = msg_send![ns, isKindOfClass: class!(NSString)];
            if !is_str {
                return None;
            }

            let utf8: *const c_char = msg_send![ns, UTF8String];
            if utf8.is_null() {
                return None;
            }

            Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
        }

        unsafe fn dict_string(dict: *mut AnyObject, key: &str) -> Option<String> {
            let k = ns_string(key);
            let v: *mut AnyObject = msg_send![dict, objectForKey: k];
            nsstring_to_string(v)
        }

        unsafe fn handle_message(label: &str, message: *mut AnyObject) {
            if message.is_null() {
                return;
            }

            let frame: *mut AnyObject = msg_send![message, frameInfo];
            if frame.is_null() {
                return;
            }
            let is_main_frame: bool = msg_send![frame, isMainFrame];
            if !is_main_frame {
                return;
            }

            let webview: *mut AnyObject = msg_send![message, webView];
            if webview.is_null() {
                return;
            }
            let url_obj: *mut AnyObject = msg_send![webview, URL];
            if url_obj.is_null() {
                return;
            }
            let url_ns: *mut AnyObject = msg_send![url_obj, absoluteString];
            let Some(url) = nsstring_to_string(url_ns) else {
                return;
            };

            let body: *mut AnyObject = msg_send![message, body];
            if body.is_null() {
                return;
            }
            let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
            if !is_dict {
                return;
            }
            let Some(text) = dict_string(body, "text") else {
                return;
            };
            let title = dict_string(body, "title").unwrap_or_default();

            deliver(
                label,
                Ok(Fetched {
                    url,
                    title,
                    text: cap_bytes(&text),
                }),
            );
        }

        /// Register the `wisAiFetch` message handler on a fetch webview.
        pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, label: String) {
            let _ = webview.with_webview(move |w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    return;
                }

                let config: *mut AnyObject = msg_send![wk, configuration];
                if config.is_null() {
                    return;
                }

                let controller: *mut AnyObject = msg_send![config, userContentController];
                if controller.is_null() {
                    return;
                }

                let handler = AiFetchHandler::new(label.clone());
                let name = ns_string("wisAiFetch");
                let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
            });
        }
    }
}
