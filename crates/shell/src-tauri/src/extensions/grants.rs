//! Where each extension may run and what it was allowed while running:
//! applied to each of its contexts, and forgotten when an extension gives
//! some back with `permissions.remove`. Main thread only.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;

use block2_06::RcBlock;
use objc2_06::rc::Retained;
use objc2_06::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2_06::MainThreadMarker;
use objc2_foundation::{NSNotification, NSNotificationCenter, NSSet, NSString};
use objc2_web_kit::{
    WKWebExtensionContext,
    WKWebExtensionContextGrantedPermissionMatchPatternsWereRemovedNotification,
    WKWebExtensionContextGrantedPermissionsWereRemovedNotification,
    WKWebExtensionContextNotificationUserInfoKeyMatchPatterns,
    WKWebExtensionContextNotificationUserInfoKeyPermissions, WKWebExtensionContextPermissionStatus,
    WKWebExtensionMatchPattern,
};
use wis_core::extensions::access::{plain_host, site_pattern, SiteAccess};
use wis_core::extensions::installed::{is_site_pattern, InstalledExtension};

use super::host;

/// What an extension may reach, as the settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Access {
    /// Every site its manifest asks for, rather than only `hosts`.
    pub all_sites: bool,
    pub hosts: Vec<String>,
    /// Permissions and match patterns it was allowed while running.
    pub granted: Vec<String>,
}

impl Access {
    pub(super) fn of(ext: &InstalledExtension) -> Self {
        Self {
            all_sites: ext.site_access == SiteAccess::All,
            hosts: ext
                .allowed_sites
                .iter()
                .filter_map(|h| plain_host(h))
                .collect(),
            granted: ext.granted.clone(),
        }
    }

    fn everywhere() -> Self {
        Self {
            all_sites: true,
            hosts: Vec::new(),
            granted: Vec::new(),
        }
    }
}

thread_local! {
    static ACCESS: RefCell<Option<HashMap<String, Access>>> = const { RefCell::new(None) };
    static OBSERVERS: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { RefCell::new(Vec::new()) };
}

/// Takes what a new host read, unless a change already brought newer
/// settings, and starts noticing extensions giving some back.
pub(super) fn init(access: HashMap<String, Access>) {
    watch();
    ACCESS.with_borrow_mut(|current| {
        current.get_or_insert(access);
    });
}

pub(super) fn access_of(id: &str) -> Option<Access> {
    ACCESS.with_borrow(|access| access.as_ref().and_then(|a| a.get(id).cloned()))
}

/// Takes the latest settings, and moves every loaded context whose
/// extension's access changed to the new access.
pub(super) fn set(next: HashMap<String, Access>) {
    let previous = ACCESS.replace(Some(next.clone())).unwrap_or_default();
    for (id, context) in host::contexts() {
        let Some(after) = next.get(&id) else {
            continue;
        };
        let before = previous
            .get(&id)
            .cloned()
            .unwrap_or_else(Access::everywhere);
        if before != *after {
            change(&context, &before, after);
        }
    }
}

/// Withdraws what `before` granted and `after` does not, then grants `after`
/// again. Losing every host takes a reload.
fn change(context: &WKWebExtensionContext, before: &Access, after: &Access) {
    let kept: Vec<String> = sites(context, after).iter().map(|p| named(p)).collect();
    let gone: Vec<_> = sites(context, before)
        .into_iter()
        .filter(|p| !kept.contains(&named(p)))
        .collect();
    // SAFETY: main thread; property reads of live objects.
    let reload = gone.iter().any(|p| unsafe { p.matchesAllHosts() });
    let controller = if reload {
        // SAFETY: main thread; a property read on a live context.
        unsafe { context.webExtensionController() }
    } else {
        None
    };
    // SAFETY: main thread; the context is loaded in this controller.
    let controller =
        controller.filter(
            |c| match unsafe { c.unloadExtensionContext_error(context) } {
                Ok(()) => true,
                Err(err) => {
                    log::warn!(
                        "an extension was not reloaded: {}",
                        err.localizedDescription()
                    );
                    false
                }
            },
        );

    let unknown = WKWebExtensionContextPermissionStatus::Unknown;
    for site in &gone {
        // SAFETY: main thread; clearing on a live context.
        unsafe { context.setPermissionStatus_forMatchPattern(unknown, site) };
    }

    let kept = permissions(context, after);
    for permission in permissions(context, before) {
        if !kept.contains(&permission) {
            // SAFETY: main thread; clearing on a live context.
            unsafe { context.setPermissionStatus_forPermission(unknown, &permission) };
        }
    }
    grant(context, after);

    if let Some(controller) = controller {
        // SAFETY: main thread; the context was unloaded from this controller.
        if let Err(err) = unsafe { controller.loadExtensionContext_error(context) } {
            log::warn!(
                "an extension did not load again: {}",
                err.localizedDescription()
            );
        }
    }
}

