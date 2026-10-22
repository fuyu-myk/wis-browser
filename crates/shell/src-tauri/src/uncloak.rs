//! Each workspace's CNAME uncloaking state ([`wis_network::cloaking`]), and
//! which workspace each webview opens pages for.

use std::collections::HashSet;
use std::sync::{Arc, PoisonError};

use wis_core::WorkspaceId;
use wis_network::cloaking::Cloaking;

use crate::state::AppState;

/// WebKitGTK's navigation handler also sees subframes, so a page could spare
/// its own cloaked hosts by framing them.
pub const SUPPORTED: bool = !cfg!(target_os = "linux");

pub fn cloaking_for(state: &AppState, workspace: WorkspaceId) -> Arc<Cloaking> {
    state
        .cloaking
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(workspace)
        .or_default()
        .clone()
}

/// When a workspace's window closes, so a private workspace's next session
/// starts with none of this one's pages.
pub fn clear(state: &AppState, workspace: WorkspaceId) {
    let cloaking = state
        .cloaking
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&workspace)
        .cloned();
    if let Some(cloaking) = cloaking {
        cloaking.clear();
    }
}

pub fn forget_workspace(state: &AppState, workspace: WorkspaceId) {
    state
        .cloaking
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&workspace);
    state
        .webview_workspaces
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|_, owner| *owner != workspace);
}

/// Before creating the webview labelled `label`.
pub fn register(state: &AppState, label: &str, workspace: WorkspaceId) {
    state
        .webview_workspaces
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(label.to_owned(), workspace);
}

pub fn forget(state: &AppState, label: &str) {
    state
        .webview_workspaces
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(label);
}

fn cloaking_of(state: &AppState, label: &str) -> Option<Arc<Cloaking>> {
    if !SUPPORTED {
        return None;
    }
    let workspace = *state
        .webview_workspaces
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(label)?;
    Some(cloaking_for(state, workspace))
}

/// Before the webview's main frame navigates to `url`, so its workspace's
/// proxy spares the host.
pub fn opening_page(state: &AppState, label: &str, url: &str) {
    if let Some(cloaking) = cloaking_of(state, label) {
        cloaking.opening_page(url);
    }
}

pub fn was_uncloaked(state: &AppState, label: &str, host: &str) -> bool {
    cloaking_of(state, label).is_some_and(|cloaking| cloaking.was_uncloaked(host))
}

/// Sites with blocking off in the site panel, saved or this session, by
/// registrable domain.
pub fn unblocked_sites(state: &AppState) -> Arc<HashSet<String>> {
    let saved = state
        .site_protection
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let session = state
        .site_protection_session
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let sites = saved
        .iter()
        .filter(|(site, _)| !session.contains_key(*site))
        .chain(session.iter())
        .filter(|(_, protection)| !protection.blocking)
        .map(|(site, _)| wis_core::site::registrable_domain(site))
        .collect();
    Arc::new(sites)
}
