//! Canvas tabs: a bundled page on the `wis-canvas` scheme, without Tauri IPC,
//! that reads and writes its workspace's canvases over the narrow `wisCanvas`
//! channel. Messages run one at a time, so saves land in order; documents
//! reach the disk every [`FLUSH_EVERY`], when a canvas tab closes, and on quit.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::http::{self, header::CONTENT_TYPE};
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder};
use url::Url;
use wis_core::{CanvasId, WisError, WisResult};
use wis_storage::Storage;

use crate::canvas_store::{CanvasMemory, CanvasTab};
use crate::state::AppState;

pub use wis_core::canvas::SCHEME;
const FLUSH_EVERY: Duration = Duration::from_secs(120);
/// A whole document plus its envelope.
const MAX_MESSAGE_BYTES: usize = wis_core::canvas::MAX_DOC_BYTES + 64 * 1024;

/// The canvas page runs only its own bundle: no network, frames or forms.
const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; font-src 'self' data:; connect-src 'none'; frame-src 'none'; \
     base-uri 'none'; form-action 'none'";

static TABS: LazyLock<Mutex<HashMap<String, CanvasTab>>> = LazyLock::new(Default::default);
static MEMORY: LazyLock<Mutex<CanvasMemory>> = LazyLock::new(Default::default);
static WORKER: OnceLock<tokio::sync::mpsc::Sender<(String, String)>> = OnceLock::new();
/// Messages waiting for the worker; a flooding page loses the excess.
const QUEUE: usize = 32;

/// The theme a canvas page starts with, set before its scripts run.
pub fn theme_script(theme: wis_core::canvas::CanvasTheme) -> String {
    format!(
        "window.__wisCanvasTheme={};",
        serde_json::to_string(theme.as_str()).unwrap_or_default()
    )
}

/// Restyle every open canvas tab.
pub fn push_theme(app: &AppHandle, theme: wis_core::canvas::CanvasTheme) {
    let script = theme_script(theme);
    let labels: Vec<String> = TABS
        .lock()
        .expect("canvas tabs poisoned")
        .keys()
        .cloned()
        .collect();
    for label in labels {
        if let Some(webview) = app.get_webview(&label) {
            if let Err(err) = webview.eval(&script) {
                log::warn!("restyling a canvas tab: {err}");
            }
        }
    }
}

pub fn is_canvas_url(url: &Url) -> bool {
    url.scheme() == SCHEME
}

/// Canvas tabs stay on the canvas page, and web tabs never reach it.
pub fn navigation_allowed(canvas_tab: bool, url: &Url) -> bool {
    if is_canvas_url(url) {
        canvas_tab
    } else {
        !canvas_tab || url.as_str() == "about:blank"
    }
}

pub fn register(label: &str, tab: CanvasTab) {
    TABS.lock()
        .expect("canvas tabs poisoned")
        .insert(label.to_string(), tab);
}

/// A canvas tab closed: forget it and write what it saved.
pub fn forget(app: &AppHandle, label: &str) {
    let known = TABS
        .lock()
        .expect("canvas tabs poisoned")
        .remove(label)
        .is_some();
    if known {
        flush_soon(app);
    }
}

fn tab_for(label: &str) -> Option<CanvasTab> {
    TABS.lock()
        .expect("canvas tabs poisoned")
        .get(label)
        .copied()
}

/// A deleted workspace's canvases: drop what is in memory and on disk.
pub async fn release_workspace(app: &AppHandle, workspace: wis_core::WorkspaceId) {
    MEMORY
        .lock()
        .expect("canvas memory poisoned")
        .release_workspace(workspace);
    let storage = app.state::<AppState>().storage.clone();
    match tokio::task::spawn_blocking(move || storage.delete_workspace_canvases(workspace)).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => log::warn!("deleting a removed workspace's canvases: {err}"),
        Err(err) => log::warn!("deleting a removed workspace's canvases panicked: {err}"),
    }
}

/// Write pending documents now. Safe from the exit hook.
pub fn flush_now(storage: &dyn Storage) -> WisResult<()> {
    MEMORY
        .lock()
        .expect("canvas memory poisoned")
        .flush(storage)
}

/// [`flush_now`] for callers with nobody to tell.
pub fn flush_logged(storage: &dyn Storage) {
    if let Err(err) = flush_now(storage) {
        log::warn!("writing canvases: {err}");
    }
}

fn flush_soon(app: &AppHandle) {
    let storage = app.state::<AppState>().storage.clone();
    tauri::async_runtime::spawn_blocking(move || flush_logged(storage.as_ref()));
}

