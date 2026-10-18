//! Extension actions: what the toolbar lists, running one with its popup
//! anchored to the button that ran it, and options pages in a window of
//! their own. Main thread only.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use base64::Engine;
use block2_06::RcBlock;
use objc2_06::rc::Retained;
use objc2_06::runtime::{NSObjectProtocol, ProtocolObject};
use objc2_06::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSBackingStoreType,
    NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSPopover, NSPopoverDidCloseNotification,
    NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSDictionary, NSNotification, NSNotificationCenter, NSPoint, NSRect, NSRectEdge, NSSize,
    NSString, NSURLRequest,
};
use objc2_web_kit::{WKWebExtensionAction, WKWebExtensionContext, WKWebView};
use wis_core::WorkspaceId;

use super::{host, ExtensionAction};

/// The button rectangle, in window points from the top left.
pub(crate) type Rect = (f64, f64, f64, f64);

/// Where the next popup anchors: a window's content view and the button,
/// and whether Wis is dark (`None` follows the system).
struct Anchor {
    view: Retained<NSView>,
    rect: Rect,
    dark: Option<bool>,
}

/// The popup on screen, and its close observer.
struct Shown {
    popover: Retained<NSPopover>,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

const ICON_POINTS: f64 = 32.0;
const OPTIONS_SIZE: (f64, f64) = (820.0, 620.0);

/// A popup that closed this recently was closed by the click now asking to
/// toggle it: AppKit closes it on mouse-down, before the click arrives.
const CLOSING_CLICK: Duration = Duration::from_millis(600);

thread_local! {
    static ANCHOR: RefCell<Option<Anchor>> = const { RefCell::new(None) };
    static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
    static CLOSED_AT: Cell<Option<Instant>> = const { Cell::new(None) };
    static OPTIONS: RefCell<HashMap<String, Retained<NSWindow>>> = RefCell::new(HashMap::new());
}

fn png_data_url(image: &NSImage) -> Option<String> {
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    // SAFETY: an empty property dictionary is valid for PNG.
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.to_vec())
    ))
}

/// The actions of `ids` running in `workspace`, for its active tab.
pub(super) fn list(workspace: WorkspaceId, ids: &[String]) -> Vec<ExtensionAction> {
    let tab = host::selected_tab(workspace);
    let tab = tab.as_deref().map(ProtocolObject::from_ref);
    let size = NSSize::new(ICON_POINTS, ICON_POINTS);
    ids.iter()
        .filter_map(|id| {
            let context = host::context_of(workspace, id)?;
            // SAFETY: property reads on the main thread.
            unsafe {
                let extension = context.webExtension();
                let action = context.actionForTab(tab);
                let icon = action
                    .as_ref()
                    .and_then(|a| a.iconForSize(size))
                    .or_else(|| extension.iconForSize(size));
                Some(ExtensionAction {
                    id: id.clone(),
                    name: extension
                        .displayName()
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                    label: action
                        .as_ref()
                        .map(|a| a.label().to_string())
                        .unwrap_or_default(),
                    badge: action
                        .as_ref()
                        .map(|a| a.badgeText().to_string())
                        .unwrap_or_default(),
                    enabled: action.as_ref().is_some_and(|a| a.isEnabled()),
                    has_options: extension.hasOptionsPage(),
                    icon: icon.as_deref().and_then(png_data_url),
                })
            }
        })
        .collect()
}

/// Runs an extension's action on the active tab; a popup it opens anchors to
/// `rect` in the window whose content view is `view`.
pub(super) fn run(
    workspace: WorkspaceId,
    id: &str,
    view: *mut c_void,
    rect: Rect,
    dark: Option<bool>,
) -> bool {
    let Some(context) = host::context_of(workspace, id) else {
        return false;
    };
    // SAFETY: tao's live content view of the calling window, on the main thread.
    let Some(view) = (unsafe { Retained::retain(view.cast::<NSView>()) }) else {
        return false;
    };
    ANCHOR.set(Some(Anchor { view, rect, dark }));
    let tab = host::selected_tab(workspace);
    // SAFETY: main thread; the tab is alive for the call.
    unsafe { context.performActionForTab(tab.as_deref().map(ProtocolObject::from_ref)) };
    true
}

