//! Integration tests verifying that every public mutation on [`BrowserState`]
//! preserves the structural invariant defined by `invariant_violations`.

use wis_core::{
    AccentColor, BrowserState, PrivacyMode, SplitDir, ThemeMode, UserSettingsPatch, WisError,
    Workspace,
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
fn background_tab_appends_without_stealing_focus() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    assert_eq!(s1.workspaces[0].active_tab_id, Some(a.id));

    // A background open adds the tab but leaves the opener active.
    let (s2, b) = s1
        .with_new_background_tab(ws, "https://b.example/")
        .unwrap();
    assert!(s2.tabs.iter().any(|t| t.id == b.id));
    assert_eq!(
        s2.workspaces[0].active_tab_id,
        Some(a.id),
        "opener stays active"
    );
    assert_clean(&s2);

    // A normal open still focuses the new tab.
    let (s3, c) = s2.with_new_tab(ws, "https://c.example/").unwrap();
    assert_eq!(s3.workspaces[0].active_tab_id, Some(c.id));
    assert_clean(&s3);
}

#[test]
fn pane_split_close_unsplit_preserve_invariants() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, _b) = s1
        .with_pane_split(ws, SplitDir::Horizontal, false, "https://b.example/")
        .unwrap();
    assert!(s2.workspaces[0].pane_tree.is_some());
    assert_clean(&s2);

    // Drag a third (background) tab onto pane `a`, placing it below.
    let (s3, c) = s2.with_new_tab(ws, "https://c.example/").unwrap();
    let s4 = s3
        .with_pane_split_with(ws, a.id, SplitDir::Vertical, false, c.id)
        .unwrap();
    assert_clean(&s4);
    let leaves = s4.workspaces[0].pane_tree.as_ref().unwrap().leaves();
    assert!(leaves.contains(&a.id) && leaves.contains(&c.id));
    assert_eq!(s4.workspaces[0].active_tab_id, Some(c.id));

    // Unsplit collapses to a single pane but keeps the tabs open.
    let s5 = s4.with_panes_unsplit(ws).unwrap();
    assert!(s5.workspaces[0].pane_tree.is_none());
    assert_eq!(s5.tabs.len(), s4.tabs.len());
    assert_clean(&s5);
}

/// `a` and `c` open, `c` in the background, then `a` split against a new `b`,
/// which takes focus.
fn split_with_a_spare_tab() -> (BrowserState, [wis_core::TabId; 3]) {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, c) = s1.with_new_tab(ws, "https://c.example/").unwrap();
    let s3 = s2.with_active_tab(a.id).unwrap();
    let (s4, b) = s3
        .with_pane_split(ws, SplitDir::Horizontal, false, "https://b.example/")
        .unwrap();
    assert_eq!(s4.workspaces[0].active_tab_id, Some(b.id));
    (s4, [a.id, b.id, c.id])
}

#[test]
fn closing_a_split_pane_refills_it_and_keeps_the_split() {
    let (s, [a, b, c]) = split_with_a_spare_tab();

    let next = s.with_tab_closed_keeping_split(b, &[b, a, c]).unwrap();
    assert_clean(&next);
    let leaves = next.workspaces[0].pane_tree.as_ref().unwrap().leaves();
    assert_eq!(
        leaves,
        vec![a, c],
        "the spare tab takes the closed pane's place"
    );
    assert_eq!(next.workspaces[0].active_tab_id, Some(c));
    assert!(next.tab(b).is_err());
}

#[test]
fn the_refill_is_the_most_recent_tab_not_in_a_pane() {
    let (s, [a, b, c]) = split_with_a_spare_tab();
    let ws = s.workspaces[0].id;
    let (s, d) = s.with_new_background_tab(ws, "https://d.example/").unwrap();

    let next = s
        .with_tab_closed_keeping_split(b, &[b, a, d.id, c])
        .unwrap();
    let leaves = next.workspaces[0].pane_tree.as_ref().unwrap().leaves();
    assert_eq!(leaves, vec![a, d.id], "recency wins over tab order");
}

