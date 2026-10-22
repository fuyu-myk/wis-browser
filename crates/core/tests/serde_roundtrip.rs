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
fn widget_kind_roundtrips_as_snake_case() {
    let json = serde_json::to_string(&wis_core::WidgetKind::Perplexity).unwrap();
    assert_eq!(json, "\"perplexity\"");
    let parsed: wis_core::WidgetKind = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, wis_core::WidgetKind::Perplexity);
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
        password_offer: Some(wis_core::PasswordOffer::Always),
        layout: Some(LayoutMode::TopTabs),
        top_tabs_rail_side: Some(wis_core::RailSide::Right),
        top_tabs_max_expanded_folders: Some(3),
        theme: Some(ThemeMode::Light),
        accent: Some(wis_core::AccentColor::Mint),
        glass_enabled: Some(true),
        sidebar_collapsible: Some(false),
        compact: Some(true),
        default_privacy_mode: Some(PrivacyMode::Strict),
        doh_resolver: Some("https://dns.example/dns-query".into()),
        homepage: Some("https://start.example".into()),
        new_tab_target: Some(wis_core::NewTabTarget::Homepage),
        shortcuts: Some(vec![wis_core::ShortcutTile {
            label: "Example".into(),
            url: "https://example.com/".into(),
        }]),
        search_engine: Some(wis_core::SearchEngine::Google),
        custom_search_engines: Some(vec![wis_core::CustomSearchEngine {
            name: "Kagi".into(),
            keyword: "k".into(),
            template: "https://kagi.com/search?q={searchTerms}".into(),
            is_default: true,
        }]),
        builtin_search_keywords: Some(HashMap::from([(
            wis_core::SearchEngine::Google,
            "g".to_string(),
        )])),
        filter_list_auto_update: Some(false),
        url_bar_focus_glow: Some(true),
        always_private: Some(true),
        tab_sleep_enabled: Some(false),
        tab_sleep_minutes: Some(30),
        tab_archive_after: Some(wis_core::TabArchiveAfter::SevenDays),
        usage_tracking: Some(false),
        usage_idle_timeout_secs: Some(300),
        week_start: Some(wis_core::usage::WeekStart::Sunday),
        canvas_theme: Some(wis_core::canvas::CanvasTheme::Light),
        persist_sessions: Some(false),
        fraud_warning_enabled: Some(false),
        global_privacy_control: Some(false),
        autoplay_policy: Some(wis_core::AutoplayPolicy::Allow),
        default_zoom_percent: Some(110),
        zoom_site_overrides: Some(HashMap::from([("docs.example".to_string(), 125)])),
        autoplay_site_overrides: Some(HashMap::from([(
            "video.example".to_string(),
            wis_core::AutoplayPolicy::Block,
        )])),
        site_protection_overrides: Some(HashMap::from([(
            "news.example".to_string(),
            wis_core::SiteProtection {
                blocking: false,
                javascript: false,
                fingerprinting: false,
            },
        )])),
        media_extensions_enabled: Some(false),
        extension_store_search: Some(false),
        video_popout_enabled: Some(false),
        user_agent_profile: Some(wis_core::UaProfile::Chrome),
        ua_site_overrides: Some(HashMap::from([(
            "example.com".to_string(),
            wis_core::UaProfile::Safari,
        )])),
        media_permissions: Some(HashMap::from([(
            "camera|https://example.com".to_string(),
            true,
        )])),
        app_launch_permissions: Some(HashMap::from([("zoommtg|zoom.us".to_string(), true)])),
        sidebar_widgets: Some(vec![
            wis_core::Widget {
                id: wis_core::WidgetId::new(),
                kind: wis_core::WidgetKind::Claude,
                label: "Claude".to_string(),
                url: "https://claude.ai".to_string(),
                icon: None,
                enabled: true,
                group: None,
            },
            wis_core::Widget {
                id: wis_core::WidgetId::new(),
                kind: wis_core::WidgetKind::GoogleCalendar,
                label: "Calendar".into(),
                url: "https://calendar.google.com".into(),
                icon: None,
                enabled: true,
                group: None,
            },
            wis_core::Widget {
                id: wis_core::WidgetId::new(),
                kind: wis_core::WidgetKind::Custom,
                label: "My wiki".into(),
                url: "https://wiki.example.com".into(),
                icon: None,
                enabled: true,
                group: Some(wis_core::WidgetGroup::Productivity),
            },
        ]),
        widget_panel_limit_enabled: Some(false),
        widget_panel_limit: Some(5),
        widget_keep_loaded: Some(false),
        widget_sleep_minutes: Some(7),
        widget_memory_cap_enabled: Some(false),
        widget_memory_cap_mb: Some(800),
        filter_subscriptions: Some(vec!["https://example.test/list.txt".to_string()]),
        link_routes: Some(vec![wis_core::link_routing::LinkRoute {
            pattern: "github.com/acme".to_string(),
            workspace_id: wis_core::WorkspaceId::new(),
        }]),
        block_cookie_notices: Some(false),
        block_annoyances: Some(true),
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
        design: Some(wis_core::ChromeDesign {
            corners: wis_core::CornerStyle::Sharp,
            ..wis_core::ChromeDesign::default()
        }),
        ai: Some(wis_core::AiSettings {
            provider: wis_core::AiProvider::OpenAiCompatible,
            model: "gpt-x".into(),
            base_url: Some("http://localhost:11434/v1".into()),
            show_reasoning: true,
            web_access: true,
            local_data: true,
            tab_actions: true,
            canvas_model: Some("qwen3:8b".into()),
        }),
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
fn top_tabs_expanded_folder_limit_defaults_to_two() {
    let s = UserSettings::default();
    assert_eq!(s.top_tabs_max_expanded_folders, 2);
}

