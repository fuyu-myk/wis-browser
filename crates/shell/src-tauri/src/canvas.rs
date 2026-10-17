//! Canvas tabs: a bundled page on `wis-canvas` without Tauri IPC, reading and
//! writing its workspace's canvases over `wisCanvas`, one message at a time.

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

use crate::canvas_health::{CardHealth, Pulse};
use crate::canvas_store::{CanvasMemory, CanvasTab};
use crate::state::AppState;

pub use wis_core::canvas::SCHEME;
const FLUSH_EVERY: Duration = Duration::from_secs(120);
const MAX_MESSAGE_BYTES: usize = wis_core::canvas::MAX_DOC_BYTES + 64 * 1024;

const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; font-src 'self' data:; connect-src 'none'; \
     frame-src wis-canvas://localhost/frame; base-uri 'none'; form-action 'none'";

/// The document an HTML card runs in: it takes the card's HTML from the canvas
/// page once, and may run scripts but never reach the network or the page.
const FRAME_PATH: &str = "/frame";
const FRAME_HTML: &str = include_str!("canvas_frame.html");
const FRAME_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; \
     style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; \
     connect-src 'none'; form-action 'none'; base-uri 'none'; sandbox allow-scripts";

static TABS: LazyLock<Mutex<HashMap<String, CanvasTab>>> = LazyLock::new(Default::default);
static MEMORY: LazyLock<Mutex<CanvasMemory>> = LazyLock::new(Default::default);
static HEALTH: LazyLock<Mutex<CardHealth>> = LazyLock::new(Default::default);
static GROUND: LazyLock<Mutex<HashMap<String, Ground>>> = LazyLock::new(Default::default);
const MAX_GROUND: f64 = 24.0;
const MAX_RADIUS: f64 = 40.0;
const MAX_GROUNDS: usize = 256;

/// How far a canvas tab reaches below its pane, and the pane's corner radius.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct Ground {
    pub bottom: f64,
    pub radius: f64,
}

impl Ground {
    fn new(bottom: f64, radius: f64) -> Self {
        let tidy = |n: f64, max: f64| {
            if n.is_finite() {
                n.clamp(0.0, max).round()
            } else {
                0.0
            }
        };
        Self {
            bottom: tidy(bottom, MAX_GROUND),
            radius: tidy(radius, MAX_RADIUS),
        }
    }
}

/// Records a canvas tab's ground and hands it to the page, which also asks for
/// it when it starts: this can land before the tab exists, and outlives a
/// sleeping tab's webview.
pub fn set_ground(app: &AppHandle, label: &str, bottom: f64, radius: f64) {
    if !label.starts_with("tab-") || label.len() > 64 {
        return;
    }

    let ground = Ground::new(bottom, radius);
    let changed = {
        let mut grounds = GROUND.lock().expect("canvas ground poisoned");
        if grounds.len() >= MAX_GROUNDS && !grounds.contains_key(label) {
            let open = TABS.lock().expect("canvas tabs poisoned");
            grounds.retain(|known, _| open.contains_key(known));
        }
        grounds.insert(label.to_string(), ground) != Some(ground)
    };
    let Some(webview) = app.get_webview(label) else {
        return;
    };
    if !changed || !webview.url().is_ok_and(|url| is_canvas_url(&url)) {
        return;
    }

    let script = format!(
        r#"window.__wisCanvasGround="{},{}";"#,
        ground.bottom, ground.radius
    );
    if let Err(err) = webview.eval(script) {
        log::warn!("pushing a canvas ground: {err}");
    }
}

fn ground_of(label: &str) -> Ground {
    GROUND
        .lock()
        .expect("canvas ground poisoned")
        .get(label)
        .copied()
        .unwrap_or_default()
}
static WORKER: OnceLock<tokio::sync::mpsc::Sender<(String, String)>> = OnceLock::new();
const QUEUE: usize = 32;