#[test]
fn closing_an_unfocused_pane_keeps_focus_where_it_is() {
    let (s, [a, b, c]) = split_with_a_spare_tab();

    let next = s.with_tab_closed_keeping_split(a, &[]).unwrap();
    assert_clean(&next);
    let leaves = next.workspaces[0].pane_tree.as_ref().unwrap().leaves();
    assert_eq!(
        leaves,
        vec![c, b],
        "with no history, the nearest tab in order"
    );
    assert_eq!(next.workspaces[0].active_tab_id, Some(b));
}

#[test]
fn a_split_with_no_spare_tab_collapses_as_before() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, b) = s1
        .with_pane_split(ws, SplitDir::Horizontal, false, "https://b.example/")
        .unwrap();

    let next = s2
        .with_tab_closed_keeping_split(b.id, &[b.id, a.id])
        .unwrap();
    assert_clean(&next);
    assert!(next.workspaces[0].pane_tree.is_none());
    assert_eq!(next.workspaces[0].active_tab_id, Some(a.id));
}

#[test]
fn closing_a_tab_outside_the_split_leaves_the_split_alone() {
    let (s, [a, b, c]) = split_with_a_spare_tab();

    let next = s.with_tab_closed_keeping_split(c, &[c, b, a]).unwrap();
    assert_clean(&next);
    let leaves = next.workspaces[0].pane_tree.as_ref().unwrap().leaves();
    assert_eq!(leaves, vec![a, b]);
    assert_eq!(next.workspaces[0].active_tab_id, Some(b));
}

#[test]
fn center_drop_replaces_a_placeholder_pane_and_closes_it() {
    // Right-click-the-active-tab split leaves an `about:blank` placeholder in the
    // complement pane; dropping a (pre-existing, background) tab on its centre
    // must swap it in and close the throwaway.
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, b) = s1.with_new_tab(ws, "https://b.example/").unwrap();
    // `b` is now a background tab; re-focus `a`, then split it against a blank.
    let s3 = s2.with_active_tab(a.id).unwrap();
    let (s4, placeholder) = s3
        .with_pane_split(ws, SplitDir::Horizontal, false, "about:blank")
        .unwrap();
    assert_clean(&s4);

    let s5 = s4
        .with_pane_leaf_replaced(ws, placeholder.id, b.id)
        .unwrap();
    assert_clean(&s5);

    let leaves = s5.workspaces[0].pane_tree.as_ref().unwrap().leaves();
    assert!(leaves.contains(&a.id) && leaves.contains(&b.id));
    assert!(
        !leaves.contains(&placeholder.id),
        "placeholder left the layout"
    );
    assert!(
        s5.tabs.iter().all(|t| t.id != placeholder.id),
        "placeholder tab was closed"
    );
    assert_eq!(s5.workspaces[0].active_tab_id, Some(b.id));
}

#[test]
fn center_drop_onto_a_real_pane_keeps_the_displaced_tab() {
    // Replacing a pane that shows a real page keeps that page as a background tab.
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, c) = s1.with_new_tab(ws, "https://c.example/").unwrap();
    // `c` is a background tab; re-focus `a`, then split it against a real page `b`.
    let s3 = s2.with_active_tab(a.id).unwrap();
    let (s4, b) = s3
        .with_pane_split(ws, SplitDir::Horizontal, false, "https://b.example/")
        .unwrap();
    assert_clean(&s4);

    let s5 = s4.with_pane_leaf_replaced(ws, b.id, c.id).unwrap();
    assert_clean(&s5);

    let leaves = s5.workspaces[0].pane_tree.as_ref().unwrap().leaves();
    assert!(leaves.contains(&c.id) && !leaves.contains(&b.id));
    assert!(
        s5.tabs.iter().any(|t| t.id == b.id),
        "displaced real page stays open as a background tab"
    );
    assert_eq!(s5.workspaces[0].active_tab_id, Some(c.id));
}