#[test]
fn top_tabs_expanded_folder_limit_patch_applies_and_zero_means_no_limit() {
    let base = UserSettings::default();
    let patched = base.with_patch(UserSettingsPatch {
        top_tabs_max_expanded_folders: Some(0),
        ..Default::default()
    });
    assert_eq!(patched.top_tabs_max_expanded_folders, 0);
    // Untouched fields keep their prior values.
    assert_eq!(patched.layout, base.layout);
    assert_eq!(patched.top_tabs_rail_side, base.top_tabs_rail_side);
}

#[test]
fn user_settings_without_expanded_folder_limit_deserialises_to_default() {
    // Settings persisted before the limit existed must still load as 2.
    let json = r#"{
        "layout": "top_tabs",
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
    assert_eq!(parsed.top_tabs_max_expanded_folders, 2);
}

#[test]
fn user_settings_without_week_start_deserialises_to_auto() {
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
    assert_eq!(parsed.week_start, wis_core::usage::WeekStart::Auto);
}

#[test]
fn ai_settings_without_local_data_keep_it_off() {
    let json = r#"{ "provider": "open_ai_compatible", "model": "qwen", "web_access": true }"#;
    let parsed: wis_core::AiSettings = serde_json::from_str(json).unwrap();
    assert!(parsed.web_access);
    assert!(!parsed.local_data);
    assert!(!parsed.tab_actions);
    assert!(!wis_core::AiSettings::default().local_data);
    assert!(!wis_core::AiSettings::default().tab_actions);
}

#[test]
fn week_start_roundtrips_as_snake_case() {
    use wis_core::usage::WeekStart;
    for (start, json) in [
        (WeekStart::Auto, "\"auto\""),
        (WeekStart::Monday, "\"monday\""),
        (WeekStart::Sunday, "\"sunday\""),
    ] {
        assert_eq!(serde_json::to_string(&start).unwrap(), json);
        assert_eq!(roundtrip(&start), start);
    }
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
fn user_settings_without_custom_engines_deserialises_to_builtin() {
    // Settings persisted before custom engines existed must still load and
    // resolve to the built-in choice.
    let json = r#"{
        "layout": "sidebar_left",
        "theme": "system",
        "accent": "primary",
        "glass_enabled": true,
        "sidebar_collapsible": true,
        "compact": false,
        "default_privacy_mode": "balanced",
        "doh_resolver": null,
        "homepage": null,
        "search_engine": "google"
    }"#;
    let parsed: UserSettings = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.search_engine, wis_core::SearchEngine::Google);
    assert!(parsed.custom_search_engines.is_empty());
    assert!(parsed.active_custom_engine().is_none());
    assert!(parsed.builtin_search_keywords.is_empty());
}

#[test]
fn unknown_builtin_keyword_engine_ids_are_dropped() {
    // A renamed/removed engine variant must not brick the settings blob.
    let json = r#"{
        "layout": "sidebar_left",
        "theme": "system",
        "accent": "primary",
        "glass_enabled": true,
        "sidebar_collapsible": true,
        "compact": false,
        "default_privacy_mode": "balanced",
        "doh_resolver": null,
        "homepage": null,
        "builtin_search_keywords": {"google": "g", "altavista": "av"}
    }"#;
    let parsed: UserSettings = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.builtin_search_keywords.len(), 1);
    assert_eq!(
        parsed.builtin_search_keywords[&wis_core::SearchEngine::Google],
        "g"
    );
}

#[test]
fn custom_search_engine_wire_format_is_stable() {
    // The TS mirror hand-maintains these field names; a rename here must fail.
    let engine = wis_core::CustomSearchEngine {
        name: "Kagi".into(),
        keyword: "k".into(),
        template: "https://kagi.com/search?q={searchTerms}".into(),
        is_default: true,
    };
    let json = serde_json::to_string(&engine).unwrap();
    for key in [
        "\"name\":",
        "\"keyword\":",
        "\"template\":",
        "\"is_default\":",
    ] {
        assert!(json.contains(key), "missing {key} in {json}");
    }
}

#[test]
fn custom_search_engine_without_is_default_deserialises_to_false() {
    // Engines persisted without the flag must load as non-default.
    let json =
        r#"{"name":"Kagi","keyword":"k","template":"https://kagi.com/search?q={searchTerms}"}"#;
    let parsed: wis_core::CustomSearchEngine = serde_json::from_str(json).unwrap();
    assert!(!parsed.is_default);
}

#[test]
fn deleted_default_engine_falls_back_to_builtin() {
    let with_default = UserSettings {
        search_engine: wis_core::SearchEngine::Bing,
        custom_search_engines: vec![wis_core::CustomSearchEngine {
            name: "Kagi".into(),
            keyword: "k".into(),
            template: "https://kagi.com/search?q={searchTerms}".into(),
            is_default: true,
        }],
        ..UserSettings::default()
    };
    assert_eq!(with_default.active_custom_engine().unwrap().name, "Kagi");

    let removed = with_default.with_patch(wis_core::UserSettingsPatch {
        custom_search_engines: Some(Vec::new()),
        ..Default::default()
    });
    assert!(removed.active_custom_engine().is_none());
    assert_eq!(removed.search_engine, wis_core::SearchEngine::Bing);
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