/// Grants an extension's sites and extras to its context before it loads.
pub(super) fn apply(context: &WKWebExtensionContext, id: &str) {
    let access = access_of(id).unwrap_or_else(Access::everywhere);
    grant(context, &access);
}

/// Allows or stops an extension on a host for one private workspace's
/// context, leaving the settings alone.
pub(super) fn set_session_site(
    context: &WKWebExtensionContext,
    id: &str,
    host: &str,
    allowed: bool,
) {
    let status = if allowed {
        WKWebExtensionContextPermissionStatus::GrantedExplicitly
    } else {
        WKWebExtensionContextPermissionStatus::Unknown
    };
    for site in host_sites(context, host) {
        // SAFETY: main thread; changing a permission on a live context.
        unsafe { context.setPermissionStatus_forMatchPattern(status, &site) };
    }
    if !allowed {
        apply(context, id);
    }
}

fn named(pattern: &WKWebExtensionMatchPattern) -> String {
    // SAFETY: a property read of a live pattern on the main thread.
    unsafe { pattern.string() }.to_string()
}

/// The patterns that allow `host` within what the extension asks for: both
/// schemes when it asks for both, else the one it does.
fn host_sites(
    context: &WKWebExtensionContext,
    host: &str,
) -> Vec<Retained<WKWebExtensionMatchPattern>> {
    // SAFETY: main thread; a property read on a live context.
    let requested = unsafe { context.webExtension().allRequestedMatchPatterns() };
    // SAFETY: main thread; matching live patterns.
    let covered = |p: &WKWebExtensionMatchPattern| {
        requested
            .iter()
            .any(|r| unsafe { r.matchesPattern(Some(p)) })
    };
    let both = pattern(&site_pattern(host));
    if let Some(both) = both.filter(|p| covered(p)) {
        return vec![both];
    }

    ["https", "http"]
        .iter()
        .filter_map(|scheme| pattern(&format!("{scheme}://{host}/*")))
        .filter(|p| covered(p))
        .collect()
}

fn pattern(item: &str) -> Option<Retained<WKWebExtensionMatchPattern>> {
    let mtm = MainThreadMarker::new()?;
    // SAFETY: main thread; an invalid pattern gives nil.
    unsafe { WKWebExtensionMatchPattern::matchPatternWithString(&NSString::from_str(item), mtm) }
}

/// The sites `access` reaches: the manifest's or the allowed ones, and those
/// allowed while running that the extension still declares.
fn sites(
    context: &WKWebExtensionContext,
    access: &Access,
) -> Vec<Retained<WKWebExtensionMatchPattern>> {
    // SAFETY: main thread; property reads on a live context.
    let (requested, reachable) = unsafe {
        let extension = context.webExtension();
        let reachable: Vec<_> = extension
            .optionalPermissionMatchPatterns()
            .iter()
            .chain(extension.requestedPermissionMatchPatterns().iter())
            .collect();
        (extension.allRequestedMatchPatterns().to_vec(), reachable)
    };
    let mut sites: Vec<_> = if access.all_sites {
        requested
    } else {
        access
            .hosts
            .iter()
            .flat_map(|h| host_sites(context, h))
            .collect()
    };
    sites.extend(
        access
            .granted
            .iter()
            .filter(|g| is_site_pattern(g))
            .filter_map(|g| pattern(g))
            // SAFETY: main thread; matching live patterns.
            .filter(|p| {
                reachable
                    .iter()
                    .any(|r| unsafe { r.matchesPattern(Some(p)) })
            }),
    );

    sites
}