#[test]
fn center_drop_onto_settings_keeps_it_open() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, c) = s1.with_new_tab(ws, "https://c.example/").unwrap();
    let s3 = s2.with_active_tab(a.id).unwrap();
    let (s4, settings) = s3
        .with_pane_split(ws, SplitDir::Horizontal, false, "about:settings")
        .unwrap();
    assert_clean(&s4);

    let s5 = s4.with_pane_leaf_replaced(ws, settings.id, c.id).unwrap();
    assert_clean(&s5);

    let leaves = s5.workspaces[0].pane_tree.as_ref().unwrap().leaves();
    assert!(leaves.contains(&c.id) && !leaves.contains(&settings.id));
    assert!(
        s5.tabs.iter().any(|t| t.id == settings.id),
        "about:settings stays open as a background tab"
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
fn a_workspace_s_lockdown_switch_is_its_own() {
    let s0 = BrowserState::new_with_defaults();
    let (s1, other) = s0.with_new_workspace("Work".into(), AccentColor::Mint);
    let first = s1.workspaces[0].id;
    assert!(!s1.workspaces[0].lockdown);

    let s2 = s1.with_workspace_lockdown(first, true).unwrap();
    assert!(s2.workspace(first).unwrap().lockdown);
    assert!(!s2.workspace(other.id).unwrap().lockdown);
    assert!(!s1.workspaces[0].lockdown);
    assert_clean(&s2);

    let ghost = Workspace::new("ghost".into(), AccentColor::Primary);
    assert!(matches!(
        s2.with_workspace_lockdown(ghost.id, true).unwrap_err(),
        WisError::WorkspaceNotFound { .. }
    ));
}

#[test]
fn new_workspaces_take_the_default_privacy_mode() {
    let state = BrowserState::new_with_defaults().with_settings(UserSettingsPatch {
        default_privacy_mode: Some(PrivacyMode::Strict),
        ..Default::default()
    });
    let (state, regular) = state.with_new_workspace("Work".into(), AccentColor::Mint);
    let (_, private) = state.with_new_private_workspace("Private".into(), AccentColor::Rose);

    assert_eq!(regular.privacy_mode, PrivacyMode::Strict);
    assert_eq!(private.privacy_mode, PrivacyMode::Strict);
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

    // Patching the custom-engine list leaves the built-in choice untouched
    // and vice versa.
    let s2 = s1.with_settings(UserSettingsPatch {
        custom_search_engines: Some(vec![wis_core::CustomSearchEngine {
            name: "Kagi".into(),
            keyword: "k".into(),
            template: "https://kagi.com/search?q={searchTerms}".into(),
            is_default: false,
        }]),
        ..Default::default()
    });
    assert_eq!(s2.settings.search_engine, before.search_engine);
    let s3 = s2.with_settings(UserSettingsPatch {
        search_engine: Some(wis_core::SearchEngine::Bing),
        ..Default::default()
    });
    assert_eq!(s3.settings.custom_search_engines.len(), 1);
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

#[test]
fn with_tab_title_updates_only_the_title() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let original_url = t1.url.clone();

    let s2 = s1.with_tab_title(t1.id, "Hello World".into()).unwrap();
    let after = s2.tab(t1.id).unwrap();
    assert_eq!(after.title, "Hello World");
    assert_eq!(after.url, original_url, "URL must not change");
    assert_clean(&s2);
}

#[test]
fn with_tab_title_is_idempotent_when_value_unchanged() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let s2 = s1.with_tab_title(t1.id, t1.title.clone()).unwrap();
    assert_eq!(s1, s2, "no-op title update must produce an equal state");
}

#[test]
fn with_tab_title_errors_on_missing_tab() {
    let s0 = BrowserState::new_with_defaults();
    let ghost = wis_core::TabId::new();
    let err = s0.with_tab_title(ghost, "x".into()).unwrap_err();
    assert!(matches!(err, WisError::TabNotFound { .. }));
}

