//! Each regular workspace's WebKit extension controller: its store and proxy,
//! the installed extensions loaded into it, and the tabs and window it knows.
//! A host lives as long as its workspace's window. Main thread only, like the
//! WebKit objects it holds; no borrow of the host table is held across a call
//! into WebKit, which may call the adapters back.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use block2_06::RcBlock;
use objc2_06::rc::Retained;
use objc2_06::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2_06::{MainThreadMarker, MainThreadOnly};
use objc2_foundation::{
    NSArray, NSError, NSObjectNSKeyValueCoding, NSSet, NSString, NSURL, NSUUID,
};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionContextPermissionStatus,
    WKWebExtensionController, WKWebExtensionControllerConfiguration, WKWebExtensionDataRecord,
    WKWebExtensionDataType, WKWebExtensionTabChangedProperties, WKWebExtensionWindow, WKWebView,
    WKWebViewConfiguration, WKWebsiteDataStore,
};
use tokio::sync::oneshot;
use wis_core::WorkspaceId;

use super::adapters::{new_delegate, new_tab, new_window, HostDelegate, HostTab, HostWindow};
use super::TabChange;

/// Public from macOS 15.4; before that the class is missing.
pub(super) fn supported() -> bool {
    AnyClass::get(c"WKWebExtensionController").is_some()
}

#[link(name = "Network", kind = "framework")]
extern "C" {
    fn nw_endpoint_create_host(hostname: *const c_char, port: *const c_char) -> *mut AnyObject;
    fn nw_proxy_config_create_http_connect(
        endpoint: *mut AnyObject,
        tls_options: *mut AnyObject,
    ) -> *mut AnyObject;
}

struct Host {
    generation: u64,
    /// A private workspace's host: in-memory store and controller, and its
    /// extensions may see private browsing.
    private: bool,
    controller: Retained<WKWebExtensionController>,
    window: Retained<HostWindow>,
    _delegate: Retained<HostDelegate>,
    store: Retained<WKWebsiteDataStore>,
    tabs: Vec<Retained<HostTab>>,
    /// The workspace's active tab as the state names it, registered or not.
    wanted_active: Option<String>,
    /// The active tab WebKit is told about, always a registered one.
    active: Option<String>,
    contexts: HashMap<String, Retained<WKWebExtensionContext>>,
    wanted: HashSet<String>,
    loading: HashSet<String>,
    waiters: Vec<oneshot::Sender<bool>>,
}

thread_local! {
    static HOSTS: RefCell<HashMap<WorkspaceId, Host>> = RefCell::new(HashMap::new());
    static GENERATION: Cell<u64> = const { Cell::new(0) };
    /// Each workspace's extensions as the last change left them, newer than
    /// any list a tab read before that change.
    static LATEST: RefCell<Option<HashMap<WorkspaceId, Vec<String>>>> = const { RefCell::new(None) };
}

/// Whether any host is running, readable off the main thread.
pub(super) static LIVE: AtomicBool = AtomicBool::new(false);

fn refresh_live(hosts: &HashMap<WorkspaceId, Host>) {
    LIVE.store(!hosts.is_empty(), Ordering::Relaxed);
}

/// The tab to report as active: the wanted one once registered, else the
/// first registered tab.
fn selected<'a>(active: Option<&'a str>, labels: &[&'a str]) -> Option<&'a str> {
    active
        .filter(|a| labels.contains(a))
        .or_else(|| labels.first().copied())
}

pub(super) fn tabs_of(workspace: WorkspaceId) -> Vec<Retained<HostTab>> {
    HOSTS.with(|hosts| {
        hosts
            .try_borrow()
            .ok()
            .and_then(|h| h.get(&workspace).map(|host| host.tabs.clone()))
            .unwrap_or_default()
    })
}

pub(super) fn window_of(
    workspace: WorkspaceId,
) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
    HOSTS.with(|hosts| {
        let hosts = hosts.try_borrow().ok()?;
        let window = hosts.get(&workspace)?.window.clone();
        Some(ProtocolObject::from_retained(window))
    })
}

pub(super) fn context_of(
    workspace: WorkspaceId,
    id: &str,
) -> Option<Retained<WKWebExtensionContext>> {
    HOSTS.with(|hosts| {
        hosts
            .try_borrow()
            .ok()?
            .get(&workspace)?
            .contexts
            .get(id)
            .cloned()
    })
}

