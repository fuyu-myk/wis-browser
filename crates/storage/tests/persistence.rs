//! End-to-end tests for the SQLite storage layer. Every test uses an
//! in-memory database (`:memory:`) so they're hermetic and fast.

use wis_core::{
    AccentColor, BrowserState, EssentialPin, LayoutMode, PrivacyMode, TabId, ThemeMode,
    UpstreamProxy, UserSettingsPatch,
};
use wis_storage::{SqliteStorage, Storage};

#[test]
fn tab_sessions_round_trip_survive_state_save_and_purge() {
    let store = fresh_store();
    let a = TabId::new();
    let b = TabId::new();

    assert!(store.load_tab_sessions().expect("load").is_empty());

    store.save_tab_session(a, "blob-a").expect("save a");
    store.save_tab_session(b, "blob-b").expect("save b");

    let loaded = store.load_tab_sessions().expect("load");
    assert_eq!(loaded.get(&a).map(String::as_str), Some("blob-a"));
    assert_eq!(loaded.get(&b).map(String::as_str), Some("blob-b"));

    // Upsert replaces the prior blob for the same tab.
    store.save_tab_session(a, "blob-a2").expect("re-save a");
    assert_eq!(
        store
            .load_tab_sessions()
            .expect("load")
            .get(&a)
            .map(String::as_str),
        Some("blob-a2")
    );

    // A full state save lives in its own tables; tab sessions must survive it.
    store
        .save_state(&BrowserState::new_with_defaults())
        .expect("save state");
    assert_eq!(store.load_tab_sessions().expect("load").len(), 2);

    // Purge keeps only the still-present tabs.
    store.purge_tab_sessions_except(&[a]).expect("purge");
    let after = store.load_tab_sessions().expect("load");
    assert_eq!(after.len(), 1);
    assert!(after.contains_key(&a));
    assert!(!after.contains_key(&b));

    // Explicit delete and empty-keep wipe.
    store.delete_tab_session(a).expect("delete a");
    assert!(store.load_tab_sessions().expect("load").is_empty());
    store.save_tab_session(b, "blob-b").expect("save b again");
    store.purge_tab_sessions_except(&[]).expect("purge all");
    assert!(store.load_tab_sessions().expect("load").is_empty());
}

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
fn meta_kv_round_trips_and_survives_state_save() {
    let store = fresh_store();
    assert_eq!(store.get_meta("window_geometry").expect("get"), None);

    store.set_meta("window_geometry", "{\"a\":1}").expect("set");
    assert_eq!(
        store.get_meta("window_geometry").expect("get").as_deref(),
        Some("{\"a\":1}")
    );

    // A full state save must not wipe app-managed meta keys.
    store
        .save_state(&BrowserState::new_with_defaults())
        .expect("save");
    assert_eq!(
        store.get_meta("window_geometry").expect("get").as_deref(),
        Some("{\"a\":1}")
    );

    store.set_meta("window_geometry", "{}").expect("overwrite");
    assert_eq!(
        store.get_meta("window_geometry").expect("get").as_deref(),
        Some("{}")
    );
}

#[test]
fn windowed_workspaces_round_trip_and_survive_state_save() {
    let store = fresh_store();
    let state = BrowserState::new_with_defaults();
    let ws = state.workspaces[0].id;

    // Empty until something is saved.
    assert!(store.load_windowed_workspaces().expect("load").is_empty());

    store
        .save_windowed_workspaces(&[ws])
        .expect("save windowed");
    assert_eq!(store.load_windowed_workspaces().expect("load"), vec![ws]);

    // A full state save (which clears/rewrites the document tables) must not
    // wipe the separately-owned windowed-layout meta key.
    store.save_state(&state).expect("save state");
    assert_eq!(store.load_windowed_workspaces().expect("load"), vec![ws]);

    // Re-saving replaces the prior list.
    store.save_windowed_workspaces(&[]).expect("clear windowed");
    assert!(store.load_windowed_workspaces().expect("load").is_empty());
}