/// The side a canvas's props panel takes: away from the sidebar, or from the
/// rail in the top-tabs layout.
fn panel_side(settings: &wis_core::UserSettings) -> &'static str {
    let sidebar = match settings.layout {
        wis_core::LayoutMode::SidebarLeft => wis_core::RailSide::Left,
        wis_core::LayoutMode::SidebarRight => wis_core::RailSide::Right,
        wis_core::LayoutMode::TopTabs => settings.top_tabs_rail_side,
    };
    match sidebar {
        wis_core::RailSide::Left => "right",
        wis_core::RailSide::Right => "left",
    }
}

/// What a canvas page reads from the settings: its theme and the side for
/// its props panel.
pub fn page_script(settings: &wis_core::UserSettings) -> String {
    let quoted = |word: &str| serde_json::to_string(word).unwrap_or_default();
    format!(
        "window.__wisCanvasTheme={};window.__wisCanvasSide={};",
        quoted(settings.canvas_theme.as_str()),
        quoted(panel_side(settings))
    )
}

/// The native appearance a canvas tab takes.
pub fn appearance(theme: wis_core::canvas::CanvasTheme) -> wis_core::ThemeMode {
    match theme {
        wis_core::canvas::CanvasTheme::Dark => wis_core::ThemeMode::Dark,
        wis_core::canvas::CanvasTheme::Light => wis_core::ThemeMode::Light,
    }
}

pub fn is_canvas_tab(label: &str) -> bool {
    tab_for(label).is_some()
}

pub fn push_page(app: &AppHandle, settings: &wis_core::UserSettings) {
    let script = page_script(settings);
    let look = appearance(settings.canvas_theme);
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
            crate::commands::apply_tab_appearance(&webview, look);
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

/// Also writes what the tab saved.
pub fn forget(app: &AppHandle, label: &str) {
    let known = TABS
        .lock()
        .expect("canvas tabs poisoned")
        .remove(label)
        .is_some();
    #[cfg(target_os = "linux")]
    linux::forget(label);
    if known {
        crate::dictation::forget(app, label);
        let mut health = HEALTH.lock().expect("canvas health poisoned");
        health.closed(label, chrono::Utc::now().timestamp_millis());
        if health.take_changed() {
            save_health(app, health.stuck_canvases());
        }
        drop(health);
        flush_soon(app);
    }
}

const HEALTH_FILE: &str = "canvas-stuck.json";

fn health_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(HEALTH_FILE))
}

/// Canvases whose cards froze, kept so the next launch opens them paused.
fn save_health(app: &AppHandle, stuck: Vec<CanvasId>) {
    let Some(path) = health_path(app) else {
        return;
    };
    tauri::async_runtime::spawn_blocking(move || {
        let result = if stuck.is_empty() {
            match std::fs::remove_file(&path) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        } else {
            serde_json::to_vec(&stuck)
                .map_err(std::io::Error::other)
                .and_then(|bytes| std::fs::write(&path, bytes))
        };
        if let Err(err) = result {
            log::warn!("saving stuck canvases: {err}");
        }
    });
}

fn load_health(app: &AppHandle) {
    let Some(path) = health_path(app) else {
        return;
    };
    let stuck = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Vec<CanvasId>>(&bytes).unwrap_or_default(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            log::warn!("reading stuck canvases: {err}");
            return;
        }
    };
    HEALTH
        .lock()
        .expect("canvas health poisoned")
        .restore(stuck);
}

/// From the exit hook: canvases still frozen open paused next launch.
pub fn quit(app: &AppHandle) {
    let mut health = HEALTH.lock().expect("canvas health poisoned");
    health.quit(chrono::Utc::now().timestamp_millis());
    if !health.take_changed() {
        return;
    }

    let Some(path) = health_path(app) else {
        return;
    };
    match serde_json::to_vec(&health.stuck_canvases()) {
        Ok(bytes) => {
            if let Err(err) = std::fs::write(path, bytes) {
                log::warn!("saving stuck canvases: {err}");
            }
        }
        Err(err) => log::warn!("saving stuck canvases: {err}"),
    }
}