pub(super) fn is_private(workspace: WorkspaceId) -> bool {
    HOSTS.with(|hosts| {
        hosts
            .try_borrow()
            .ok()
            .and_then(|h| h.get(&workspace).map(|host| host.private))
            .unwrap_or(true)
    })
}

/// Every loaded context, with its extension's id.
pub(super) fn contexts() -> Vec<(String, Retained<WKWebExtensionContext>)> {
    HOSTS.with_borrow(|hosts| {
        hosts
            .values()
            .flat_map(|host| host.contexts.iter().map(|(id, c)| (id.clone(), c.clone())))
            .collect()
    })
}

pub(super) fn selected_tab(workspace: WorkspaceId) -> Option<Retained<HostTab>> {
    HOSTS.with(|hosts| {
        let hosts = hosts.try_borrow().ok()?;
        let host = hosts.get(&workspace)?;
        let labels: Vec<&str> = host.tabs.iter().map(|t| t.label()).collect();
        let label = selected(host.active.as_deref(), &labels)?;
        host.tabs.iter().find(|t| t.label() == label).cloned()
    })
}

/// Routes the store through the workspace's proxy, so nothing an extension
/// fetches goes around it.
unsafe fn route_through_proxy(store: &WKWebsiteDataStore, port: u16) -> bool {
    let Ok(port) = CString::new(port.to_string()) else {
        return false;
    };
    let endpoint = nw_endpoint_create_host(c"127.0.0.1".as_ptr(), port.as_ptr());
    let Some(endpoint) = Retained::<AnyObject>::from_raw(endpoint) else {
        return false;
    };
    let proxy = nw_proxy_config_create_http_connect(
        Retained::as_ptr(&endpoint).cast_mut(),
        std::ptr::null_mut(),
    );
    let Some(proxy) = Retained::<AnyObject>::from_raw(proxy) else {
        return false;
    };
    let proxies = NSArray::from_retained_slice(&[proxy]);
    store.setValue_forKey(Some(&proxies), &NSString::from_str("proxyConfigurations"));
    true
}

/// The workspace's store, as wry opens it for `data_store_identifier`.
fn workspace_store(workspace: WorkspaceId, mtm: MainThreadMarker) -> Retained<WKWebsiteDataStore> {
    let id = NSUUID::from_bytes(crate::commands::workspace_id_to_data_store_bytes(workspace));
    // SAFETY: a class factory with no preconditions (macOS 14+), on the main thread.
    unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&id, mtm) }
}

/// A regular workspace's controller configuration, keeping its extensions'
/// storage under an identifier derived from the workspace.
fn persistent_configuration(
    workspace: WorkspaceId,
    mtm: MainThreadMarker,
) -> Retained<WKWebExtensionControllerConfiguration> {
    let identifier =
        crate::commands::workspace_id_to_data_store_bytes(workspace).map(|byte| byte ^ 0xa5);
    // SAFETY: a class factory with no preconditions, on the main thread.
    unsafe {
        WKWebExtensionControllerConfiguration::configurationWithIdentifier(
            &NSUUID::from_bytes(identifier),
            mtm,
        )
    }
}

fn new_host(
    workspace: WorkspaceId,
    private: bool,
    port: u16,
    wanted_active: Option<String>,
    mtm: MainThreadMarker,
) -> Option<Host> {
    let store = if private {
        crate::private_store::store_for(workspace, mtm)
    } else {
        workspace_store(workspace, mtm)
    };
    // SAFETY: main thread; the store is live.
    if !unsafe { route_through_proxy(&store, port) } {
        log::error!("extensions stay off in {workspace}: its proxy could not be set");
        return None;
    }

    // SAFETY: main thread; each object is configured before the controller is built from it.
    let controller = unsafe {
        let config = if private {
            WKWebExtensionControllerConfiguration::nonPersistentConfiguration(mtm)
        } else {
            persistent_configuration(workspace, mtm)
        };
        config.setDefaultWebsiteDataStore(Some(&store));
        let pages = config.webViewConfiguration();
        pages.setWebsiteDataStore(&store);
        config.setWebViewConfiguration(Some(&pages));
        WKWebExtensionController::initWithConfiguration(
            WKWebExtensionController::alloc(mtm),
            &config,
        )
    };
    let delegate = new_delegate(workspace, mtm);
    // SAFETY: the host keeps the delegate alive as long as the controller.
    unsafe { controller.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };

    let generation = GENERATION.get() + 1;
    GENERATION.set(generation);
    Some(Host {
        generation,
        private,
        controller,
        window: new_window(workspace, mtm),
        _delegate: delegate,
        store,
        tabs: Vec::new(),
        wanted_active,
        active: None,
        contexts: HashMap::new(),
        wanted: HashSet::new(),
        loading: HashSet::new(),
        waiters: Vec::new(),
    })
}

