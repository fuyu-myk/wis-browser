//! Per-site protection in the page: Wis's content rule lists, element hiding
//! and adblock scriptlets (`blocking`), the page's own scripts (`javascript`)
//! and WebKit's fingerprinting protection (`fingerprinting`). The workspace
//! proxy still refuses listed servers, since it cannot tell which page a
//! request came from. A change made in a private workspace is held in memory
//! until Wis quits, never saved.

use std::collections::HashMap;

use tauri::{AppHandle, Emitter, Manager, State};
use wis_core::site_protection::with_site_protection;
use wis_core::user_agent::normalize_host;
use wis_core::{site_protection_for_host, SiteProtection, TabId, WisError, WisResult};

use crate::state::AppState;

pub const SESSION_CHANGED_EVENT: &str = "wis-site-protection-session";

pub fn for_url(
    saved: &HashMap<String, SiteProtection>,
    session: &HashMap<String, SiteProtection>,
    url: &str,
) -> SiteProtection {
    let Some(host) = url::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .and_then(|u| u.host_str().map(str::to_owned))
    else {
        return SiteProtection::default();
    };
    session
        .get(&normalize_host(&host))
        .copied()
        .unwrap_or_else(|| site_protection_for_host(&host, saved))
}

/// Switches `site`, the site the panel showed for the tab, and reloads the
/// tab. Refused when the tab has moved to another site since.
#[tauri::command]
pub async fn tab_set_site_protection(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    site: String,
    protection: SiteProtection,
) -> WisResult<()> {
    let live = app
        .webviews()
        .get(&format!("tab-{}", id.as_uuid()))
        .and_then(|webview| webview.url().ok());
    let (stored, private) = {
        let browser = state.browser.lock().await;
        let tab = browser.tab(id)?;
        let private = browser.workspace(tab.workspace_id)?.ephemeral;
        (tab.url.clone(), private)
    };
    let url = live.clone().unwrap_or(stored);
    let host = match (url.scheme(), url.host_str()) {
        ("http" | "https", Some(host)) => normalize_host(host),
        _ => String::new(),
    };
    if host.is_empty() || host != normalize_host(&site) {
        return Err(WisError::InvalidInput {
            message: "the tab no longer shows this site".into(),
        });
    }

    let unblocked = crate::uncloak::unblocked_sites(&state);
    let session = {
        let mut session = state
            .site_protection_session
            .write()
            .expect("site_protection_session poisoned");
        let changed = if private {
            session.insert(host.clone(), protection) != Some(protection)
        } else {
            session.remove(&host).is_some()
        };
        changed.then(|| session.clone())
    };
    if let Some(session) = session {
        if let Err(err) = app.emit(SESSION_CHANGED_EVENT, session) {
            log::warn!("site protection session event failed: {err}");
        }
    }

    if !private {
        crate::commands::mutate_unit_immediate(&app, &state, move |s| {
            let overrides =
                with_site_protection(&s.settings.site_protection_overrides, &host, protection);
            Ok(s.with_settings(wis_core::UserSettingsPatch {
                site_protection_overrides: Some(overrides),
                ..Default::default()
            }))
        })
        .await?;
        state.refresh_site_protection_cache().await;
    }
    if crate::uncloak::unblocked_sites(&state) != unblocked {
        state.refresh_all_proxy_policies().await;
    }

    if live.is_some() {
        crate::commands::tab_reload(app, state, id).await?;
    }
    Ok(())
}

/// The switches changed in private workspaces this session, by host.
#[tauri::command]
pub fn site_protection_session(state: State<'_, AppState>) -> HashMap<String, SiteProtection> {
    state
        .site_protection_session
        .read()
        .expect("site_protection_session poisoned")
        .clone()
}

#[cfg(target_os = "macos")]
pub mod imp {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};
    use objc2_foundation::{NSSet, NSString};

    use wis_core::SiteProtection;

    /// Turning blocking off skips only `wis_rule_lists`, so extensions' rules
    /// keep applying.
    ///
    /// # Safety
    ///
    /// Main thread only; `preferences` is null or a live `WKWebpagePreferences`.
    pub unsafe fn apply(
        preferences: *mut AnyObject,
        protection: SiteProtection,
        wis_rule_lists: &[String],
    ) {
        if preferences.is_null() {
            return;
        }
        let _: () = msg_send![preferences, setAllowsContentJavaScript: protection.javascript];
        if !protection.fingerprinting {
            set_fingerprinting_protection(preferences, false);
        }
        if protection.blocking {
            return;
        }
        let supported: bool = msg_send![
            preferences,
            respondsToSelector: sel!(_setContentRuleListsEnabled:exceptions:)
        ];
        if !supported {
            return;
        }

        let names: Vec<_> = wis_rule_lists
            .iter()
            .map(|id| NSString::from_str(id))
            .collect();
        let skipped = NSSet::from_retained_slice(&names);
        let skipped = objc2_06::rc::Retained::as_ptr(&skipped) as *mut AnyObject;
        let _: () = msg_send![preferences, _setContentRuleListsEnabled: true, exceptions: skipped];
    }

    /// WebKit's Advanced Tracking and Fingerprinting Protection (macOS 13.3+).
    ///
    /// # Safety
    ///
    /// Main thread only; `preferences` is a live `WKWebpagePreferences`.
    pub unsafe fn set_fingerprinting_protection(preferences: *mut AnyObject, enabled: bool) {
        let supported: bool = msg_send![
            preferences,
            respondsToSelector: sel!(_setNetworkConnectionIntegrityEnabled:)
        ];
        if supported {
            let _: () = msg_send![preferences, _setNetworkConnectionIntegrityEnabled: enabled];
        }
    }

    #[cfg(test)]
    mod tests {
        use objc2::runtime::{AnyObject, Bool};
        use objc2::{class, sel};

        #[test]
        fn every_send_matches_its_method() {
            let preferences = class!(WKWebpagePreferences);
            preferences
                .verify_sel::<(Bool, *mut AnyObject), ()>(
                    sel!(_setContentRuleListsEnabled:exceptions:),
                )
                .unwrap();
            preferences
                .verify_sel::<(Bool,), ()>(sel!(setAllowsContentJavaScript:))
                .unwrap();
            preferences
                .verify_sel::<(Bool,), ()>(sel!(_setNetworkConnectionIntegrityEnabled:))
                .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFF: SiteProtection = SiteProtection {
        blocking: false,
        javascript: false,
        fingerprinting: false,
    };

    #[test]
    fn only_websites_take_a_site_s_protection() {
        let saved = HashMap::from([("localhost".to_string(), OFF)]);
        let none = HashMap::new();
        assert_eq!(for_url(&saved, &none, "http://localhost:3000/"), OFF);
        assert_eq!(for_url(&saved, &none, "https://www.localhost/a"), OFF);
        assert_eq!(
            for_url(&saved, &none, "tauri://localhost/index.html"),
            SiteProtection::default()
        );
        assert_eq!(
            for_url(&saved, &none, "about:blank"),
            SiteProtection::default()
        );
    }

    #[test]
    fn a_private_session_s_switch_wins_over_the_saved_one() {
        let saved = HashMap::from([("a.example".to_string(), OFF)]);
        let session = HashMap::from([("a.example".to_string(), SiteProtection::default())]);
        assert_eq!(
            for_url(&saved, &session, "https://www.a.example/"),
            SiteProtection::default()
        );
        assert_eq!(for_url(&saved, &HashMap::new(), "https://a.example/"), OFF);
    }
}
