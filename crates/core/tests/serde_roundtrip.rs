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
        keybindings: Some(HashMap::from([
            (KeyAction::NewTab, "Ctrl+T".into()),
            (KeyAction::CloseTab, "Ctrl+W".into()),
            (KeyAction::FindInPage, "Ctrl+F".into()),
            (KeyAction::FindNext, "Ctrl+G".into()),
            (KeyAction::FindPrevious, "Ctrl+Shift+G".into()),
        ])),
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
    assert_eq!(serde_json::to_string(&UpstreamProxy::Tor).unwrap(), "{\"kind\":\"tor\"}");
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
