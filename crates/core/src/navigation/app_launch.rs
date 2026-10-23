//! Custom-scheme "open in app" classification.
//!
//! The embedded engine can only render web schemes. This module decides
//! which schemes qualify and builds the permission key remembered decisions
//! are stored under.

/// Schemes the engine renders or resolves itself.
const ENGINE_SCHEMES: &[&str] = &["http", "https", "about", "blob", "data", "javascript"];

/// Schemes never handed to the OS: local files and app-internal surfaces,
/// plus OS handlers with a history of one-click exploitation: Follina-class
/// diagnostic/installer handlers, Explorer search spoofing, and network-share
/// mounts that leak credentials.
const BLOCKED_SCHEMES: &[&str] = &[
    "file",
    "tauri",
    "asset",
    "ipc",
    "wry",
    "wis",
    "wis-canvas",
    "vbscript",
    "ms-msdt",
    "ms-officecmd",
    "ms-appinstaller",
    "search-ms",
    "search",
    "smb",
    "afp",
    "nfs",
    "cifs",
    "shell",
];

/// True when a navigation to `scheme` should be cancelled in the engine and
/// offered to the user as an app handoff instead. Expects the lowercase
/// scheme a parsed `Url` yields.
pub fn is_app_scheme(scheme: &str) -> bool {
    !scheme.is_empty() && !ENGINE_SCHEMES.contains(&scheme) && !BLOCKED_SCHEMES.contains(&scheme)
}

/// Key a remembered launch decision is stored under in
/// `UserSettings::app_launch_permissions`: `"<scheme>|<initiator host>"`,
/// which is the site that triggered the launch.
pub fn permission_key(scheme: &str, initiator_host: &str) -> String {
    format!("{scheme}|{initiator_host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_schemes_are_not_app_schemes() {
        for scheme in ["http", "https", "about", "blob", "data", "javascript"] {
            assert!(!is_app_scheme(scheme), "{scheme} must stay in the engine");
        }
    }

    #[test]
    fn blocked_schemes_are_not_app_schemes() {
        let blocked = [
            "file",
            "tauri",
            "asset",
            "ipc",
            "wry",
            "wis",
            "vbscript",
            "ms-msdt",
            "ms-officecmd",
            "ms-appinstaller",
            "search-ms",
            "search",
            "smb",
            "afp",
            "nfs",
            "cifs",
            "shell",
            "",
        ];
        for scheme in blocked {
            assert!(!is_app_scheme(scheme), "{scheme:?} must never launch");
        }
    }

    #[test]
    fn app_schemes_are_intercepted() {
        for scheme in ["zoommtg", "slack", "mailto", "spotify", "vscode", "tel"] {
            assert!(
                is_app_scheme(scheme),
                "{scheme} should offer an app handoff"
            );
        }
    }

    #[test]
    fn permission_key_is_scheme_pipe_host() {
        assert_eq!(permission_key("zoommtg", "zoom.us"), "zoommtg|zoom.us");
        assert_eq!(permission_key("slack", ""), "slack|");
    }
}
