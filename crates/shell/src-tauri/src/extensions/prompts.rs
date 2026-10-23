//! Asking the user when a running extension wants more than it was installed
//! with: `permissions.request`, or a site an extension page fetched from.
//! WebKit asks about permissions and sites separately in one turn; they are
//! gathered into one question, answered all or nothing. Main thread only.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use block2_06::{DynBlock, RcBlock};
use objc2_06::rc::Retained;
use objc2_foundation::{NSDate, NSOperationQueue, NSSet, NSString, NSURL};
use objc2_web_kit::{WKWebExtensionContext, WKWebExtensionMatchPattern};
use serde::Serialize;
use tauri::Emitter;
use wis_core::WorkspaceId;

pub(super) type Done<T> = DynBlock<dyn Fn(NonNull<NSSet<T>>, *mut NSDate)>;

/// One of WebKit's questions and how to answer it.
pub(super) enum Asked {
    Permissions(
        RcBlock<dyn Fn(NonNull<NSSet<NSString>>, *mut NSDate)>,
        Retained<NSSet<NSString>>,
    ),
    Patterns(
        RcBlock<dyn Fn(NonNull<NSSet<WKWebExtensionMatchPattern>>, *mut NSDate)>,
        Retained<NSSet<WKWebExtensionMatchPattern>>,
    ),
    Urls(
        RcBlock<dyn Fn(NonNull<NSSet<NSURL>>, *mut NSDate)>,
        Retained<NSSet<NSURL>>,
    ),
}

struct Pending {
    request: u64,
    workspace: WorkspaceId,
    /// A private workspace's grants last only as long as it does.
    private: bool,
    id: String,
    name: String,
    asked: Vec<Asked>,
    at: Instant,
    shown: bool,
}

impl Pending {
    fn deny(&self) {
        for asked in &self.asked {
            asked.answer(false);
        }
    }
}

/// What the prompt shows.
#[derive(Debug, Clone, Serialize)]
pub struct PermissionRequest {
    pub request: u64,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub abilities: Vec<String>,
}

pub const PERMISSION_REQUEST_EVENT: &str = "wis-extension-permission-request";

/// WebKit gives up after two minutes and denies; an answer later than this
/// counts for nothing.
const ANSWER_WITHIN: Duration = Duration::from_secs(110);

thread_local! {
    static PENDING: RefCell<Vec<Pending>> = const { RefCell::new(Vec::new()) };
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// The site a URL is on, as the pattern WebKit grants for it.
fn origin_pattern(url: &NSURL) -> Option<String> {
    Some(format!("{}://{}/*", url.scheme()?, url.host()?))
}

impl Asked {
    /// What it asks for, as the settings remember it.
    fn items(&self) -> Vec<String> {
        match self {
            Asked::Permissions(_, set) => set.iter().map(|p| p.to_string()).collect(),
            // SAFETY: property reads on the main thread.
            Asked::Patterns(_, set) => set
                .iter()
                .map(|p| unsafe { p.string() }.to_string())
                .collect(),
            Asked::Urls(_, set) => set.iter().filter_map(|u| origin_pattern(&u)).collect(),
        }
    }

    fn answer(&self, allow: bool) {
        match self {
            Asked::Permissions(done, set) => reply(done, set, allow),
            Asked::Patterns(done, set) => reply(done, set, allow),
            Asked::Urls(done, set) => reply(done, set, allow),
        }
    }
}

fn reply<T: objc2_06::Message>(done: &Done<T>, asked: &NSSet<T>, allow: bool) {
    let none = NSSet::<T>::new();
    let given = if allow { asked } else { &none };
    done.call((NonNull::from(given), std::ptr::null_mut()));
}

/// Takes one of WebKit's questions, joining it to an unshown one from the
/// same extension in the same workspace.
pub(super) fn ask(workspace: WorkspaceId, context: &WKWebExtensionContext, asked: Asked) {
    // SAFETY: property reads on the main thread.
    let (id, name) = unsafe {
        (
            context.uniqueIdentifier().to_string(),
            context
                .webExtension()
                .displayName()
                .map(|n| wis_core::extensions::installed::tidy_name(&n.to_string()))
                .unwrap_or_default(),
        )
    };
    let stale: Vec<Pending> = PENDING.with_borrow_mut(|pending| {
        let (stale, live) = std::mem::take(pending)
            .into_iter()
            .partition(|p| p.at.elapsed() >= ANSWER_WITHIN);
        *pending = live;
        stale
    });
    stale.iter().for_each(Pending::deny);

    let private = super::host::is_private(workspace);
    let new = PENDING.with_borrow_mut(|pending| {
        match pending
            .iter_mut()
            .find(|p| !p.shown && p.workspace == workspace && p.id == id)
        {
            Some(open) => {
                open.asked.push(asked);
                None
            }
            None => {
                let request = NEXT.replace(NEXT.get() + 1);
                pending.push(Pending {
                    request,
                    workspace,
                    private,
                    id,
                    name,
                    asked: vec![asked],
                    at: Instant::now(),
                    shown: false,
                });
                Some(request)
            }
        }
    });
    if let Some(request) = new {
        let show = RcBlock::new(move || show(request));
        // SAFETY: the main queue runs the block on a later turn of the main thread.
        unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&show) };
    }
}

fn show(request: u64) {
    let found = PENDING.with_borrow_mut(|pending| {
        let open = pending.iter_mut().find(|p| p.request == request)?;
        open.shown = true;
        let items: Vec<String> = open.asked.iter().flat_map(Asked::items).collect();
        Some((open.workspace, open.name.clone(), items))
    });
    let Some((workspace, name, items)) = found else {
        return;
    };

    let (patterns, permissions): (Vec<String>, Vec<String>) = items
        .into_iter()
        .partition(|i| wis_core::extensions::installed::is_site_pattern(i));
    let abilities = wis_core::extensions::access::request_abilities(&permissions, &patterns);
    if abilities.is_empty() {
        answer(request, workspace, true);
        return;
    }

    // The popup that asked would cover the prompt; WebKit still answers it.
    super::actions::dismiss_popup();
    let payload = PermissionRequest {
        request,
        workspace_id: workspace,
        name,
        abilities,
    };
    let sent = super::APP
        .get()
        .map(|app| app.emit(PERMISSION_REQUEST_EVENT, payload));
    if !matches!(sent, Some(Ok(()))) {
        log::warn!("an extension's request could not be shown");
        answer(request, workspace, false);
    }
}

/// Answers a request from `workspace`, remembering what was allowed.
pub(super) fn answer(request: u64, workspace: WorkspaceId, allow: bool) {
    let taken = PENDING.with_borrow_mut(|pending| {
        let at = pending
            .iter()
            .position(|p| p.request == request && p.workspace == workspace)?;
        Some(pending.remove(at))
    });
    let Some(pending) = taken else {
        return;
    };
    let allow = allow && pending.at.elapsed() < ANSWER_WITHIN;
    if !allow {
        pending.deny();
        return;
    }

    for asked in &pending.asked {
        asked.answer(true);
    }
    if !pending.private {
        let items = pending.asked.iter().flat_map(Asked::items).collect();
        super::access::remember_granted(pending.id, items);
    }
}

/// Denies the questions of a workspace whose window closed.
pub(super) fn forget(workspace: WorkspaceId) {
    let closed: Vec<Pending> = PENDING.with_borrow_mut(|pending| {
        let (closed, open) = std::mem::take(pending)
            .into_iter()
            .partition(|p| p.workspace == workspace);
        *pending = open;
        closed
    });
    closed.iter().for_each(Pending::deny);
}
