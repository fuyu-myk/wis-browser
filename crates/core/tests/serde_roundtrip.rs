//! Integration tests for JSON serde round-tripping. Anything that crosses
//! the Tauri IPC boundary must survive serialise → deserialise unchanged.

use std::collections::HashMap;

use wis_core::{
    AccentColor, BrowserState, KeyAction, LayoutMode, PrivacyMode, ThemeMode, UserSettings,
    UserSettingsPatch,
};

fn roundtrip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let json = serde_json::to_string(value).expect("serialise");
    serde_json::from_str(&json).expect("deserialise")
}

#[test]
fn default_browser_state_roundtrips() {
    let before = BrowserState::new_with_defaults();
    let after: BrowserState = roundtrip(&before);
    assert_eq!(before, after);
}

#[test]
fn populated_browser_state_roundtrips() {
    let s0 = BrowserState::new_with_defaults();
    let ws_id = s0.workspaces[0].id;
    let (s1, _) = s0.with_new_workspace("Work".into(), AccentColor::Mint);
    let (s2, _) = s1
        .with_new_tab(ws_id, "https://example.com/path?q=1")
        .unwrap();
    let (s3, _) = s2
        .with_new_tab(ws_id, "https://anotherexample.org/")
        .unwrap();
    let s4 = s3.with_settings(UserSettingsPatch {
        layout: Some(LayoutMode::TopTabs),
        theme: Some(ThemeMode::Dark),
        compact: Some(true),
        default_privacy_mode: Some(PrivacyMode::Strict),
        ..Default::default()
    });

    let after: BrowserState = roundtrip(&s4);
    assert_eq!(s4, after);
}

#[test]
fn layout_mode_roundtrips_as_snake_case() {
    let json = serde_json::to_string(&LayoutMode::SidebarRight).unwrap();
    assert_eq!(json, "\"sidebar_right\"");
    let parsed: LayoutMode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, LayoutMode::SidebarRight);
}

#[test]
fn privacy_mode_roundtrips_as_snake_case() {
    for mode in [
        PrivacyMode::Strict,
        PrivacyMode::Balanced,
        PrivacyMode::Permissive,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: PrivacyMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mode);
    }
}

#[test]
fn settings_patch_omits_none_fields_when_serialised() {
    let patch = UserSettingsPatch {
        theme: Some(ThemeMode::Dark),
        ..Default::default()
    };
    let json = serde_json::to_string(&patch).unwrap();
    assert!(json.contains("\"theme\""), "explicit field must be present");
    assert!(!json.contains("\"layout\""), "unset field must be omitted");
    assert!(
        !json.contains("\"glass_enabled\""),
        "unset field must be omitted"
    );
}

#[test]
fn settings_patch_round_trips_when_all_fields_set() {
    let patch = UserSettingsPatch {
        layout: Some(LayoutMode::TopTabs),
        top_tabs_rail_side: Some(wis_core::RailSide::Right),
        theme: Some(ThemeMode::Light),
        accent: Some(wis_core::AccentColor::Mint),
        glass_enabled: Some(true),
        sidebar_collapsible: Some(false),
        compact: Some(true),
        default_privacy_mode: Some(PrivacyMode::Strict),
        doh_resolver: Some("https://dns.example/dns-query".into()),
        homepage: Some("https://start.example".into()),
        search_engine: Some(wis_core::SearchEngine::Google),
        filter_list_auto_update: Some(false),
        url_bar_focus_glow: Some(true),
        always_private: Some(true),
        tab_sleep_enabled: Some(false),
        tab_sleep_minutes: Some(30),
        persist_sessions: Some(false),
        fraud_warning_enabled: Some(false),
        media_extensions_enabled: Some(false),
        video_popout_enabled: Some(false),
        user_agent_profile: Some(wis_core::UaProfile::Chrome),
        ua_site_overrides: Some(HashMap::from([(
            "example.com".to_string(),
            wis_core::UaProfile::Safari,
        )])),
        sidebar_widgets: Some(vec![wis_core::Widget {
            id: wis_core::WidgetId::new(),
            kind: wis_core::WidgetKind::Claude,
            label: "Claude".to_string(),
            url: "https://claude.ai".to_string(),
            icon: None,
            enabled: true,
        }]),
        widget_panel_limit_enabled: Some(false),
        widget_panel_limit: Some(5),
        widget_keep_loaded: Some(false),
        widget_sleep_minutes: Some(7),
        widget_memory_cap_enabled: Some(false),
        widget_memory_cap_mb: Some(800),
        filter_subscriptions: Some(vec!["https://example.test/list.txt".to_string()]),
        user_cosmetic_rules: Some(std::collections::HashMap::from([(
            "example.test".to_string(),
            vec![".ad".to_string()],
        )])),
        userscripts: Some(vec![wis_core::userscript::Userscript::from_source(
            "// ==UserScript==\n// @name T\n// @match *\n// ==/UserScript==\n",
        )
        .unwrap()]),
        keybindings: Some(HashMap::from([
            (KeyAction::NewTab, "Ctrl+T".into()),
            (KeyAction::CloseTab, "Ctrl+W".into()),
            (KeyAction::FindInPage, "Ctrl+F".into()),
            (KeyAction::FindNext, "Ctrl+G".into()),
            (KeyAction::FindPrevious, "Ctrl+Shift+G".into()),
        ])),
        download_ask_where: Some(true),
        download_last_dir: Some("/Users/someone/Desktop".into()),
        custom_accent_palette: Some(vec!["#aabbcc".into()]),
        custom_themes: Some(Vec::new()),
        motion: Some(wis_core::MotionLevel::Reduced),
    };
    let after: UserSettingsPatch = roundtrip(&patch);
    assert_eq!(patch, after);
}