#[test]
fn with_tab_loading_flips_the_loading_flag() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    assert!(!s1.tab(t1.id).unwrap().loading);

    let s2 = s1.with_tab_loading(t1.id, true).unwrap();
    assert!(s2.tab(t1.id).unwrap().loading);

    let s3 = s2.with_tab_loading(t1.id, false).unwrap();
    assert!(!s3.tab(t1.id).unwrap().loading);
}

#[test]
fn with_tab_loading_is_idempotent_when_value_unchanged() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let s2 = s1.with_tab_loading(t1.id, false).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn with_tab_nav_state_updates_both_directions() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let tab = s1.tab(t1.id).unwrap();
    assert!(!tab.can_go_back);
    assert!(!tab.can_go_forward);

    let s2 = s1.with_tab_nav_state(t1.id, true, false).unwrap();
    let tab = s2.tab(t1.id).unwrap();
    assert!(tab.can_go_back);
    assert!(!tab.can_go_forward);

    let s3 = s2.with_tab_nav_state(t1.id, false, true).unwrap();
    let tab = s3.tab(t1.id).unwrap();
    assert!(!tab.can_go_back);
    assert!(tab.can_go_forward);
}

#[test]
fn with_tab_nav_state_is_idempotent_when_value_unchanged() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let s2 = s1.with_tab_nav_state(t1.id, false, false).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn with_tab_url_synced_updates_url_but_preserves_title() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let s2 = s1.with_tab_title(t1.id, "My Page".into()).unwrap();

    let s3 = s2
        .with_tab_url_synced(t1.id, "https://example.com/other")
        .unwrap();

    let after = s3.tab(t1.id).unwrap();
    assert_eq!(after.url.as_str(), "https://example.com/other");
    assert_eq!(
        after.title, "My Page",
        "title must be preserved by url sync"
    );
    assert_clean(&s3);
}

#[test]
fn with_tab_url_synced_is_idempotent_when_url_unchanged() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let s2 = s1
        .with_tab_url_synced(t1.id, "https://example.com")
        .unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn with_tab_url_synced_errors_on_bad_url() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://example.com").unwrap();
    let err = s1
        .with_tab_url_synced(t1.id, "definitely not a url")
        .unwrap_err();
    assert!(matches!(err, WisError::InvalidUrl { .. }));
}

#[test]
fn tab_folders_create_assign_collapse_delete_preserve_invariants() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, _b) = s1.with_new_tab(ws, "https://b.example/").unwrap();
    assert_clean(&s2);

    // Create a folder and move `a` into it.
    let (s3, folder) = s2
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .unwrap();
    let s4 = s3.with_tab_assigned_folder(a.id, Some(folder.id)).unwrap();
    assert_eq!(s4.tab(a.id).unwrap().folder_id, Some(folder.id));
    assert_clean(&s4);

    // A new tab opened while a foldered tab is active inherits the folder.
    let s5 = s4.with_active_tab(a.id).unwrap();
    let (s6, c) = s5.with_new_tab(ws, "https://c.example/").unwrap();
    assert_eq!(s6.tab(c.id).unwrap().folder_id, Some(folder.id));
    assert_clean(&s6);

    // Rename, recolor, collapse: still clean.
    let s7 = s6
        .with_tab_folder_collapsed(ws, folder.id, true)
        .unwrap()
        .with_tab_folder_renamed(ws, folder.id, "Renamed".into())
        .unwrap()
        .with_tab_folder_recolored(ws, folder.id, AccentColor::Rose)
        .unwrap();
    let f = s7.workspaces[0]
        .tab_folders
        .iter()
        .find(|f| f.id == folder.id)
        .unwrap();
    assert!(f.collapsed);
    assert_eq!(f.name, "Renamed");
    assert_eq!(f.accent, AccentColor::Rose);
    assert_clean(&s7);

    // Deleting the folder leaves its members open but loose.
    let s8 = s7.with_tab_folder_removed(ws, folder.id).unwrap();
    assert!(s8.workspaces[0].tab_folders.is_empty());
    assert_eq!(s8.tab(a.id).unwrap().folder_id, None);
    assert_eq!(s8.tab(c.id).unwrap().folder_id, None);
    assert_clean(&s8);
}