/// A configuration for one of the workspace's tabs: its store and controller.
pub(super) fn tab_configuration(
    workspace: WorkspaceId,
    mtm: MainThreadMarker,
) -> Option<Retained<WKWebViewConfiguration>> {
    let (store, controller) = HOSTS.with_borrow(|hosts| {
        let host = hosts.get(&workspace)?;
        Some((host.store.clone(), host.controller.clone()))
    })?;
    // SAFETY: main thread; a fresh configuration takes the live store and controller.
    unsafe {
        let config = WKWebViewConfiguration::new(mtm);
        config.setWebsiteDataStore(&store);
        config.setWebExtensionController(Some(&controller));
        Some(config)
    }
}

/// Makes sure the workspace has a host, then answers `ready` once its
/// extensions are in: true, or false when it could not have one.
pub(super) fn ensure(
    workspace: WorkspaceId,
    private: bool,
    port: u16,
    dir: PathBuf,
    wanted: Vec<String>,
    active: Option<String>,
    ready: oneshot::Sender<bool>,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        let _ = ready.send(false);
        return;
    };
    if HOSTS.with_borrow(|hosts| hosts.contains_key(&workspace)) {
        wait_for_loads(workspace, ready);
        return;
    }

    let Some(host) = new_host(workspace, private, port, active, mtm) else {
        let _ = ready.send(false);
        return;
    };
    let controller = host.controller.clone();
    let window = host.window.clone();
    HOSTS.with_borrow_mut(|hosts| {
        hosts.insert(workspace, host);
        refresh_live(hosts);
    });
    let window = ProtocolObject::from_ref(&*window);
    // SAFETY: main thread; the host keeps the window alive.
    unsafe {
        controller.didOpenWindow(window);
        controller.didFocusWindow(Some(window));
    }

    let wanted = LATEST
        .with_borrow(|latest| latest.as_ref()?.get(&workspace).cloned())
        .unwrap_or(wanted);
    sync_host(workspace, &dir, &wanted, mtm);
    wait_for_loads(workspace, ready);
}

fn wait_for_loads(workspace: WorkspaceId, ready: oneshot::Sender<bool>) {
    let ready = HOSTS.with_borrow_mut(|hosts| match hosts.get_mut(&workspace) {
        Some(host) if !host.loading.is_empty() => {
            host.waiters.push(ready);
            None
        }
        Some(_) => Some((ready, true)),
        None => Some((ready, false)),
    });
    if let Some((ready, answer)) = ready {
        let _ = ready.send(answer);
    }
}

/// Loads what each workspace's list adds and unloads what it drops, in every
/// live host.
pub(super) fn sync_all(
    dir: PathBuf,
    wanted: HashMap<WorkspaceId, Vec<String>>,
    access: HashMap<String, super::grants::Access>,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    super::grants::set(access);
    let workspaces: Vec<WorkspaceId> = HOSTS.with_borrow(|hosts| hosts.keys().copied().collect());
    for workspace in workspaces {
        let list = wanted.get(&workspace).cloned().unwrap_or_default();
        sync_host(workspace, &dir, &list, mtm);
    }
    LATEST.with_borrow_mut(|latest| *latest = Some(wanted));
}

fn sync_host(workspace: WorkspaceId, dir: &Path, wanted: &[String], mtm: MainThreadMarker) {
    let plan = HOSTS.with_borrow_mut(|hosts| {
        let host = hosts.get_mut(&workspace)?;
        host.wanted = wanted.iter().cloned().collect();
        let dropped: Vec<String> = host
            .contexts
            .keys()
            .filter(|id| !host.wanted.contains(*id))
            .cloned()
            .collect();
        let dropped: Vec<_> = dropped
            .iter()
            .filter_map(|id| host.contexts.remove(id))
            .collect();
        let added: Vec<String> = wanted
            .iter()
            .filter(|id| !host.contexts.contains_key(*id) && !host.loading.contains(*id))
            .cloned()
            .collect();
        host.loading.extend(added.iter().cloned());
        Some((host.controller.clone(), host.generation, dropped, added))
    });
    let Some((controller, generation, dropped, added)) = plan else {
        return;
    };

    for context in dropped {
        // SAFETY: main thread; the context was loaded into this controller.
        if let Err(err) = unsafe { controller.unloadExtensionContext_error(&context) } {
            log::warn!(
                "unloading an extension failed: {}",
                err.localizedDescription()
            );
        }
    }
    for id in added {
        load_extension(&dir.join(&id), mtm, move |extension| {
            finish_load(workspace, generation, id, extension);
        });
    }
}