/// The permissions allowed while running that the extension still declares.
fn permissions(context: &WKWebExtensionContext, access: &Access) -> Vec<Retained<NSString>> {
    // SAFETY: main thread; a property read on a live context.
    let optional = unsafe { context.webExtension().optionalPermissions() };
    access
        .granted
        .iter()
        .filter(|g| !is_site_pattern(g))
        .map(|g| NSString::from_str(g))
        .filter(|p| optional.containsObject(p))
        .collect()
}

fn grant(context: &WKWebExtensionContext, access: &Access) {
    let granted = WKWebExtensionContextPermissionStatus::GrantedExplicitly;
    for site in sites(context, access) {
        // SAFETY: main thread; granting on a live context.
        unsafe { context.setPermissionStatus_forMatchPattern(granted, &site) };
    }
    for permission in permissions(context, access) {
        // SAFETY: main thread; granting on a live context.
        unsafe { context.setPermissionStatus_forPermission(granted, &permission) };
    }
}

fn watch() {
    if OBSERVERS.with_borrow(|o| !o.is_empty()) {
        return;
    }
    let center = NSNotificationCenter::defaultCenter();
    let removed = RcBlock::new(|note: NonNull<NSNotification>| {
        if MainThreadMarker::new().is_none() {
            return;
        }
        // SAFETY: the notification is alive for the block's call.
        given_back(unsafe { note.as_ref() });
    });
    // SAFETY: main thread; with no queue the block runs where WebKit posts,
    // which is the main queue, and the observers live for the app.
    let observers = unsafe {
        [
            WKWebExtensionContextGrantedPermissionsWereRemovedNotification,
            WKWebExtensionContextGrantedPermissionMatchPatternsWereRemovedNotification,
        ]
        .map(|name| {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &removed)
        })
    };
    OBSERVERS.with_borrow_mut(|o| o.extend(observers));
}

fn strings_under(note: &NSNotification, key: &NSString) -> Vec<String> {
    let Some(info) = note.userInfo() else {
        return Vec::new();
    };
    let Some(value) = info.objectForKey(key.as_ref()) else {
        return Vec::new();
    };
    let Ok(set) = value.downcast::<NSSet>() else {
        return Vec::new();
    };
    set.iter()
        .filter_map(|item: Retained<AnyObject>| {
            if let Some(name) = item.downcast_ref::<NSString>() {
                return Some(name.to_string());
            }
            // SAFETY: a property read of a pattern WebKit handed over.
            item.downcast_ref::<WKWebExtensionMatchPattern>()
                .map(|p| unsafe { p.string() }.to_string())
        })
        .collect()
}

fn given_back(note: &NSNotification) {
    let Some(context) = note
        .object()
        .and_then(|o| o.downcast::<WKWebExtensionContext>().ok())
    else {
        return;
    };
    // SAFETY: a property read on the main thread.
    let id = unsafe { context.uniqueIdentifier() }.to_string();
    // SAFETY: WebKit's notification keys, constant for the app.
    let keys = unsafe {
        [
            WKWebExtensionContextNotificationUserInfoKeyPermissions,
            WKWebExtensionContextNotificationUserInfoKeyMatchPatterns,
        ]
    };
    let removed: Vec<String> = keys
        .into_iter()
        .flat_map(|key| strings_under(note, key))
        .collect();
    let held: Vec<String> = ACCESS.with_borrow(|access| {
        access
            .as_ref()
            .and_then(|a| a.get(&id))
            .map(|a| &a.granted)
            .map(|items| {
                removed
                    .iter()
                    .filter(|r| items.contains(r))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    });
    let given_up: Vec<String> = held
        .into_iter()
        .filter(|item| !still_granted(&context, item))
        .collect();
    if !given_up.is_empty() {
        super::access::forget_granted(id, given_up);
    }
}

fn still_granted(context: &WKWebExtensionContext, item: &str) -> bool {
    let granted = WKWebExtensionContextPermissionStatus::GrantedImplicitly.0;
    // SAFETY: main thread; status reads on a live context.
    unsafe {
        if is_site_pattern(item) {
            pattern(item).is_some_and(|p| context.permissionStatusForMatchPattern(&p).0 >= granted)
        } else {
            context
                .permissionStatusForPermission(&NSString::from_str(item))
                .0
                >= granted
        }
    }
}