/// Start the message worker and the periodic write.
pub fn start(app: AppHandle) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, String)>(QUEUE);
    if WORKER.set(tx).is_err() {
        return;
    }

    let worker_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some((label, text)) = rx.recv().await {
            let storage = worker_app.state::<AppState>().storage.clone();
            let from = label.clone();
            let now = chrono::Utc::now().timestamp_millis();
            match tauri::async_runtime::spawn_blocking(move || {
                handle(storage.as_ref(), &from, &text, now)
            })
            .await
            {
                Ok(reply) => deliver(&worker_app, &label, &reply),
                Err(err) => log::warn!("canvas message panicked: {err}"),
            }
        }
    });

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(FLUSH_EVERY).await;
            let storage = app.state::<AppState>().storage.clone();
            if let Err(err) =
                tokio::task::spawn_blocking(move || flush_logged(storage.as_ref())).await
            {
                log::warn!("writing canvases panicked: {err}");
            }
        }
    });
}

/// Queue a message a canvas page posted.
fn receive(label: String, text: String) {
    if text.len() > MAX_MESSAGE_BYTES {
        log::warn!("dropped an oversized canvas message from {label}");
        return;
    }
    if let Some(worker) = WORKER.get() {
        if let Err(err) = worker.try_send((label, text)) {
            log::warn!("dropped a canvas message: {err}");
        }
    }
}

fn deliver(app: &AppHandle, label: &str, reply: &str) {
    let Some(webview) = app.get_webview(label) else {
        return;
    };
    if let Err(err) = webview.eval(format!(
        "window.__wisCanvas&&window.__wisCanvas.reply({reply})"
    )) {
        log::warn!("replying to a canvas page: {err}");
    }
}

#[derive(Debug, Deserialize)]
struct Envelope {
    id: u64,
    #[serde(flatten)]
    request: Request,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Request {
    List,
    Load { canvas: CanvasId },
    Create { name: Option<String> },
    Rename { canvas: CanvasId, name: String },
    Save { canvas: CanvasId, doc: String },
    Delete { canvas: CanvasId },
}

/// Run one page message and return the reply the page receives.
fn handle(storage: &dyn Storage, label: &str, text: &str, now: i64) -> String {
    let (id, result) = match serde_json::from_str::<Envelope>(text) {
        Ok(envelope) => (
            Value::from(envelope.id),
            run(storage, label, envelope.request, now),
        ),
        Err(_) => (
            Value::Null,
            Err(WisError::InvalidInput {
                message: "the canvas page sent a message Wis could not read".into(),
            }),
        ),
    };
    let reply = match result {
        Ok(value) => json!({ "id": id, "ok": true, "value": value }),
        Err(err) => json!({ "id": id, "ok": false, "error": user_message(&err) }),
    };
    reply.to_string()
}

fn run(storage: &dyn Storage, label: &str, request: Request, now: i64) -> WisResult<Value> {
    let tab = tab_for(label).ok_or_else(|| WisError::InvalidInput {
        message: "this tab is not a canvas".into(),
    })?;
    let mut memory = MEMORY.lock().expect("canvas memory poisoned");
    Ok(match request {
        Request::List => json!(memory.list(storage, tab)?),
        Request::Load { canvas } => {
            let canvas = memory.load(storage, tab, canvas)?;
            json!({ "id": canvas.id, "name": canvas.name, "doc": canvas.doc, "updatedAt": canvas.updated_at })
        }
        Request::Create { name } => json!(memory.create(storage, tab, name.as_deref(), now)?),
        Request::Rename { canvas, name } => json!(memory.rename(storage, tab, canvas, &name, now)?),
        Request::Save { canvas, doc } => {
            memory.save(tab, canvas, doc, now)?;
            Value::Null
        }
        Request::Delete { canvas } => {
            memory.delete(storage, tab, canvas)?;
            Value::Null
        }
    })
}

fn user_message(err: &WisError) -> String {
    match err {
        WisError::InvalidInput { message } => message.clone(),
        WisError::PersistenceFailed { reason } => {
            log::warn!("canvas storage: {reason}");
            "Wis could not reach its canvas store.".into()
        }
        other => other.to_string(),
    }
}

/// `wis-canvas://localhost/...`: the canvas page and its built assets, only
/// for registered canvas tabs.
pub fn handle_request(
    ctx: UriSchemeContext<'_, tauri::Wry>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    if tab_for(ctx.webview_label()).is_none() {
        responder.respond(status(403, "forbidden"));
        return;
    }

    let app = ctx.app_handle().clone();
    let uri = request.uri().clone();
    tauri::async_runtime::spawn(async move {
        let response = if tauri::is_dev() {
            dev_asset(&app, &uri).await
        } else {
            bundled_asset(&app, uri.path())
        };
        responder.respond(response);
    });
}

/// The bundled file a canvas page may load: the page itself or a built asset.
fn asset_path(path: &str) -> Option<String> {
    if path.is_empty() || path == "/" {
        return Some("canvas.html".into());
    }

    let rest = path.strip_prefix("/assets/")?;
    let safe = rest
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'));

    safe.then(|| format!("assets/{rest}"))
}

