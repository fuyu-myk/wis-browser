//! One in-memory WebKit data store per private workspace (macOS and Linux).
//!
//! Private tabs and widget panels build from a configuration carrying their
//! workspace's store: created on first use, released when the workspace's
//! window closes or the workspace is deleted. WebKit then drops the data
//! with the last webview still using it.
//!
//! On Linux they share one ephemeral context named by the workspace's
//! ephemeral data directory (wry's nineteenth patch). Tauri keeps that
//! context for the app's lifetime, so releasing clears its data instead.

#[cfg(any(target_os = "macos", test))]
use std::collections::HashMap;

use wis_core::WorkspaceId;

#[cfg(target_os = "macos")]
pub use imp::{release, store_for, with_workspace_store};

#[cfg(target_os = "linux")]
pub use gtk_store::{release, remember};

/// On Windows private webviews keep WebView2's InPrivate handling.
#[cfg(windows)]
pub fn release(_app: &tauri::AppHandle, _workspace_id: WorkspaceId) {}

/// The per-workspace store table.
#[cfg(any(target_os = "macos", test))]
struct Registry<T> {
    stores: HashMap<WorkspaceId, T>,
}

#[cfg(any(target_os = "macos", test))]
impl<T: Clone> Registry<T> {
    fn new() -> Self {
        Self {
            stores: HashMap::new(),
        }
    }

    fn get_or_create(&mut self, id: WorkspaceId, create: impl FnOnce() -> T) -> T {
        self.stores.entry(id).or_insert_with(create).clone()
    }

    fn release(&mut self, id: WorkspaceId) -> bool {
        self.stores.remove(&id).is_some()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;

    use objc2_06::rc::Retained;
    use objc2_06::MainThreadMarker;
    use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};
    use tauri::webview::WebviewBuilder;
    use tauri::{AppHandle, Wry};
    use wis_core::{WisError, WisResult, WorkspaceId};

    use super::Registry;

    thread_local! {
        // Main thread only, like the WebKit objects it holds.
        static STORES: RefCell<Registry<Retained<WKWebsiteDataStore>>> =
            RefCell::new(Registry::new());
    }

    /// Attach a fresh configuration on the workspace's shared store. The
    /// builder visits the main thread so the WebKit objects are made there and
    /// never dropped elsewhere by this module; wry then installs its own
    /// content controller and handlers on the configuration.
    pub async fn with_workspace_store(
        app: &AppHandle,
        workspace_id: WorkspaceId,
        builder: WebviewBuilder<Wry>,
    ) -> WisResult<WebviewBuilder<Wry>> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        app.run_on_main_thread(move || {
            let mtm = MainThreadMarker::new().expect("run_on_main_thread runs on the main thread");
            let store = store_for(workspace_id, mtm);
            // SAFETY: main thread; a fresh configuration takes a live store.
            let config = unsafe {
                let config = WKWebViewConfiguration::new(mtm);
                config.setWebsiteDataStore(&store);
                config
            };
            let _ = tx.send(builder.with_webview_configuration(config));
        })
        .map_err(|err| WisError::InvalidInput {
            message: format!("run_on_main_thread: {err}"),
        })?;

