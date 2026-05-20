//! Integration tests verifying that every public mutation on [`BrowserState`]
//! preserves the structural invariant defined by `invariant_violations`.

use wis_core::{
    AccentColor, BrowserState, PrivacyMode, ThemeMode, UserSettingsPatch, WisError, Workspace,
};

fn assert_clean(state: &BrowserState) {
    let violations = state.invariant_violations();
    assert!(
        violations.is_empty(),
        "expected no invariant violations, got: {:#?}",
        violations
    );
}

#[test]
fn default_state_has_one_workspace_no_tabs_and_no_violations() {
    let state = BrowserState::new_with_defaults();
    assert_eq!(state.workspaces.len(), 1);
    assert_eq!(state.tabs.len(), 0);
    assert_eq!(state.active_workspace(), &state.workspaces[0]);
    assert_clean(&state);
}

#[test]
fn creating_a_workspace_returns_a_new_state_without_mutating_the_old() {
    let before = BrowserState::new_with_defaults();
    let before_clone = before.clone();
    let (after, new_ws) = before.with_new_workspace("Work".into(), AccentColor::Mint);

    assert_eq!(before, before_clone, "old state must not be mutated");
    assert_eq!(after.workspaces.len(), 2);
    assert_eq!(new_ws.name, "Work");
    assert_eq!(new_ws.accent, AccentColor::Mint);
    assert_clean(&after);
}

#[test]
fn renaming_a_missing_workspace_errors() {
    let state = BrowserState::new_with_defaults();
    let ghost = Workspace::new("ghost".into(), AccentColor::Primary);
    let err = state
        .with_renamed_workspace(ghost.id, "x".into())
        .unwrap_err();
    assert!(matches!(err, WisError::WorkspaceNotFound { .. }));
}

#[test]
fn deleting_the_last_workspace_is_rejected() {
    let state = BrowserState::new_with_defaults();
    let only_id = state.workspaces[0].id;
    let err = state.with_workspace_removed(only_id).unwrap_err();
    assert!(matches!(err, WisError::CannotDeleteLastWorkspace));
}

#[test]
fn deleting_active_workspace_picks_a_new_active_and_drops_its_tabs() {
    let s0 = BrowserState::new_with_defaults();
    let ws_a_id = s0.workspaces[0].id;
    let (s1, ws_b) = s0.with_new_workspace("B".into(), AccentColor::Rose);
    let s2 = s1.with_new_tab(ws_a_id, "https://example.com").unwrap().0;
    assert_eq!(s2.tabs.len(), 1);

    let s3 = s2.with_workspace_removed(ws_a_id).unwrap();
    assert_eq!(s3.workspaces.len(), 1);
    assert_eq!(s3.active_workspace_id, ws_b.id);
    assert_eq!(s3.tabs.len(), 0, "tabs from deleted workspace must be gone");
    assert_clean(&s3);
}

#[test]
fn creating_a_tab_with_invalid_url_errors() {
    let state = BrowserState::new_with_defaults();
    let ws_id = state.workspaces[0].id;
    let err = state.with_new_tab(ws_id, "not a url").unwrap_err();
    assert!(matches!(err, WisError::InvalidUrl { .. }));
}

#[test]
fn creating_a_tab_in_a_missing_workspace_errors() {
    let state = BrowserState::new_with_defaults();
    let ghost = Workspace::new("ghost".into(), AccentColor::Primary);
    let err = state
        .with_new_tab(ghost.id, "https://example.com")
        .unwrap_err();
    assert!(matches!(err, WisError::WorkspaceNotFound { .. }));
}

#[test]
fn creating_a_tab_appends_it_to_the_workspace_and_activates_it() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, tab) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    assert_eq!(s1.tabs.len(), 1);
    let ws = s1.workspace(ws_id).unwrap();
    assert_eq!(ws.tab_order, vec![tab.id]);
    assert_eq!(ws.active_tab_id, Some(tab.id));
    assert_clean(&s1);
}