fn bundled_asset(app: &AppHandle, path: &str) -> http::Response<Vec<u8>> {
    let Some(file) = asset_path(path) else {
        return status(404, "not found");
    };
    let page = file == "canvas.html";
    match app.asset_resolver().get(file) {
        // A missing asset resolves to the chrome's own page; never serve it here.
        Some(asset) if page || !asset.mime_type.starts_with("text/html") => {
            respond(asset.bytes, &asset.mime_type, PAGE_CSP)
        }
        _ => status(404, "not found"),
    }
}

/// A path the dev server may be asked for: never another host or a parent.
fn dev_path(path: &str) -> Option<&str> {
    if path == "/" {
        return Some("/canvas.html");
    }

    (path.starts_with('/') && !path.starts_with("//") && !path.contains("..")).then_some(path)
}

/// Development: the page and its modules come from the dev server.
async fn dev_asset(app: &AppHandle, uri: &http::Uri) -> http::Response<Vec<u8>> {
    let Some(base) = app.config().build.dev_url.clone() else {
        return status(500, "no dev server");
    };
    let Some(path) = dev_path(uri.path()) else {
        return status(400, "bad request");
    };
    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let Ok(target) = base.join(&format!("{path}{query}")) else {
        return status(400, "bad request");
    };
    let origin = base.origin().ascii_serialization();
    let ws_origin = origin.replacen("http", "ws", 1);
    let csp = format!(
        "default-src 'self' 'unsafe-inline' 'unsafe-eval' data: {origin} {ws_origin}; \
         frame-src 'none'; base-uri 'none'; form-action 'none'"
    );
    let fetched = async {
        let response = reqwest::get(target).await?;
        let mime = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let ok = response.status().is_success();
        Ok::<_, reqwest::Error>((ok, mime, response.bytes().await?.to_vec()))
    };
    match fetched.await {
        Ok((true, mime, bytes)) => respond(bytes, &mime, &csp),
        Ok((false, _, _)) => status(404, "not found"),
        Err(err) => {
            log::warn!("canvas dev asset: {err}");
            status(502, "dev server unavailable")
        }
    }
}

fn respond(body: Vec<u8>, mime: &str, csp: &str) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(200)
        .header(CONTENT_TYPE, mime)
        .header("Content-Security-Policy", csp)
        .header("X-Content-Type-Options", "nosniff")
        .body(body)
        .unwrap_or_else(|_| status(500, "bad response"))
}

fn status(code: u16, msg: &str) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(code)
        .body(msg.as_bytes().to_vec())
        .expect("static response")
}

#[cfg(target_os = "macos")]
pub use macos::install;

