//! End-to-end tests for the SQLite storage layer. Every test uses an
//! in-memory database (`:memory:`) so they're hermetic and fast.

use wis_core::{
    AccentColor, BrowserState, EssentialPin, LayoutMode, PrivacyMode, ThemeMode, UpstreamProxy,
    UserSettingsPatch,
};
use wis_storage::{SqliteStorage, Storage};

#[test]
fn upstream_proxy_survives_round_trip() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let workspace = s0.workspaces[0].with_upstream_proxy(Some(UpstreamProxy::Socks5 {
        host: "127.0.0.1".into(),
        port: 9050,
    }));
    let state = BrowserState {
        workspaces: vec![workspace],
        ..s0
    };

    store.save_state(&state).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    assert_eq!(
        loaded.workspaces[0].upstream_proxy,
        Some(UpstreamProxy::Socks5 {
            host: "127.0.0.1".into(),
            port: 9050,
        })
    );
}

fn fresh_store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("open in-memory DB")
}

#[test]
fn session_cookies_round_trip_overwrite_and_ttl() {
    use wis_core::WorkspaceId;
    let store = fresh_store();
    let ws = WorkspaceId::new();
    let ttl = 14;

    assert_eq!(store.load_session_cookies(ws, ttl).expect("load"), None);

    store.save_session_cookies(ws, "blob-1").expect("save");
    assert_eq!(
        store.load_session_cookies(ws, ttl).expect("load").as_deref(),
        Some("blob-1"),
    );

    // Saving again replaces the data (one row per workspace).
    store.save_session_cookies(ws, "blob-2").expect("save 2");
    assert_eq!(store.load_session_cookies(ws, ttl).expect("load").as_deref(), Some("blob-2"));

    // A zero-day TTL treats everything as expired (nothing loads, purge clears).
    assert_eq!(store.load_session_cookies(ws, 0).expect("load ttl0"), None);
    store.purge_stale_session_cookies(0).expect("purge all");
    assert_eq!(store.load_session_cookies(ws, ttl).expect("after purge"), None);

    // Scoped per workspace.
    store.save_session_cookies(ws, "blob-3").expect("save 3");
    assert_eq!(store.load_session_cookies(WorkspaceId::new(), ttl).expect("other"), None);
}

#[test]
fn loading_an_empty_store_returns_none() {
    let store = fresh_store();
    let loaded = store.load_state().expect("load_state");
    assert!(loaded.is_none(), "fresh store must be empty");
}

#[test]
fn default_state_round_trips() {
    let store = fresh_store();
    let state = BrowserState::new_with_defaults();
    store.save_state(&state).expect("save_state");

    let loaded = store
        .load_state()
        .expect("load_state")
        .expect("state must be present after saving");
    assert_eq!(loaded, state);
}

#[test]
fn populated_state_round_trips() {
    let store = fresh_store();

    let s0 = BrowserState::new_with_defaults();
    let ws_a_id = s0.workspaces[0].id;
    let (s1, ws_b) = s0.with_new_workspace("Work".into(), AccentColor::Mint);
    let (s2, t1) = s1.with_new_tab(ws_a_id, "https://example.com/a").unwrap();
    let (s3, t2) = s2.with_new_tab(ws_a_id, "https://example.com/b").unwrap();
    let (s4, _t3) = s3.with_new_tab(ws_b.id, "https://b.example/").unwrap();
    let s5 = s4.with_active_tab(t2.id).unwrap();
    let (s6, _) = s5.with_tab_pinned(t1.id, true).unwrap();
    let s7 = s6.with_settings(UserSettingsPatch {
        layout: Some(LayoutMode::TopTabs),
        theme: Some(ThemeMode::Dark),
        glass_enabled: Some(false),
        default_privacy_mode: Some(PrivacyMode::Strict),
        ..Default::default()
    });

    store.save_state(&s7).expect("save_state");
    let loaded = store.load_state().expect("load_state").unwrap();

    assert_eq!(loaded, s7);
}

#[test]
fn essentials_survive_round_trip() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let mut workspace = s0.workspaces[0].clone();
    workspace.essentials = vec![
        EssentialPin {
            slot: 0,
            label: "Mail".into(),
            url: "https://mail.example/".into(),
            icon: Some("mail".into()),
        },
        EssentialPin {
            slot: 2,
            label: "Calendar".into(),
            url: "https://calendar.example/".into(),
            icon: None,
        },
    ];
    let state = BrowserState {
        workspaces: vec![workspace],
        ..s0
    };

    store.save_state(&state).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    assert_eq!(loaded.workspaces[0].essentials.len(), 2);
    assert_eq!(
        loaded.workspaces[0].essentials,
        state.workspaces[0].essentials
    );
}