/// Shows the action's popup at the last anchor.
pub(super) fn present_popup(action: &WKWebExtensionAction) -> Result<(), String> {
    let anchor = ANCHOR.take().ok_or("no button to anchor the popup to")?;
    // SAFETY: a property read on the main thread.
    let popover = unsafe { action.popupPopover() }.ok_or("the extension has no popup")?;
    let (x, y, width, height) = anchor.rect;
    let (y, edge) = if anchor.view.isFlipped() {
        (y, NSRectEdge::MaxY)
    } else {
        (
            anchor.view.bounds().size.height - y - height,
            NSRectEdge::MinY,
        )
    };
    let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));
    let appearance = anchor.dark.and_then(|dark| {
        // SAFETY: AppKit's appearance name constants.
        NSAppearance::appearanceNamed(unsafe {
            if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        })
    });
    popover.setAppearance(appearance.as_deref());
    dismiss_popup();
    popover.showRelativeToRect_ofView_preferredEdge(rect, &anchor.view, edge);

    let observer = observe_close(&popover);
    SHOWN.set(Some(Shown { popover, observer }));
    Ok(())
}

fn forget(shown: &Shown) {
    // SAFETY: main thread; the observer was registered with the default center.
    unsafe { NSNotificationCenter::defaultCenter().removeObserver((*shown.observer).as_ref()) };
}

fn observe_close(popover: &NSPopover) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    let closed = RcBlock::new(|_: NonNull<NSNotification>| {
        let shown = SHOWN.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.take()));
        if let Some(shown) = shown {
            CLOSED_AT.set(Some(Instant::now()));
            forget(&shown);
        }
    });
    // SAFETY: main thread; the observer is removed when the popover closes or
    // another popup replaces it.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSPopoverDidCloseNotification),
            Some(popover),
            None,
            &closed,
        )
    }
}

/// Closes the popup on screen; true when there was one, or when one closed
/// a moment ago because of the click asking.
pub(super) fn dismiss_popup() -> bool {
    let shown = SHOWN.with(|s| s.borrow_mut().take());
    if let Some(shown) = shown {
        forget(&shown);
        if shown.popover.isShown() {
            // SAFETY: main thread; closing a popover on screen.
            unsafe { shown.popover.performClose(None) };
        }
        CLOSED_AT.set(None);
        return true;
    }
    CLOSED_AT
        .take()
        .is_some_and(|at| at.elapsed() < CLOSING_CLICK)
}

/// Opens an extension's options page in a window of its own, built on the
/// extension's configuration, or brings an open one forward.
pub(super) fn open_options(context: &WKWebExtensionContext) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    // SAFETY: property reads on the main thread.
    let (id, name, url, config) = unsafe {
        (
            context.uniqueIdentifier().to_string(),
            context
                .webExtension()
                .displayName()
                .map(|n| n.to_string())
                .unwrap_or_default(),
            context
                .optionsPageURL()
                .ok_or("this extension has no options page")?,
            context
                .webViewConfiguration()
                .ok_or("this extension is not running")?,
        )
    };
    let open = OPTIONS.with_borrow(|windows| windows.get(&id).cloned());
    if let Some(window) = open.filter(|w| w.isVisible()) {
        window.makeKeyAndOrderFront(None);
        return Ok(());
    }

    let frame = NSRect::new(NSPoint::ZERO, NSSize::new(OPTIONS_SIZE.0, OPTIONS_SIZE.1));
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    // SAFETY: main thread; the window owns the web view, and the table keeps
    // the window alive after it closes so it is never freed from under AppKit.
    let window = unsafe {
        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        window.setReleasedWhenClosed(false);
        window.setTitle(&NSString::from_str(&format!("{name} options")));
        let webview = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config);
        window.setContentView(Some(&webview));
        webview.loadRequest(&NSURLRequest::requestWithURL(&url));
        window
    };
    window.center();
    window.makeKeyAndOrderFront(None);
    OPTIONS.with_borrow_mut(|windows| windows.insert(id, window));

    Ok(())
}