#[test]
fn user_settings_roundtrips() {
    let before = UserSettings::default();
    let after: UserSettings = roundtrip(&before);
    assert_eq!(before, after);
}

#[test]
fn tab_sleep_defaults_are_enabled_at_fifteen_minutes() {
    let s = UserSettings::default();
    assert!(s.tab_sleep_enabled);
    assert_eq!(s.tab_sleep_minutes, 15);
}

#[test]
fn tab_sleep_patch_applies_and_leaves_others_untouched() {
    let base = UserSettings::default();
    let patched = base.with_patch(UserSettingsPatch {
        tab_sleep_enabled: Some(false),
        tab_sleep_minutes: Some(45),
        ..Default::default()
    });
    assert!(!patched.tab_sleep_enabled);
    assert_eq!(patched.tab_sleep_minutes, 45);
    // Untouched fields keep their prior values.
    assert_eq!(patched.layout, base.layout);
    assert_eq!(patched.theme, base.theme);
}

#[test]
fn user_settings_without_tab_sleep_fields_deserialises_to_defaults() {
    // Settings persisted before tab-sleep existed must still load.
    let json = r#"{
        "layout": "sidebar_left",
        "theme": "system",
        "accent": "primary",
        "glass_enabled": true,
        "sidebar_collapsible": true,
        "compact": false,
        "default_privacy_mode": "balanced",
        "doh_resolver": null,
        "homepage": null
    }"#;
    let parsed: UserSettings = serde_json::from_str(json).unwrap();
    assert!(parsed.tab_sleep_enabled);
    assert_eq!(parsed.tab_sleep_minutes, 15);
}

#[test]
fn upstream_proxy_roundtrips_tagged() {
    use wis_core::UpstreamProxy;
    for variant in [
        UpstreamProxy::Socks5 {
            host: "127.0.0.1".into(),
            port: 9050,
        },
        UpstreamProxy::Tor,
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        let parsed: UpstreamProxy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, variant);
    }
    // Tagged by `kind`, snake_case.
    let json = serde_json::to_string(&UpstreamProxy::tor_default()).unwrap();
    assert!(json.contains("\"kind\":\"socks5\""), "got {json}");
    assert_eq!(
        serde_json::to_string(&UpstreamProxy::Tor).unwrap(),
        "{\"kind\":\"tor\"}"
    );
}

#[test]
fn search_engine_roundtrips_as_lowercase() {
    for (variant, expected) in [
        (wis_core::SearchEngine::DuckDuckGo, "\"duckduckgo\""),
        (wis_core::SearchEngine::Google, "\"google\""),
        (wis_core::SearchEngine::Bing, "\"bing\""),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected);
        let parsed: wis_core::SearchEngine = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, variant);
    }
}

#[test]
fn error_serialises_with_tag_and_payload() {
    let id = wis_core::WorkspaceId::new();
    let err = wis_core::WisError::WorkspaceNotFound { id };
    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains("\"code\":\"workspace_not_found\""));
    assert!(json.contains(&id.0.to_string()));
}

#[test]
fn tab_folders_roundtrip() {
    let s0 = BrowserState::new_with_defaults();
    let ws = s0.workspaces[0].id;
    let (s1, a) = s0.with_new_tab(ws, "https://a.example/").unwrap();
    let (s2, folder) = s1
        .with_new_tab_folder(ws, "Work".into(), AccentColor::Lila)
        .unwrap();
    let s3 = s2
        .with_tab_assigned_folder(a.id, Some(folder.id))
        .unwrap()
        .with_tab_folder_collapsed(ws, folder.id, true)
        .unwrap();

    let after: BrowserState = roundtrip(&s3);
    assert_eq!(s3, after);
}

#[test]
fn tab_without_folder_id_field_deserialises_as_loose() {
    // Old persisted tabs predate `folder_id`; serde-default keeps them loose.
    let json = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
        "workspace_id": "00000000-0000-0000-0000-000000000002",
        "url": "https://example.com/",
        "title": "Example",
        "favicon": null,
        "pinned": false,
        "muted": false,
        "zoom": 1.0,
        "created_at": "2024-01-01T00:00:00Z",
        "last_active_at": "2024-01-01T00:00:00Z"
    }"#;
    let tab: wis_core::Tab = serde_json::from_str(json).expect("deserialise legacy tab");
    assert_eq!(tab.folder_id, None);
}