        rx.await.map_err(|_| WisError::InvalidInput {
            message: "private store builder dropped".to_string(),
        })
    }

    /// The workspace's in-memory store, made on first use.
    pub fn store_for(
        workspace_id: WorkspaceId,
        mtm: MainThreadMarker,
    ) -> Retained<WKWebsiteDataStore> {
        STORES.with_borrow_mut(|stores| {
            // SAFETY: a class factory with no preconditions, on the main thread.
            stores.get_or_create(workspace_id, || unsafe {
                WKWebsiteDataStore::nonPersistentDataStore(mtm)
            })
        })
    }

    /// Forget the workspace's store. Webviews still on it keep it alive; it
    /// and its data go with the last of them.
    pub fn release(app: &AppHandle, workspace_id: WorkspaceId) {
        let forget = move || {
            STORES.with_borrow_mut(|stores| stores.release(workspace_id));
        };
        if MainThreadMarker::new().is_some() {
            forget();
        } else if let Err(err) = app.run_on_main_thread(forget) {
            log::warn!("private store release for {workspace_id}: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    fn counted(made: &Cell<u32>) -> impl FnOnce() -> Rc<u32> + '_ {
        move || {
            made.set(made.get() + 1);
            Rc::new(made.get())
        }
    }

    #[test]
    fn a_workspace_reuses_one_store() {
        let made = Cell::new(0);
        let mut registry = Registry::new();
        let ws = WorkspaceId::new();

        let first = registry.get_or_create(ws, counted(&made));
        let second = registry.get_or_create(ws, counted(&made));

        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(made.get(), 1);
    }

    #[test]
    fn workspaces_never_share_a_store() {
        let made = Cell::new(0);
        let mut registry = Registry::new();

        let a = registry.get_or_create(WorkspaceId::new(), counted(&made));
        let b = registry.get_or_create(WorkspaceId::new(), counted(&made));

        assert!(!Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn release_lets_the_store_go_and_the_next_use_starts_fresh() {
        let made = Cell::new(0);
        let mut registry = Registry::new();
        let ws = WorkspaceId::new();

        let old = registry.get_or_create(ws, counted(&made));
        assert!(registry.release(ws));
        assert_eq!(
            Rc::strong_count(&old),
            1,
            "the registry still holds the store"
        );

        let fresh = registry.get_or_create(ws, counted(&made));
        assert!(!Rc::ptr_eq(&old, &fresh));
    }

    #[test]
    fn releasing_an_unknown_workspace_is_a_no_op() {
        let mut registry: Registry<Rc<u32>> = Registry::new();
        assert!(!registry.release(WorkspaceId::new()));
    }
}

#[cfg(target_os = "linux")]
mod gtk_store {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use webkit2gtk::{
        WebViewExt, WebsiteDataManager, WebsiteDataManagerExtManual, WebsiteDataTypes,
    };

    use super::WorkspaceId;

    thread_local! {
        // Main thread only, like the GObjects it holds.
        static MANAGERS: RefCell<HashMap<WorkspaceId, WebsiteDataManager>> =
            RefCell::default();
    }

    /// Note the data manager a private webview of `workspace_id` shares, so
    /// releasing the workspace can clear it.
    pub fn remember(webview: &tauri::Webview, workspace_id: WorkspaceId) {
        let noted = webview.with_webview(move |platform| {
            let Some(manager) = platform.inner().website_data_manager() else {
                return;
            };
            MANAGERS.with(|managers| {
                managers.borrow_mut().entry(workspace_id).or_insert(manager);
            });
        });
        if let Err(err) = noted {
            log::warn!("private store: noting a private webview's data failed: {err}");
        }
    }

    pub fn release(app: &tauri::AppHandle, workspace_id: WorkspaceId) {
        let posted = app.run_on_main_thread(move || {
            let Some(manager) =
                MANAGERS.with(|managers| managers.borrow_mut().remove(&workspace_id))
            else {
                return;
            };
            clear_all(&manager);
            let _ = webkit2gtk::glib::timeout_add_local_once(SECOND_CLEAR, move || {
                let shown_again =
                    MANAGERS.with(|managers| managers.borrow().contains_key(&workspace_id));
                if !shown_again {
                    clear_all(&manager);
                }
            });
        });
        if let Err(err) = posted {
            log::warn!("private store: clearing a private workspace failed: {err}");
        }
    }

    const SECOND_CLEAR: std::time::Duration = std::time::Duration::from_secs(2);

    fn clear_all(manager: &WebsiteDataManager) {
        manager.clear(
            WebsiteDataTypes::ALL,
            webkit2gtk::glib::TimeSpan(0),
            None::<&webkit2gtk::gio::Cancellable>,
            |cleared| {
                if let Err(err) = cleared {
                    log::warn!("private store: clearing a private workspace failed: {err}");
                }
            },
        );
    }
}