#[test]
fn pinning_and_foldering_are_mutually_exclusive() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, folder) = s1
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .unwrap();
    let s3 = s2.with_tab_assigned_folder(a.id, Some(folder.id)).unwrap();

    // Pinning a foldered tab clears its folder membership.
    let (s4, _) = s3.with_tab_pinned(a.id, true).unwrap();
    assert!(s4.tab(a.id).unwrap().pinned);
    assert_eq!(s4.tab(a.id).unwrap().folder_id, None);
    assert_clean(&s4);

    // Assigning a pinned tab to a folder unpins it.
    let s5 = s4.with_tab_assigned_folder(a.id, Some(folder.id)).unwrap();
    assert!(!s5.tab(a.id).unwrap().pinned);
    assert_eq!(s5.tab(a.id).unwrap().folder_id, Some(folder.id));
    assert!(!s5.workspaces[0].pinned_tab_ids.contains(&a.id));
    assert_clean(&s5);
}

#[test]
fn selected_folder_targets_new_tabs_and_clears_on_delete() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, _a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, folder) = s1
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .unwrap();

    // Select the folder: new tabs file into it even though the active tab is loose.
    let s3 = s2.with_selected_tab_folder(ws, Some(folder.id)).unwrap();
    let (s4, b) = s3.with_new_tab(ws, "https://b.example/").unwrap();
    assert_eq!(s4.tab(b.id).unwrap().folder_id, Some(folder.id));
    assert_clean(&s4);

    // Deleting the selected folder clears the selection (no dangling target).
    let s5 = s4.with_tab_folder_removed(ws, folder.id).unwrap();
    assert_eq!(s5.workspaces[0].selected_folder_id, None);
    assert_clean(&s5);

    // With no selection, a new tab is loose again.
    let (s6, c) = s5.with_new_tab(ws, "https://c.example/").unwrap();
    assert_eq!(s6.tab(c.id).unwrap().folder_id, None);
    assert_clean(&s6);
}

#[test]
fn loose_new_tab_clears_the_selected_folder() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, folder) = s0
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .unwrap();
    let s2 = s1.with_selected_tab_folder(ws, Some(folder.id)).unwrap();

    // `with_new_tab` honours the selection; `with_new_loose_tab` never does.
    let (s3, filed) = s2.with_new_tab(ws, "https://a.example/").unwrap();
    assert_eq!(s3.tab(filed.id).unwrap().folder_id, Some(folder.id));
    let (s4, loose) = s3.with_new_loose_tab(ws, "https://b.example/").unwrap();
    assert_eq!(s4.tab(loose.id).unwrap().folder_id, None);
    // The new loose tab is focused, so no folder stays highlighted.
    assert_eq!(s4.workspaces[0].active_tab_id, Some(loose.id));
    assert_eq!(s4.workspaces[0].selected_folder_id, None);
    assert_clean(&s4);
}

#[test]
fn folder_selection_follows_the_focused_tab() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, loose) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, folder) = s1
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .unwrap();
    let (s3, filed) = s2.with_new_tab(ws, "https://b.example/").unwrap();
    let s3 = s3
        .with_tab_assigned_folder(filed.id, Some(folder.id))
        .unwrap();

    // Focusing a foldered tab selects its folder; focusing a loose tab clears it.
    let s4 = s3.with_active_tab(filed.id).unwrap();
    assert_eq!(s4.workspaces[0].selected_folder_id, Some(folder.id));
    let s5 = s4.with_active_tab(loose.id).unwrap();
    assert_eq!(s5.workspaces[0].selected_folder_id, None);
    assert_clean(&s5);

    // Deselecting while a foldered tab is active makes the next tab loose.
    let s6 = s5
        .with_active_tab(filed.id)
        .unwrap()
        .with_selected_tab_folder(ws, None)
        .unwrap();
    let (s7, c) = s6.with_new_tab(ws, "https://c.example/").unwrap();
    assert_eq!(s7.tab(c.id).unwrap().folder_id, None);
    assert_clean(&s7);
}