fn tab_for(label: &str) -> Option<CanvasTab> {
    TABS.lock()
        .expect("canvas tabs poisoned")
        .get(label)
        .copied()
}

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

/// Safe from the exit hook.
pub fn flush_now(storage: &dyn Storage) -> WisResult<()> {
    MEMORY
        .lock()
        .expect("canvas memory poisoned")
        .flush(storage)
}

pub fn flush_logged(storage: &dyn Storage) {
    if let Err(err) = flush_now(storage) {
        log::warn!("writing canvases: {err}");
    }
}

fn flush_soon(app: &AppHandle) {
    let storage = app.state::<AppState>().storage.clone();
    tauri::async_runtime::spawn_blocking(move || flush_logged(storage.as_ref()));
}

pub fn start(app: AppHandle) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, String)>(QUEUE);
    if WORKER.set(tx).is_err() {
        return;
    }
    load_health(&app);
    crate::canvas_ai::start(app.clone());

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
            let mut health = HEALTH.lock().expect("canvas health poisoned");
            if health.take_changed() {
                save_health(&worker_app, health.stuck_canvases());
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

/// The page writes `{"id":...` first, so a message too big or too broken to
/// read can still be answered.
pub(crate) fn head_id(text: &str) -> Option<u64> {
    let rest = text.strip_prefix(r#"{"id":"#)?;
    let digits = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..digits].parse().ok()
}

fn refuse(label: &str, text: &str, message: &str) {
    let (Some(app), Some(id)) = (crate::canvas_ai::app(), head_id(text)) else {
        return;
    };
    let reply = json!({ "id": id, "ok": false, "error": message });
    deliver(app, label, &reply.to_string());
}

fn receive(label: String, text: String) {
    let limit = if crate::canvas_ai::is_document_request(&text) {
        crate::canvas_ai::MAX_DOCUMENT_MESSAGE
    } else {
        MAX_MESSAGE_BYTES
    };
    if text.len() > limit {
        log::warn!("dropped an oversized canvas message from {label}");
        refuse(
            &label,
            text.get(..32).unwrap_or(""),
            "That is too much for one canvas message.",
        );
        return;
    }
    if crate::canvas_ai::is_ai_request(&text) {
        match tab_for(&label) {
            Some(tab) => crate::canvas_ai::receive(label, tab, text),
            None => refuse(&label, &text, "this tab is not a canvas"),
        }
        return;
    }
    if let Some(worker) = WORKER.get() {
        if let Err(err) = worker.try_send((label, text)) {
            log::warn!("dropped a canvas message: {err}");
        }
    }
}

pub(crate) fn deliver(app: &AppHandle, label: &str, reply: &str) {
    let Some(webview) = app.get_webview(label) else {
        return;
    };
    if !webview.url().is_ok_and(|url| is_canvas_url(&url)) {
        return;
    }
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
    Pulse { canvas: CanvasId, state: Pulse },
    Ground,
}

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
    if let Request::Ground = request {
        return Ok(json!(ground_of(label)));
    }
    if let Request::Pulse { canvas, state } = request {
        let safe = HEALTH
            .lock()
            .expect("canvas health poisoned")
            .pulse(canvas, label, state, now);
        return Ok(json!({ "safe": safe }));
    }
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
        Request::Pulse { .. } | Request::Ground => Value::Null,
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

/// Answers registered canvas tabs only.
pub fn handle_request(
    ctx: UriSchemeContext<'_, tauri::Wry>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    if tab_for(ctx.webview_label()).is_none() {
        responder.respond(status(403, "forbidden"));
        return;
    }

    if request.uri().path() == FRAME_PATH {
        responder.respond(frame_page());
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

/// Only the page itself or a built asset.
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

fn frame_page() -> http::Response<Vec<u8>> {
    respond(FRAME_HTML.as_bytes().to_vec(), "text/html", FRAME_CSP)
}

fn dev_csp(origin: &str) -> String {
    let ws_origin = origin.replacen("http", "ws", 1);
    format!(
        "default-src 'self' 'unsafe-inline' 'unsafe-eval' data: {origin} {ws_origin}; \
         frame-src {SCHEME}://localhost{FRAME_PATH}; base-uri 'none'; form-action 'none'"
    )
}

fn dev_path(path: &str) -> Option<&str> {
    if path == "/" {
        return Some("/canvas.html");
    }

    (path.starts_with('/') && !path.starts_with("//") && !path.contains("..")).then_some(path)
}

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
    let csp = dev_csp(&base.origin().ascii_serialization());
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

/// A message's body when it opens with `token` and a newline; nothing when
/// the token is missing or another one.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn unseal<'a>(token: &str, text: &'a str) -> Option<&'a str> {
    let (sent, body) = text.split_once('\n')?;
    (!token.is_empty() && sent == token).then_some(body)
}

#[cfg(target_os = "linux")]
pub use linux::{install, token_script};

/// The canvas channel on Linux. WebKitGTK's script messages carry no frame,
/// so the canvas page proves it is the main frame with a per-tab token: only
/// the main frame's start script holds it, and cards are another origin. The
/// same webview settings that macOS sets keep cards off the network.
#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::sync::{LazyLock, Mutex};

    use javascriptcore::ValueExt;
    use webkit2gtk::glib::object::{ObjectExt, ObjectType};
    use webkit2gtk::{SettingsExt, UserContentManagerExt, WebViewExt};

    const HANDLER: &str = "wisCanvas";
    /// The WebKit features macOS turns off in `canvas_webview`, which bypass
    /// CSP: WebRTC's STUN, and preconnect and DNS prefetch links.
    const NETWORK_FEATURES: [&str; 3] = [
        "PeerConnectionEnabled",
        "LinkPreconnect",
        "LinkDNSPrefetchEnabled",
    ];

    static TOKENS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

    /// The start script that hands the canvas page its tab's token.
    pub fn token_script(label: &str) -> String {
        let token = uuid::Uuid::new_v4().simple().to_string();
        if let Ok(mut tokens) = TOKENS.lock() {
            tokens.insert(label.to_string(), token.clone());
        }
        format!(
            "window.__wisCanvasToken={};",
            serde_json::to_string(&token).unwrap_or_default()
        )
    }

    pub fn forget(label: &str) {
        if let Ok(mut tokens) = TOKENS.lock() {
            tokens.remove(label);
        }
    }

    /// The message without its token, if it carries this tab's.
    fn unsealed(label: &str, text: &str) -> Option<String> {
        let tokens = TOKENS.lock().ok()?;
        super::unseal(tokens.get(label)?, text).map(str::to_string)
    }

    type AllFeatures = unsafe extern "C" fn() -> *mut c_void;
    type ListLength = unsafe extern "C" fn(*mut c_void) -> usize;
    type ListGet = unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
    type Identifier = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type SetEnabled = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int);
    type ListUnref = unsafe extern "C" fn(*mut c_void);

    unsafe fn symbol(name: &CStr) -> *mut c_void {
        libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr())
    }

    /// Turns off WebKit features by name through the feature API WebKitGTK
    /// 2.42 added, which the bindings predate. Older WebKitGTK lacks it, and
    /// the card bootstrap then keeps cards off.
    unsafe fn disable_features(settings: *mut c_void) {
        let all = symbol(c"webkit_settings_get_all_features");
        let length = symbol(c"webkit_feature_list_get_length");
        let get = symbol(c"webkit_feature_list_get");
        let identifier = symbol(c"webkit_feature_get_identifier");
        let set_enabled = symbol(c"webkit_settings_set_feature_enabled");
        let unref = symbol(c"webkit_feature_list_unref");
        if [all, length, get, identifier, set_enabled, unref]
            .iter()
            .any(|f| f.is_null())
        {
            log::info!("canvas: this WebKitGTK cannot switch features; HTML cards stay off");
            return;
        }

        let all: AllFeatures = std::mem::transmute(all);
        let length: ListLength = std::mem::transmute(length);
        let get: ListGet = std::mem::transmute(get);
        let identifier: Identifier = std::mem::transmute(identifier);
        let set_enabled: SetEnabled = std::mem::transmute(set_enabled);
        let unref: ListUnref = std::mem::transmute(unref);
        let list = all();
        if list.is_null() {
            return;
        }

        let mut switched = Vec::new();
        let mut related = Vec::new();
        for at in 0..length(list) {
            let feature = get(list, at);
            if feature.is_null() {
                continue;
            }
            let name = identifier(feature);
            if name.is_null() {
                continue;
            }
            let name = CStr::from_ptr(name).to_string_lossy().into_owned();
            if NETWORK_FEATURES.contains(&name.as_str()) {
                set_enabled(settings, feature, 0);
                switched.push(name);
            } else if ["Preconnect", "Prefetch", "PeerConnection", "WebRTC"]
                .iter()
                .any(|word| name.contains(word))
            {
                related.push(name);
            }
        }

        unref(list);
        let missing: Vec<&str> = NETWORK_FEATURES
            .iter()
            .copied()
            .filter(|feature| !switched.iter().any(|name| name == feature))
            .collect();
        if !missing.is_empty() {
            log::info!(
                "canvas: switched off {switched:?}; this WebKitGTK has no {missing:?} (related: {related:?})"
            );
        }
    }

    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, label: String) {
        let _ = webview.with_webview(move |w| {
            let wv = w.inner();
            if let Some(settings) = wv.settings() {
                settings.set_enable_webrtc(false);
                settings.set_enable_dns_prefetching(false);
                // SAFETY: a live WebKitSettings, for the duration of the calls.
                unsafe { disable_features(settings.as_ptr() as *mut c_void) };
            }
            let Some(ucm) = wv.user_content_manager() else {
                log::warn!("canvas: webview has no user content manager");
                return;
            };
            if !ucm.register_script_message_handler(HANDLER) {
                log::warn!("canvas: failed to register {HANDLER}");
                return;
            }

            let page = wv.downgrade();
            ucm.connect_script_message_received(Some(HANDLER), move |_ucm, result| {
                let on_canvas = page
                    .upgrade()
                    .and_then(|wv| wv.uri())
                    .is_some_and(|uri| uri.starts_with(&format!("{}:", super::SCHEME)));
                if !on_canvas {
                    return;
                }

                let Some(value) = result.js_value().filter(|v| v.is_string()) else {
                    return;
                };
                if let Some(text) = unsealed(&label, value.to_str().as_str()) {
                    super::receive(label.clone(), text);
                }
            });
        });
    }
}

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

        /// A wrong message type aborts at runtime only.
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

    fn directives(csp: &str) -> Vec<&str> {
        csp.split(';').map(str::trim).collect()
    }

    #[test]
    fn the_page_may_frame_only_the_card_host() {
        let host = format!("frame-src {SCHEME}://localhost{FRAME_PATH}");
        assert!(directives(PAGE_CSP).contains(&host.as_str()));
        assert!(directives(&dev_csp("http://localhost:1420")).contains(&host.as_str()));
    }

    #[test]
    fn cards_run_scripts_but_reach_nothing() {
        let page = frame_page();
        let csp = page.headers()["Content-Security-Policy"].to_str().unwrap();
        for wanted in [
            "default-src 'none'",
            "connect-src 'none'",
            "form-action 'none'",
            "base-uri 'none'",
            "sandbox allow-scripts",
        ] {
            assert!(directives(csp).contains(&wanted), "{wanted} missing");
        }
        assert!(!csp.contains("frame-src") && !csp.contains("http"));
        let body = String::from_utf8(page.body().clone()).unwrap();
        assert!(body.contains("e.source !== parent"));
        assert!(body.contains("typeof RTCPeerConnection !== 'undefined'"));
    }

    #[test]
    fn a_ground_stays_within_reason() {
        assert_eq!(
            Ground::new(12.4, 12.0),
            Ground {
                bottom: 12.0,
                radius: 12.0
            }
        );
        assert_eq!(
            Ground::new(500.0, -3.0),
            Ground {
                bottom: MAX_GROUND,
                radius: 0.0
            }
        );
        assert_eq!(Ground::new(f64::NAN, f64::INFINITY), Ground::default());
    }

    #[test]
    fn a_message_is_answerable_from_its_opening() {
        assert_eq!(head_id(r#"{"id":42,"op":"save""#), Some(42));
        assert_eq!(head_id(r#"{"id":7"#), Some(7));
        assert_eq!(head_id(r#"{"op":"x","id":1}"#), None);
        assert_eq!(head_id("garbage"), None);
    }

    #[test]
    fn a_canvas_tab_looks_like_its_canvas_theme_not_the_browser() {
        use wis_core::canvas::CanvasTheme;
        assert_eq!(appearance(CanvasTheme::Dark), wis_core::ThemeMode::Dark);
        assert_eq!(appearance(CanvasTheme::Light), wis_core::ThemeMode::Light);
        let label = "tab-appearance-test";
        assert!(!is_canvas_tab(label));
        register(
            label,
            CanvasTab {
                workspace: wis_core::WorkspaceId::new(),
                ephemeral: false,
            },
        );
        assert!(is_canvas_tab(label));
        TABS.lock().unwrap().remove(label);
    }

    #[test]
    fn only_messages_sealed_with_the_tab_token_open() {
        assert_eq!(unseal("abc", "abc\n{\"id\":1}"), Some("{\"id\":1}"));
        assert_eq!(unseal("abc", "abd\n{}"), None, "another token");
        assert_eq!(unseal("abc", "{\"id\":1}"), None, "no token at all");
        assert_eq!(unseal("", "\n{}"), None, "an empty token never matches");
        assert_eq!(
            unseal("abc", "abc\nline\nmore"),
            Some("line\nmore"),
            "only the first line is the seal"
        );
    }

    #[test]
    fn the_page_script_sets_the_theme_and_the_side_away_from_the_sidebar() {
        use wis_core::canvas::CanvasTheme;
        use wis_core::{LayoutMode, RailSide, UserSettings};
        let mut settings = UserSettings {
            canvas_theme: CanvasTheme::Light,
            layout: LayoutMode::SidebarLeft,
            ..Default::default()
        };
        assert_eq!(
            page_script(&settings),
            r#"window.__wisCanvasTheme="light";window.__wisCanvasSide="right";"#
        );
        settings.canvas_theme = CanvasTheme::Dark;
        settings.layout = LayoutMode::SidebarRight;
        assert_eq!(
            page_script(&settings),
            r#"window.__wisCanvasTheme="dark";window.__wisCanvasSide="left";"#
        );
        settings.layout = LayoutMode::TopTabs;
        settings.top_tabs_rail_side = RailSide::Left;
        assert_eq!(panel_side(&settings), "right");
        settings.top_tabs_rail_side = RailSide::Right;
        assert_eq!(panel_side(&settings), "left");
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

        let start =
            json!({ "id": 5, "op": "pulse", "canvas": canvas, "state": "start" }).to_string();
        let first: Value = serde_json::from_str(&handle(&storage, &label, &start, 9)).unwrap();
        assert_eq!(first["value"]["safe"], false);
        let again: Value = serde_json::from_str(&handle(&storage, &label, &start, 10)).unwrap();
        assert_eq!(again["value"]["safe"], true, "restarted without a stop");

        let garbled: Value = serde_json::from_str(&handle(&storage, &label, "{nope", 8)).unwrap();
        assert_eq!(
            (garbled["id"].clone(), garbled["ok"].clone()),
            (Value::Null, Value::Bool(false))
        );
        TABS.lock().unwrap().remove(&label);
    }
}