fn finish_load(
    workspace: WorkspaceId,
    generation: u64,
    id: String,
    extension: Result<Retained<WKWebExtension>, String>,
) {
    let controller = HOSTS.with_borrow(|hosts| {
        let host = hosts
            .get(&workspace)
            .filter(|h| h.generation == generation)?;
        host.wanted
            .contains(&id)
            .then(|| (host.controller.clone(), host.private))
    });
    let context = match (controller, extension) {
        (Some((controller, private)), Ok(extension)) => {
            load_context(&controller, &extension, &id, private)
        }
        (Some(_), Err(message)) => Err(message),
        (None, _) => Err("no longer wanted".into()),
    };

    let waiters = HOSTS.with_borrow_mut(|hosts| {
        let host = hosts
            .get_mut(&workspace)
            .filter(|h| h.generation == generation)?;
        host.loading.remove(&id);
        match context {
            Ok(context) => {
                host.contexts.insert(id, context);
            }
            Err(message) => log::warn!("extension {id} did not load: {message}"),
        }
        host.loading
            .is_empty()
            .then(|| std::mem::take(&mut host.waiters))
    });
    for waiter in waiters.into_iter().flatten() {
        let _ = waiter.send(true);
    }
}

fn load_context(
    controller: &WKWebExtensionController,
    extension: &WKWebExtension,
    id: &str,
    private: bool,
) -> Result<Retained<WKWebExtensionContext>, String> {
    let granted = WKWebExtensionContextPermissionStatus::GrantedExplicitly;
    // SAFETY: main thread; the context is configured before it is loaded.
    unsafe {
        let context = WKWebExtensionContext::contextForExtension(extension);
        context.setUniqueIdentifier(&NSString::from_str(id));
        context.setHasAccessToPrivateData(private);
        for permission in extension.requestedPermissions().iter() {
            context.setPermissionStatus_forPermission(granted, &permission);
        }
        super::grants::apply(&context, id);
        controller
            .loadExtensionContext_error(&context)
            .map_err(|err| err.localizedDescription().to_string())?;
        Ok(context)
    }
}

/// Reads the extension at `dir`; `done` gets it, or why it could not be read.
pub(super) fn load_extension(
    dir: &Path,
    mtm: MainThreadMarker,
    done: impl FnOnce(Result<Retained<WKWebExtension>, String>) + 'static,
) {
    let done = RefCell::new(Some(done));
    let block = RcBlock::new(move |extension: *mut WKWebExtension, error: *mut NSError| {
        let Some(done) = done.borrow_mut().take() else {
            return;
        };
        // SAFETY: WebKit hands over a live extension or error, or null.
        let extension = unsafe { Retained::retain(extension) };
        let error = unsafe { Retained::retain(error) };
        done(extension.ok_or_else(|| {
            error.map_or_else(
                || "the extension could not be read".to_string(),
                |e| e.localizedDescription().to_string(),
            )
        }));
    });
    let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&dir.to_string_lossy()), true);
    // SAFETY: main thread; `url` is an absolute file URL to a folder.
    unsafe { WKWebExtension::extensionWithResourceBaseURL_completionHandler(&url, &block, mtm) };
}

/// What WebKit made of an extension, for its install review.
pub(super) struct Facts {
    pub name: String,
    pub version: String,
    pub description: String,
    pub recognised: Vec<String>,
    pub patterns: Vec<String>,
}

pub(super) fn facts_of(extension: &WKWebExtension) -> Facts {
    let text = |s: Option<Retained<NSString>>| s.map(|s| s.to_string()).unwrap_or_default();
    // SAFETY: property reads on the main thread.
    unsafe {
        Facts {
            name: text(extension.displayName()),
            version: text(extension.version()),
            description: text(extension.displayDescription()),
            recognised: extension
                .requestedPermissions()
                .iter()
                .map(|p| p.to_string())
                .collect(),
            patterns: extension
                .allRequestedMatchPatterns()
                .iter()
                .map(|p| p.string().to_string())
                .collect(),
        }
    }
}