#[test]
fn closing_the_active_tab_selects_its_successors_folder() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, folder) = s1
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .unwrap();
    let s3 = s2.with_tab_assigned_folder(a.id, Some(folder.id)).unwrap();
    let (s4, b) = s3.with_new_loose_tab(ws, "https://b.example/").unwrap();
    let s4 = s4.with_selected_tab_folder(ws, None).unwrap();
    assert_eq!(s4.workspaces[0].active_tab_id, Some(b.id));

    // Closing the active loose tab lands on `a` and highlights its folder.
    let s5 = s4.with_tab_closed(b.id).unwrap();
    assert_eq!(s5.workspaces[0].active_tab_id, Some(a.id));
    assert_eq!(s5.workspaces[0].selected_folder_id, Some(folder.id));
    assert_clean(&s5);

    // Closing a background tab leaves the selection alone.
    let (s6, c) = s5
        .with_new_background_tab(ws, "https://c.example/")
        .unwrap();
    let s7 = s6.with_selected_tab_folder(ws, None).unwrap();
    let s8 = s7.with_tab_closed(c.id).unwrap();
    assert_eq!(s8.workspaces[0].active_tab_id, Some(a.id));
    assert_eq!(s8.workspaces[0].selected_folder_id, None);
    assert_clean(&s8);
}

#[test]
fn filing_pinning_and_pane_focus_move_the_selection() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, b) = s1.with_new_tab(ws, "https://b.example/").unwrap();
    let (s3, folder) = s2
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .unwrap();
    assert_eq!(s3.workspaces[0].active_tab_id, Some(b.id));

    // Filing a background tab leaves the selection; filing the active tab moves it.
    let s4 = s3.with_tab_assigned_folder(a.id, Some(folder.id)).unwrap();
    assert_eq!(s4.workspaces[0].selected_folder_id, None);
    let s5 = s4.with_tab_assigned_folder(b.id, Some(folder.id)).unwrap();
    assert_eq!(s5.workspaces[0].selected_folder_id, Some(folder.id));

    // Pinning the active tab makes it loose, so the selection clears.
    let (s6, _) = s5.with_tab_pinned(b.id, true).unwrap();
    assert_eq!(s6.workspaces[0].selected_folder_id, None);
    assert_clean(&s6);

    // Pane focus changes follow the newly focused leaf.
    let s7 = s6
        .with_pane_split_with(ws, b.id, SplitDir::Horizontal, false, a.id)
        .unwrap();
    assert_eq!(s7.workspaces[0].active_tab_id, Some(a.id));
    assert_eq!(s7.workspaces[0].selected_folder_id, Some(folder.id));
    let s8 = s7.with_pane_leaf_replaced(ws, a.id, b.id).unwrap();
    assert_eq!(s8.workspaces[0].active_tab_id, Some(b.id));
    assert_eq!(s8.workspaces[0].selected_folder_id, None);
    assert_clean(&s8);
}

