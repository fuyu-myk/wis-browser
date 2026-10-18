//! The objects WebKit asks about a workspace's tabs and window.

use objc2_06::rc::{Retained, Weak};
use objc2_06::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2_06::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_web_kit::{
    WKWebExtensionContext, WKWebExtensionController, WKWebExtensionControllerDelegate,
    WKWebExtensionTab, WKWebExtensionWindow, WKWebView,
};
use wis_core::WorkspaceId;

use super::host::{selected_tab, tabs_of, window_of};

pub(super) struct TabIvars {
    workspace: WorkspaceId,
    label: String,
    webview: Weak<WKWebView>,
}

pub(super) struct WorkspaceIvars {
    workspace: WorkspaceId,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WisExtensionTab"]
    #[ivars = TabIvars]
    pub(super) struct HostTab;

    unsafe impl NSObjectProtocol for HostTab {}

    unsafe impl WKWebExtensionTab for HostTab {
        #[unsafe(method_id(webViewForWebExtensionContext:))]
        fn web_view(&self, _context: &WKWebExtensionContext) -> Option<Retained<WKWebView>> {
            self.ivars().webview.load()
        }

        #[unsafe(method_id(windowForWebExtensionContext:))]
        fn window(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            window_of(self.ivars().workspace)
        }

        #[unsafe(method(isSelectedForWebExtensionContext:))]
        fn is_selected(&self, _context: &WKWebExtensionContext) -> bool {
            selected_tab(self.ivars().workspace).is_some_and(|tab| tab.label() == self.label())
        }

        #[unsafe(method_id(urlForWebExtensionContext:))]
        fn url(&self, _context: &WKWebExtensionContext) -> Option<Retained<NSURL>> {
            // SAFETY: a property read on the main thread.
            self.ivars().webview.load().and_then(|w| unsafe { w.URL() })
        }

        #[unsafe(method_id(titleForWebExtensionContext:))]
        fn title(&self, _context: &WKWebExtensionContext) -> Option<Retained<NSString>> {
            // SAFETY: a property read on the main thread.
            self.ivars()
                .webview
                .load()
                .and_then(|w| unsafe { w.title() })
        }
    }
);

impl HostTab {
    pub(super) fn label(&self) -> &str {
        &self.ivars().label
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WisExtensionWindow"]
    #[ivars = WorkspaceIvars]
    pub(super) struct HostWindow;

    unsafe impl NSObjectProtocol for HostWindow {}

    unsafe impl WKWebExtensionWindow for HostWindow {
        #[unsafe(method_id(tabsForWebExtensionContext:))]
        fn tabs(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionTab>>> {
            let tabs: Vec<_> = tabs_of(self.ivars().workspace)
                .into_iter()
                .map(ProtocolObject::from_retained)
                .collect();
            NSArray::from_retained_slice(&tabs)
        }

        #[unsafe(method_id(activeTabForWebExtensionContext:))]
        fn active_tab(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionTab>>> {
            selected_tab(self.ivars().workspace).map(ProtocolObject::from_retained)
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WisExtensionControllerDelegate"]
    #[ivars = WorkspaceIvars]
    pub(super) struct HostDelegate;

    unsafe impl NSObjectProtocol for HostDelegate {}

    unsafe impl WKWebExtensionControllerDelegate for HostDelegate {
        #[unsafe(method_id(webExtensionController:openWindowsForExtensionContext:))]
        fn open_windows(
            &self,
            _controller: &WKWebExtensionController,
            _context: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionWindow>>> {
            let windows: Vec<_> = window_of(self.ivars().workspace).into_iter().collect();
            NSArray::from_retained_slice(&windows)
        }

        #[unsafe(method_id(webExtensionController:focusedWindowForExtensionContext:))]
        fn focused_window(
            &self,
            _controller: &WKWebExtensionController,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            window_of(self.ivars().workspace)
        }
    }
);

pub(super) fn new_tab(
    workspace: WorkspaceId,
    label: String,
    webview: &Retained<WKWebView>,
    mtm: MainThreadMarker,
) -> Retained<HostTab> {
    let this = HostTab::alloc(mtm).set_ivars(TabIvars {
        workspace,
        label,
        webview: Weak::from_retained(webview),
    });
    // SAFETY: NSObject's init on a freshly allocated instance.
    unsafe { msg_send![super(this), init] }
}

pub(super) fn new_window(workspace: WorkspaceId, mtm: MainThreadMarker) -> Retained<HostWindow> {
    let this = HostWindow::alloc(mtm).set_ivars(WorkspaceIvars { workspace });
    // SAFETY: NSObject's init on a freshly allocated instance.
    unsafe { msg_send![super(this), init] }
}

pub(super) fn new_delegate(
    workspace: WorkspaceId,
    mtm: MainThreadMarker,
) -> Retained<HostDelegate> {
    let this = HostDelegate::alloc(mtm).set_ivars(WorkspaceIvars { workspace });
    // SAFETY: NSObject's init on a freshly allocated instance.
    unsafe { msg_send![super(this), init] }
}

#[cfg(test)]
mod tests {
    use objc2_06::ClassType;

    /// Registering a class checks each method against the protocol it
    /// claims, without making any WebKit object.
    #[test]
    fn the_adapters_register_against_their_protocols() {
        for class in [
            super::HostTab::class(),
            super::HostWindow::class(),
            super::HostDelegate::class(),
        ] {
            assert!(class.name().to_str().unwrap().starts_with("WisExtension"));
        }
    }
}