/// Tells the workspace's controller about a new tab's webview.
pub(super) fn open_tab(workspace: WorkspaceId, label: String, webview: *mut c_void) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: wry's live WKWebView for this tab, on the main thread.
    let Some(webview) = (unsafe { Retained::retain(webview.cast::<WKWebView>()) }) else {
        return;
    };
    let tab = new_tab(workspace, label.clone(), &webview, mtm);

    let known = HOSTS.with_borrow_mut(|hosts| {
        let host = hosts.get_mut(&workspace)?;
        host.tabs.push(tab.clone());
        let previous = if host.wanted_active.as_deref() == Some(label.as_str()) {
            let previous = host.active.replace(label.clone());
            Some(previous.and_then(|p| host.tabs.iter().find(|t| t.label() == p).cloned()))
        } else {
            None
        };
        Some((host.controller.clone(), previous))
    });
    let Some((controller, activated)) = known else {
        return;
    };
    let tab = ProtocolObject::from_ref(&*tab);
    // SAFETY: main thread; the host keeps both tabs alive.
    unsafe {
        controller.didOpenTab(tab);
        if let Some(previous) = activated {
            controller.didActivateTab_previousActiveTab(
                tab,
                previous.as_deref().map(ProtocolObject::from_ref),
            );
        }
    }
}

/// Tells the controller a tab's webview is gone.
pub(super) fn close_tab(label: &str) {
    let found = HOSTS.with_borrow_mut(|hosts| {
        let host = hosts
            .values_mut()
            .find(|host| host.tabs.iter().any(|t| t.label() == label))?;
        let at = host.tabs.iter().position(|t| t.label() == label)?;
        let tab = host.tabs.remove(at);
        if host.active.as_deref() == Some(label) {
            host.active = None;
        }
        Some((host.controller.clone(), tab))
    });
    let Some((controller, tab)) = found else {
        return;
    };
    // SAFETY: main thread; `tab` is alive for the call.
    unsafe { controller.didCloseTab_windowIsClosing(ProtocolObject::from_ref(&*tab), false) };
}

/// Ends the workspace's host with its window: its extensions stop running.
pub(super) fn release(workspace: WorkspaceId) {
    super::prompts::forget(workspace);
    let host = HOSTS.with_borrow_mut(|hosts| {
        let host = hosts.remove(&workspace);
        refresh_live(hosts);
        host
    });
    if let Some(host) = host {
        // SAFETY: main thread; the window is alive for the call.
        unsafe {
            host.controller
                .didCloseWindow(ProtocolObject::from_ref(&*host.window))
        };
    }
}

/// Tells the controller what changed about a tab, for `tabs.onUpdated`.
pub(super) fn tab_changed(label: &str, change: TabChange) {
    let properties = match change {
        TabChange::Title => WKWebExtensionTabChangedProperties::Title,
        TabChange::Started => {
            WKWebExtensionTabChangedProperties::Loading | WKWebExtensionTabChangedProperties::URL
        }
        TabChange::Finished => WKWebExtensionTabChangedProperties::Loading,
    };
    let found = HOSTS.with_borrow(|hosts| {
        hosts.values().find_map(|host| {
            let tab = host.tabs.iter().find(|t| t.label() == label)?;
            Some((host.controller.clone(), tab.clone()))
        })
    });
    let Some((controller, tab)) = found else {
        return;
    };
    // SAFETY: main thread; the tab is alive for the call.
    unsafe {
        controller.didChangeTabProperties_forTab(properties, ProtocolObject::from_ref(&*tab))
    };
}

/// Whether a stored data record belongs to an extension no longer kept.
fn is_stale(id: &str, size: usize, keep: &HashSet<String>) -> bool {
    size > 0 && !keep.contains(id)
}