#[test]
fn tab_ordering_is_preserved_across_round_trips() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://a.example/").unwrap();
    let (s2, t2) = s1.with_new_tab(ws_id, "https://b.example/").unwrap();
    let (s3, t3) = s2.with_new_tab(ws_id, "https://c.example/").unwrap();

    // Reverse the order.
    let s4 = s3.with_tab_order(ws_id, vec![t3.id, t2.id, t1.id]).unwrap();

    store.save_state(&s4).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    assert_eq!(loaded.workspaces[0].tab_order, vec![t3.id, t2.id, t1.id]);
}

#[test]
fn saving_again_replaces_prior_state() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, _) = s0.with_new_tab(ws_id, "https://first.example/").unwrap();
    store.save_state(&s1).expect("save first");

    // Save a state with no tabs in any workspace; the prior tab must be gone.
    let s2 = BrowserState::new_with_defaults();
    store.save_state(&s2).expect("save second");

    let loaded = store.load_state().expect("load").unwrap();
    assert_eq!(loaded.tabs.len(), 0, "previous tabs must be cleared");
    assert_eq!(loaded.workspaces.len(), 1);
}

#[test]
fn workspace_ordering_is_preserved() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let (s1, _) = s0.with_new_workspace("Beta".into(), AccentColor::Mint);
    let (s2, _) = s1.with_new_workspace("Gamma".into(), AccentColor::Rose);
    let (s3, _) = s2.with_new_workspace("Delta".into(), AccentColor::Sun);

    store.save_state(&s3).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    let names: Vec<_> = loaded.workspaces.iter().map(|w| w.name.clone()).collect();
    assert_eq!(names, vec!["Personal", "Beta", "Gamma", "Delta"]);
}

#[test]
fn active_workspace_is_preserved() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let (s1, ws_b) = s0.with_new_workspace("B".into(), AccentColor::Sun);
    let s2 = s1.with_active_workspace(ws_b.id).unwrap();

    store.save_state(&s2).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    assert_eq!(loaded.active_workspace_id, ws_b.id);
}

#[test]
fn saving_state_with_a_dangling_tab_id_is_rejected() {
    // Workspaces' tab_order is iterated against state.tabs when saving;
    // a dangling reference must surface as DataCorruption rather than
    // silently writing inconsistent rows.
    let store = fresh_store();
    let mut state = BrowserState::new_with_defaults();
    let ghost_tab_id = wis_core::TabId::new();
    state.workspaces[0].tab_order.push(ghost_tab_id);

    let result = store.save_state(&state);
    assert!(
        matches!(result, Err(wis_storage::StorageError::DataCorruption(_))),
        "expected DataCorruption, got {result:?}"
    );
}

#[test]
fn pane_layout_round_trips() {
    use wis_core::SplitDir;

    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, _t1) = s0.with_new_tab(ws_id, "https://a.example/").unwrap();
    // Split the active pane: opens a second tab side-by-side.
    let (s2, _t2) = s1
        .with_pane_split(ws_id, SplitDir::Horizontal, false, "https://b.example/")
        .unwrap();

    assert!(
        s2.workspaces[0].pane_tree.is_some(),
        "precondition: split set"
    );

    store.save_state(&s2).expect("save");
    let loaded = store.load_state().expect("load").unwrap();

    assert_eq!(loaded.workspaces[0].pane_tree, s2.workspaces[0].pane_tree);
    assert_eq!(loaded, s2);
}

#[test]
fn ephemeral_workspaces_are_not_persisted() {
    // A private (ephemeral) workspace and its tabs must never hit disk.
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let persistent_id = s0.workspaces[0].id;
    let (s1, private_ws) = s0.with_new_private_workspace("Private".into(), AccentColor::Ink);
    let (s2, _t) = s1
        .with_new_tab(private_ws.id, "https://secret.example/")
        .unwrap();

    store.save_state(&s2).expect("save");
    let loaded = store.load_state().expect("load").unwrap();

    assert_eq!(
        loaded.workspaces.len(),
        1,
        "private workspace must be dropped"
    );
    assert_eq!(loaded.workspaces[0].id, persistent_id);
    assert!(
        loaded.tabs.iter().all(|t| t.workspace_id == persistent_id),
        "no tabs from the private workspace may survive"
    );
    assert!(!loaded.workspaces[0].ephemeral);
}

#[test]
fn active_pointer_falls_back_when_active_workspace_is_ephemeral() {
    // If the live active workspace is private, hydration must fall back to a
    // persisted workspace rather than producing an invalid state.
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let persistent_id = s0.workspaces[0].id;
    let (s1, private_ws) = s0.with_new_private_workspace("Private".into(), AccentColor::Ink);
    let s2 = s1.with_active_workspace(private_ws.id).unwrap();

    store.save_state(&s2).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    assert_eq!(loaded.active_workspace_id, persistent_id);
}

#[test]
fn settings_default_to_userdefaults_when_meta_row_missing() {
    // Even after an empty save, settings come back as `UserSettings::default()`
    // rather than panicking.
    let store = fresh_store();
    let state = BrowserState::new_with_defaults();
    store.save_state(&state).unwrap();
    let loaded = store.load_state().unwrap().unwrap();
    assert_eq!(loaded.settings, wis_core::UserSettings::default());
}