#[test]
fn closing_a_tab_removes_it_and_reactivates_a_neighbour() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://a.example").unwrap();
    let (s2, t2) = s1.with_new_tab(ws_id, "https://b.example").unwrap();
    let (s3, t3) = s2.with_new_tab(ws_id, "https://c.example").unwrap();

    let s4 = s3.with_active_tab(t2.id).unwrap();
    assert_eq!(s4.workspace(ws_id).unwrap().active_tab_id, Some(t2.id));

    let s5 = s4.with_tab_closed(t2.id).unwrap();
    assert_eq!(s5.tabs.len(), 2);
    let ws = s5.workspace(ws_id).unwrap();
    assert_eq!(ws.tab_order, vec![t1.id, t3.id]);
    assert_eq!(
        ws.active_tab_id,
        Some(t1.id),
        "closing the middle tab should activate the previous one"
    );
    assert_clean(&s5);
}

#[test]
fn closing_the_last_tab_in_a_workspace_leaves_active_tab_none() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let s2 = s1.with_tab_closed(t1.id).unwrap();
    assert_eq!(s2.workspace(ws_id).unwrap().active_tab_id, None);
    assert_clean(&s2);
}

#[test]
fn reorder_must_contain_exactly_the_same_tab_ids() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://a.example").unwrap();
    let (s2, t2) = s1.with_new_tab(ws_id, "https://b.example").unwrap();

    // Valid reorder: reverse.
    let s3 = s2.with_tab_order(ws_id, vec![t2.id, t1.id]).unwrap();
    assert_eq!(s3.workspace(ws_id).unwrap().tab_order, vec![t2.id, t1.id]);

    // Invalid reorder: missing tab.
    let err = s2.with_tab_order(ws_id, vec![t1.id]).unwrap_err();
    assert!(matches!(err, WisError::InvalidInput { .. }));

    // Invalid reorder: unknown tab.
    let ghost = wis_core::Tab::new(ws_id, url::Url::parse("https://x.example").unwrap());
    let err = s2
        .with_tab_order(ws_id, vec![t1.id, t2.id, ghost.id])
        .unwrap_err();
    assert!(matches!(err, WisError::InvalidInput { .. }));
}

#[test]
fn pinning_marks_the_tab_and_records_it_in_the_workspace() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();

    let (s2, t1_pinned) = s1.with_tab_pinned(t1.id, true).unwrap();
    assert!(t1_pinned.pinned);
    let ws = s2.workspace(ws_id).unwrap();
    assert!(ws.pinned_tab_ids.contains(&t1.id));

    let (s3, t1_unpinned) = s2.with_tab_pinned(t1.id, false).unwrap();
    assert!(!t1_unpinned.pinned);
    assert!(!s3.workspace(ws_id).unwrap().pinned_tab_ids.contains(&t1.id));
}

#[test]
fn settings_patch_updates_only_specified_fields() {
    let s0 = BrowserState::new_with_defaults();
    let before = s0.settings.clone();
    let patch = UserSettingsPatch {
        theme: Some(ThemeMode::Dark),
        glass_enabled: Some(false),
        ..Default::default()
    };
    let s1 = s0.with_settings(patch);
    assert_eq!(s1.settings.theme, ThemeMode::Dark);
    assert!(!s1.settings.glass_enabled);
    assert_eq!(s1.settings.layout, before.layout);
    assert_eq!(
        s1.settings.default_privacy_mode,
        before.default_privacy_mode
    );
    assert_eq!(s1.settings.doh_resolver, before.doh_resolver);
}

#[test]
fn switching_active_tab_touches_its_last_active_timestamp() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://a.example").unwrap();
    let original_ts = t1.last_active_at;
    std::thread::sleep(std::time::Duration::from_millis(2));
    let s2 = s1.with_active_tab(t1.id).unwrap();
    let touched = s2.tab(t1.id).unwrap();
    assert!(touched.last_active_at > original_ts);
}

#[test]
fn setting_a_workspaces_privacy_mode_is_isolated_to_that_workspace() {
    let s0 = BrowserState::new_with_defaults();
    let (s1, ws_b) = s0.with_new_workspace("B".into(), AccentColor::Sun);
    let updated_workspace = ws_b.with_privacy_mode(PrivacyMode::Strict);
    let workspaces = s1
        .workspaces
        .iter()
        .map(|w| {
            if w.id == ws_b.id {
                updated_workspace.clone()
            } else {
                w.clone()
            }
        })
        .collect();
    let s2 = BrowserState {
        workspaces,
        ..s1.clone()
    };
    assert_eq!(
        s2.workspace(ws_b.id).unwrap().privacy_mode,
        PrivacyMode::Strict
    );
    assert_eq!(
        s2.workspaces[0].privacy_mode,
        PrivacyMode::Balanced,
        "default workspace must be unaffected"
    );
}
