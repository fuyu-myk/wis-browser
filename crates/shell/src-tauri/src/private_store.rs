//! One in-memory WebKit data store per private workspace (macOS).
//!
//! Private tabs and widget panels build from a configuration carrying their
//! workspace's store: created on first use, released when the workspace's
//! window closes or the workspace is deleted. WebKit then drops the data
//! with the last webview still using it.

#[cfg(any(target_os = "macos", test))]
use std::collections::HashMap;

use wis_core::WorkspaceId;

#[cfg(target_os = "macos")]
pub use imp::{release, with_workspace_store};

/// Elsewhere private webviews keep the engine's incognito handling.
#[cfg(not(target_os = "macos"))]
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
            let store = STORES.with_borrow_mut(|stores| {
                // SAFETY: a class factory with no preconditions, on the main thread.
                stores.get_or_create(workspace_id, || unsafe {
                    WKWebsiteDataStore::nonPersistentDataStore(mtm)
                })
            });
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
