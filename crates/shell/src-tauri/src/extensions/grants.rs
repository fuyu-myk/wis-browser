//! What extensions were allowed while running: applied to each of their
//! contexts, and forgotten when an extension gives some back with
//! `permissions.remove`. Main thread only.

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
use wis_core::extensions::is_site_pattern;

use super::host;

thread_local! {
    static GRANTS: RefCell<Option<HashMap<String, Vec<String>>>> = const { RefCell::new(None) };
    static OBSERVERS: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { RefCell::new(Vec::new()) };
}

/// Takes the grants a new host read, unless a change already brought newer
/// ones, and starts noticing extensions giving some back.
pub(super) fn init(grants: HashMap<String, Vec<String>>) {
    watch();
    GRANTS.with_borrow_mut(|current| {
        current.get_or_insert(grants);
    });
}

/// Takes the latest grants, granting what is new and withdrawing what was
/// revoked in every loaded context.
pub(super) fn set(next: HashMap<String, Vec<String>>) {
    let previous = GRANTS.replace(Some(next.clone())).unwrap_or_default();
    let none = Vec::new();
    for (id, context) in host::contexts() {
        let before = previous.get(&id).unwrap_or(&none);
        let after = next.get(&id).unwrap_or(&none);
        let added: Vec<String> = after
            .iter()
            .filter(|g| !before.contains(g))
            .cloned()
            .collect();
        let revoked: Vec<String> = before
            .iter()
            .filter(|g| !after.contains(g))
            .cloned()
            .collect();
        if revoked.is_empty() {
            grant(&context, &added);
        } else {
            revoke(&context, &revoked);
            // Clearing a site also clears the narrower sites it covers.
            grant_required_sites(&context);
            grant(&context, after);
        }
    }
}

fn grant_required_sites(context: &WKWebExtensionContext) {
    let granted = WKWebExtensionContextPermissionStatus::GrantedExplicitly;
    // SAFETY: main thread; property reads and permission changes on a live context.
    unsafe {
        for pattern in context.webExtension().allRequestedMatchPatterns().iter() {
            context.setPermissionStatus_forMatchPattern(granted, &pattern);
        }
    }
}

/// Grants an extension's remembered extras to its context before it loads.
pub(super) fn apply(context: &WKWebExtensionContext, id: &str) {
    let items = GRANTS.with_borrow(|grants| {
        grants
            .as_ref()
            .and_then(|g| g.get(id).cloned())
            .unwrap_or_default()
    });
    grant(context, &items);
}

fn pattern(item: &str) -> Option<Retained<WKWebExtensionMatchPattern>> {
    let mtm = MainThreadMarker::new()?;
    // SAFETY: main thread; an invalid pattern gives nil.
    unsafe { WKWebExtensionMatchPattern::matchPatternWithString(&NSString::from_str(item), mtm) }
}

/// Grants what the extension still declares it may ask for.
fn grant(context: &WKWebExtensionContext, items: &[String]) {
    let granted = WKWebExtensionContextPermissionStatus::GrantedExplicitly;
    // SAFETY: main thread; property reads on a live context.
    let (optional, reachable) = unsafe {
        let extension = context.webExtension();
        let reachable: Vec<_> = extension
            .optionalPermissionMatchPatterns()
            .iter()
            .chain(extension.requestedPermissionMatchPatterns().iter())
            .collect();
        (extension.optionalPermissions(), reachable)
    };
    for item in items {
        if is_site_pattern(item) {
            let Some(pattern) = pattern(item) else {
                continue;
            };
            // SAFETY: main thread; matching and granting on live objects.
            unsafe {
                if reachable.iter().any(|r| r.matchesPattern(Some(&pattern))) {
                    context.setPermissionStatus_forMatchPattern(granted, &pattern);
                }
            }
        } else {
            let permission = NSString::from_str(item);
            if optional.containsObject(&permission) {
                // SAFETY: main thread; granting on a live context.
                unsafe { context.setPermissionStatus_forPermission(granted, &permission) };
            }
        }
    }
}

fn revoke(context: &WKWebExtensionContext, items: &[String]) {
    let unknown = WKWebExtensionContextPermissionStatus::Unknown;
    for item in items {
        // SAFETY: main thread; clearing a permission on a live context.
        unsafe {
            if is_site_pattern(item) {
                if let Some(pattern) = pattern(item) {
                    context.setPermissionStatus_forMatchPattern(unknown, &pattern);
                }
            } else {
                context.setPermissionStatus_forPermission(unknown, &NSString::from_str(item));
            }
        }
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
    let held: Vec<String> = GRANTS.with_borrow(|grants| {
        grants
            .as_ref()
            .and_then(|g| g.get(&id))
            .map(|items| {
                removed
                    .iter()
                    .filter(|r| items.contains(r))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    });
    if !held.is_empty() {
        super::forget_granted(id, held);
    }
}