#[test]
fn link_opens_land_beside_their_opener_in_link_order() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, b) = s1.with_new_tab(ws, "https://b.example/").unwrap();
    let s3 = s2.with_active_tab(a.id).unwrap();

    // Two background opens from `a` sit right after it, in the order opened.
    let (s4, l1) = s3
        .with_new_background_tab(ws, "https://l1.example/")
        .unwrap();
    let (s5, l2) = s4
        .with_new_background_tab(ws, "https://l2.example/")
        .unwrap();
    assert_eq!(s5.workspaces[0].tab_order, vec![a.id, l1.id, l2.id, b.id]);
    assert_eq!(s5.workspaces[0].active_tab_id, Some(a.id));
    assert_clean(&s5);

    // A foreground open lands after that run and takes focus; the next follows it.
    let (s6, f1) = s5
        .with_new_related_tab(ws, "https://f1.example/", true)
        .unwrap();
    let (s7, f2) = s6
        .with_new_related_tab(ws, "https://f2.example/", true)
        .unwrap();
    assert_eq!(
        s7.workspaces[0].tab_order,
        vec![a.id, l1.id, l2.id, f1.id, f2.id, b.id]
    );
    assert_eq!(s7.workspaces[0].active_tab_id, Some(f2.id));

    // Re-focusing `a` starts a fresh run right behind it; Cmd+T still appends.
    let s8 = s7.with_active_tab(a.id).unwrap();
    let (s9, l3) = s8
        .with_new_background_tab(ws, "https://l3.example/")
        .unwrap();
    assert_eq!(s9.workspaces[0].tab_order[..2], [a.id, l3.id]);
    let (s10, t) = s9.with_new_tab(ws, "https://t.example/").unwrap();
    assert_eq!(s10.workspaces[0].tab_order.last(), Some(&t.id));
    assert_clean(&s10);

    // A folder selected while a loose tab is active files the open at the folder's end.
    let (s11, folder) = s10
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .unwrap();
    let s12 = s11.with_selected_tab_folder(ws, Some(folder.id)).unwrap();
    let (s13, filed) = s12
        .with_new_background_tab(ws, "https://filed.example/")
        .unwrap();
    assert_eq!(s13.tab(filed.id).unwrap().folder_id, Some(folder.id));
    assert_eq!(s13.workspaces[0].tab_order.last(), Some(&filed.id));
    assert_clean(&s13);
}

#[test]
fn selecting_a_folder_from_another_workspace_is_rejected() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, _ws2) = s0.with_new_workspace("Work".into(), AccentColor::Mint);
    let ws2 = s1.workspaces[1].id;
    let (s2, folder2) = s1
        .with_new_tab_folder(ws2, "Other".into(), AccentColor::Sun)
        .unwrap();
    let err = s2.with_selected_tab_folder(ws, Some(folder2.id));
    assert!(matches!(err, Err(WisError::InvalidInput { .. })));
}

#[test]
fn assigning_a_folder_from_another_workspace_is_rejected() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, _ws2) = s0.with_new_workspace("Work".into(), AccentColor::Mint);
    let ws2 = s1.workspaces[1].id;
    let (s2, a) = s1.with_new_tab(ws, "https://a.example/").unwrap();
    let (s3, folder2) = s2
        .with_new_tab_folder(ws2, "Other".into(), AccentColor::Sun)
        .unwrap();

    let err = s3.with_tab_assigned_folder(a.id, Some(folder2.id));
    assert!(matches!(err, Err(WisError::InvalidInput { .. })));
}

#[test]
fn activating_a_tab_opens_the_folder_that_was_hiding_it() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, inside) = s0.with_new_tab(ws, "https://inside.example/").unwrap();
    let (s2, outside) = s1.with_new_tab(ws, "https://outside.example/").unwrap();
    let (s3, folder) = s2
        .with_new_tab_folder(ws, "Work".to_string(), AccentColor::Azur)
        .unwrap();
    let s4 = s3
        .with_tab_assigned_folder(inside.id, Some(folder.id))
        .unwrap()
        .with_active_tab(outside.id)
        .unwrap()
        .with_tab_folder_collapsed(ws, folder.id, true)
        .unwrap();
    assert!(
        s4.workspaces[0].tab_folders[0].collapsed,
        "the folder starts collapsed"
    );

    let s5 = s4.with_active_tab(inside.id).unwrap();

    assert!(
        !s5.workspaces[0].tab_folders[0].collapsed,
        "activating a tab inside it must open the folder"
    );
    assert_clean(&s5);
}

#[test]
fn activating_a_loose_tab_leaves_folders_as_they_were() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, loose) = s0.with_new_tab(ws, "https://loose.example/").unwrap();
    let (s2, folder) = s1
        .with_new_tab_folder(ws, "Work".to_string(), AccentColor::Azur)
        .unwrap();
    let s3 = s2.with_tab_folder_collapsed(ws, folder.id, true).unwrap();

    let s4 = s3.with_active_tab(loose.id).unwrap();

    assert!(
        s4.workspaces[0].tab_folders[0].collapsed,
        "a tab outside the folder must not open it"
    );
    assert_clean(&s4);
}