/// Clears the stored data of every extension not in `keep` from each listed
/// regular workspace's extension storage, whether its window is open or not.
pub(super) fn clear_data(workspaces: Vec<WorkspaceId>, keep: HashSet<String>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: a class property, on the main thread.
    let types = unsafe { WKWebExtensionController::allExtensionDataTypes(mtm) };
    for workspace in workspaces {
        let live = HOSTS.with_borrow(|hosts| {
            hosts
                .get(&workspace)
                .filter(|host| !host.private)
                .map(|host| host.controller.clone())
        });
        let controller = live.unwrap_or_else(|| {
            // SAFETY: main thread; a controller on the workspace's storage, alive until the clearing ends.
            unsafe {
                WKWebExtensionController::initWithConfiguration(
                    WKWebExtensionController::alloc(mtm),
                    &persistent_configuration(workspace, mtm),
                )
            }
        });
        clear_records(controller, types.clone(), keep.clone());
    }
}

fn clear_records(
    controller: Retained<WKWebExtensionController>,
    types: Retained<NSSet<WKWebExtensionDataType>>,
    keep: HashSet<String>,
) {
    let held = controller.clone();
    let removing = types.clone();
    let fetched = RcBlock::new(move |records: NonNull<NSArray<WKWebExtensionDataRecord>>| {
        // SAFETY: WebKit hands over a live array for the length of the call.
        let records = unsafe { records.as_ref() };
        let stale: Vec<Retained<WKWebExtensionDataRecord>> = records
            .iter()
            // SAFETY: property reads on the main thread.
            .filter(|r| unsafe {
                is_stale(
                    &r.uniqueIdentifier().to_string(),
                    r.totalSizeInBytes(),
                    &keep,
                )
            })
            .collect();
        if stale.is_empty() {
            return;
        }

        let stale = NSArray::from_retained_slice(&stale);
        let until_done = held.clone();
        let done = RcBlock::new(move || {
            let _ = &until_done;
        });
        // SAFETY: main thread; the records came from this controller.
        unsafe {
            held.removeDataOfTypes_fromDataRecords_completionHandler(&removing, &stale, &done)
        };
    });
    // SAFETY: main thread; the block keeps the controller alive until it runs.
    unsafe { controller.fetchDataRecordsOfTypes_completionHandler(&types, &fetched) };
}

/// Records each workspace's active tab and tells its controller once that
/// tab is registered.
pub(super) fn set_active(actives: Vec<(WorkspaceId, Option<String>)>) {
    for (workspace, label) in actives {
        let change = HOSTS.with_borrow_mut(|hosts| {
            let host = hosts.get_mut(&workspace)?;
            host.wanted_active.clone_from(&label);
            let label = label?;
            let tab = host.tabs.iter().find(|t| t.label() == label).cloned()?;
            if host.active.as_deref() == Some(label.as_str()) {
                return None;
            }
            let previous = host
                .active
                .replace(label)
                .and_then(|p| host.tabs.iter().find(|t| t.label() == p).cloned());
            Some((host.controller.clone(), tab, previous))
        });
        let Some((controller, tab, previous)) = change else {
            continue;
        };
        // SAFETY: main thread; both tabs are alive for the call.
        unsafe {
            controller.didActivateTab_previousActiveTab(
                ProtocolObject::from_ref(&*tab),
                previous.as_deref().map(ProtocolObject::from_ref),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2::runtime::AnyObject;
    use objc2::{class, sel};

    use std::collections::HashSet;

    use super::{is_stale, selected};

    type Obj = *mut AnyObject;

    #[test]
    fn data_is_cleared_only_for_extensions_no_longer_kept() {
        let keep: HashSet<String> = ["kept".to_string()].into();
        assert!(is_stale("removed", 7, &keep));
        assert!(!is_stale("kept", 7, &keep));
        assert!(!is_stale("removed", 0, &keep));
    }

    #[test]
    fn the_wanted_tab_is_active_once_registered() {
        assert_eq!(selected(Some("tab-b"), &["tab-a", "tab-b"]), Some("tab-b"));
    }

    #[test]
    fn an_unregistered_active_tab_falls_back_to_the_first() {
        assert_eq!(selected(Some("tab-c"), &["tab-a", "tab-b"]), Some("tab-a"));
        assert_eq!(selected(None, &["tab-a"]), Some("tab-a"));
        assert_eq!(selected(Some("tab-a"), &[]), None);
    }

    /// The proxy is set through key-value coding, which the typed bindings
    /// do not cover. A wrong message type aborts at runtime only.
    #[test]
    fn the_proxy_send_matches_its_method() {
        class!(WKWebsiteDataStore)
            .verify_sel::<(Obj, Obj), ()>(sel!(setValue:forKey:))
            .unwrap();
    }
}