#[test]
fn workspace_background_survives_round_trip() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let background = wis_core::Background::Gradient(wis_core::GradientSpec {
        angle: 160,
        opacity: 100,
        stops: vec![
            wis_core::GradientStop {
                color: "#efe7ff".into(),
                position: 0,
            },
            wis_core::GradientStop {
                color: "#f7f6f3".into(),
                position: 100,
            },
        ],
    });
    let workspace = s0.workspaces[0].with_background(Some(background.clone()));
    let state = BrowserState {
        workspaces: vec![workspace],
        ..s0
    };

    store.save_state(&state).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    assert_eq!(loaded.workspaces[0].background, Some(background.clone()));
    let wis_core::Background::Gradient(g) = background else {
        unreachable!()
    };
    assert_eq!(
        loaded.workspaces[0].background_drafts.gradient,
        Some(g),
        "with_background must remember the gradient draft"
    );
}

#[test]
fn a_workspace_s_lockdown_switch_survives_round_trip() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let id = s0.workspaces[0].id;
    let state = s0.with_workspace_lockdown(id, true).unwrap();

    store.save_state(&state).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    assert!(loaded.workspaces[0].lockdown);
}

#[test]
fn tab_folders_and_membership_survive_round_trip() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").expect("tab a");
    let (s2, _b) = s1.with_new_tab(ws, "https://b.example/").expect("tab b");
    let (s3, folder) = s2
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .expect("folder");
    let s4 = s3
        .with_tab_assigned_folder(a.id, Some(folder.id))
        .expect("assign")
        .with_tab_folder_collapsed(ws, folder.id, true)
        .expect("collapse");

    store.save_state(&s4).expect("save");
    let loaded = store.load_state().expect("load").unwrap();

    let lw = &loaded.workspaces[0];
    assert_eq!(lw.tab_folders.len(), 1);
    assert_eq!(lw.tab_folders[0].id, folder.id);
    assert_eq!(lw.tab_folders[0].name, "Work");
    assert!(lw.tab_folders[0].collapsed);
    assert_eq!(
        loaded.tab(a.id).expect("tab a present").folder_id,
        Some(folder.id)
    );
    assert!(loaded.invariant_violations().is_empty());
}

#[test]
fn folder_selection_is_derived_from_the_active_tab_on_load() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").expect("tab a");
    let (s2, folder) = s1
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Mint)
        .expect("folder");
    let s3 = s2
        .with_tab_assigned_folder(a.id, Some(folder.id))
        .expect("assign")
        .with_selected_tab_folder(ws, None)
        .expect("deselect");

    store.save_state(&s3).expect("save");
    let loaded = store.load_state().expect("load").unwrap();

    assert_eq!(loaded.workspaces[0].active_tab_id, Some(a.id));
    assert_eq!(loaded.workspaces[0].selected_folder_id, Some(folder.id));
    assert!(loaded.invariant_violations().is_empty());
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
        store
            .load_session_cookies(ws, ttl)
            .expect("load")
            .as_deref(),
        Some("blob-1"),
    );

    // Saving again replaces the data (one row per workspace).
    store.save_session_cookies(ws, "blob-2").expect("save 2");
    assert_eq!(
        store
            .load_session_cookies(ws, ttl)
            .expect("load")
            .as_deref(),
        Some("blob-2")
    );

    // A zero-day TTL treats everything as expired (nothing loads, purge clears).
    assert_eq!(store.load_session_cookies(ws, 0).expect("load ttl0"), None);
    store.purge_stale_session_cookies(0).expect("purge all");
    assert_eq!(
        store.load_session_cookies(ws, ttl).expect("after purge"),
        None
    );

    // Scoped per workspace.
    store.save_session_cookies(ws, "blob-3").expect("save 3");
    assert_eq!(
        store
            .load_session_cookies(WorkspaceId::new(), ttl)
            .expect("other"),
        None
    );
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
fn tab_reader_flag_is_session_only_and_resets_on_round_trip() {
    let store = fresh_store();
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, t1) = s0.with_new_tab(ws_id, "https://a.example/").unwrap();
    let s2 = s1.with_tab_reader(t1.id, true).unwrap();
    assert!(
        s2.tabs.iter().find(|t| t.id == t1.id).unwrap().reader,
        "reader flag is set in memory"
    );

    store.save_state(&s2).expect("save");
    let loaded = store.load_state().expect("load").unwrap();
    let tab = loaded.tabs.iter().find(|t| t.id == t1.id).unwrap();
    assert!(
        !tab.reader,
        "reader is in-memory only; the store reconstructs it as false"
    );
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