/// The `wisCanvas` handler: main-frame messages from a page still on the
/// canvas scheme, read as one JSON string.
#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use super::{receive, SCHEME};

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct CanvasHandler;

        unsafe impl ClassType for CanvasHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisCanvasHandler";
        }

        impl DeclaredClass for CanvasHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl CanvasHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                if let Some(text) = unsafe { message_text(message) } {
                    receive(label, text);
                }
            }
        }
    );

    impl CanvasHandler {
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

    unsafe fn message_text(message: *mut AnyObject) -> Option<String> {
        if message.is_null() {
            return None;
        }
        let frame: *mut AnyObject = msg_send![message, frameInfo];
        if frame.is_null() {
            return None;
        }
        let is_main_frame: bool = msg_send![frame, isMainFrame];
        if !is_main_frame {
            return None;
        }

        let webview: *mut AnyObject = msg_send![message, webView];
        if webview.is_null() {
            return None;
        }
        let url_obj: *mut AnyObject = msg_send![webview, URL];
        if url_obj.is_null() {
            return None;
        }
        let url = nsstring_to_string(msg_send![url_obj, absoluteString])?;
        if !url.starts_with(&format!("{SCHEME}:")) {
            return None;
        }

        nsstring_to_string(msg_send![message, body])
    }

    /// Register `wisCanvas` on a canvas tab, in the page's own world.
    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, label: String) {
        let result = webview.with_webview(move |w| unsafe {
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
            let handler = CanvasHandler::new(label);
            let name = ns_string("wisCanvas");
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
        });
        if let Err(err) = result {
            log::warn!("installing the canvas channel: {err}");
        }
    }

    #[cfg(test)]
    mod tests {
        use objc2::runtime::{AnyObject, Bool};
        use objc2::{class, sel};

        type Obj = *mut AnyObject;

        /// A wrong message type aborts at runtime only, so check each send
        /// against its method.
        #[test]
        fn every_send_matches_its_method() {
            let message = class!(WKScriptMessage);
            message.verify_sel::<(), Obj>(sel!(frameInfo)).unwrap();
            message.verify_sel::<(), Obj>(sel!(webView)).unwrap();
            message.verify_sel::<(), Obj>(sel!(body)).unwrap();
            class!(WKFrameInfo)
                .verify_sel::<(), Bool>(sel!(isMainFrame))
                .unwrap();
            class!(WKWebView).verify_sel::<(), Obj>(sel!(URL)).unwrap();
            class!(WKWebView)
                .verify_sel::<(), Obj>(sel!(configuration))
                .unwrap();
            class!(WKWebViewConfiguration)
                .verify_sel::<(), Obj>(sel!(userContentController))
                .unwrap();
            class!(WKUserContentController)
                .verify_sel::<(Obj, Obj), ()>(sel!(addScriptMessageHandler:name:))
                .unwrap();
            class!(NSURL)
                .verify_sel::<(), Obj>(sel!(absoluteString))
                .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wis_storage::SqliteStorage;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn canvas_tabs_stay_on_the_canvas_and_web_tabs_never_reach_it() {
        assert!(navigation_allowed(
            true,
            &url("wis-canvas://localhost/?canvas=x")
        ));
        assert!(navigation_allowed(true, &url("about:blank")));
        assert!(!navigation_allowed(true, &url("https://example.com/")));
        assert!(!navigation_allowed(false, &url("wis-canvas://localhost/")));
        assert!(navigation_allowed(false, &url("https://example.com/")));
    }

    #[test]
    fn only_the_page_and_its_built_assets_are_served() {
        assert_eq!(asset_path("/").as_deref(), Some("canvas.html"));
        assert_eq!(
            asset_path("/assets/canvas-AbC_1.js").as_deref(),
            Some("assets/canvas-AbC_1.js")
        );
        for refused in [
            "/index.html",
            "/canvas.html",
            "/assets/../index.html",
            "/assets/",
            "/assets/a//b.js",
            "/assets/%2e%2e/x",
        ] {
            assert_eq!(asset_path(refused), None, "{refused} must be refused");
        }
    }

    #[test]
    fn the_theme_script_sets_one_quoted_word() {
        use wis_core::canvas::CanvasTheme;
        assert_eq!(
            theme_script(CanvasTheme::Light),
            r#"window.__wisCanvasTheme="light";"#
        );
        assert_eq!(
            theme_script(CanvasTheme::Dark),
            r#"window.__wisCanvasTheme="dark";"#
        );
    }

    #[test]
    fn the_dev_server_is_asked_only_for_its_own_paths() {
        assert_eq!(dev_path("/"), Some("/canvas.html"));
        assert_eq!(
            dev_path("/src/canvas/main.tsx"),
            Some("/src/canvas/main.tsx")
        );
        for refused in ["//evil.example/x", "/src/../../etc/passwd", "relative"] {
            assert_eq!(dev_path(refused), None, "{refused} must be refused");
        }
    }

    #[test]
    fn messages_run_only_for_registered_canvas_tabs_and_reply_by_id() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let label = format!("tab-test-{}", uuid::Uuid::new_v4());
        let stranger: Value =
            serde_json::from_str(&handle(&storage, &label, r#"{"id":1,"op":"list"}"#, 1)).unwrap();
        assert_eq!(stranger["ok"], false);

        register(
            &label,
            CanvasTab {
                workspace: wis_core::WorkspaceId::new(),
                ephemeral: false,
            },
        );
        let created: Value = serde_json::from_str(&handle(
            &storage,
            &label,
            r#"{"id":2,"op":"create","name":"Plans"}"#,
            5,
        ))
        .unwrap();
        assert_eq!(
            (created["id"].as_u64(), created["ok"].as_bool()),
            (Some(2), Some(true))
        );
        assert_eq!(created["value"]["name"], "Plans");

        let canvas = created["value"]["id"].as_str().unwrap().to_string();
        let save =
            json!({ "id": 3, "op": "save", "canvas": canvas, "doc": "{\"cards\":[]}" }).to_string();
        assert_eq!(
            serde_json::from_str::<Value>(&handle(&storage, &label, &save, 6)).unwrap()["ok"],
            true
        );
        let load = json!({ "id": 4, "op": "load", "canvas": canvas }).to_string();
        let loaded: Value = serde_json::from_str(&handle(&storage, &label, &load, 7)).unwrap();
        assert_eq!(loaded["value"]["doc"], "{\"cards\":[]}");

        let garbled: Value = serde_json::from_str(&handle(&storage, &label, "{nope", 8)).unwrap();
        assert_eq!(
            (garbled["id"].clone(), garbled["ok"].clone()),
            (Value::Null, Value::Bool(false))
        );
        TABS.lock().unwrap().remove(&label);
    }
}
