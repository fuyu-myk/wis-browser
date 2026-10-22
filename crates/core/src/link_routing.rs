//! Which workspace a link opened from another app goes to.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::ids::WorkspaceId;
use crate::state::BrowserState;
use crate::workspace::Workspace;

pub const MAX_LINK_ROUTES: usize = 200;
const MAX_PATTERN_LEN: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkRoute {
    /// A site with an optional path, as [`normalized_pattern`] writes it.
    pub pattern: String,
    pub workspace_id: WorkspaceId,
}

/// A site as a URL host without `www.`, with an optional path encoded as a
/// URL path and lowercased, trimmed of its query and trailing slash.
pub fn normalized_pattern(input: &str) -> Option<String> {
    let input = input.trim();
    if input.len() > 3 * MAX_PATTERN_LEN || input.contains(char::is_whitespace) {
        return None;
    }

    let rest = without_scheme(input);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (site, path) = split(rest);
    let site = site
        .strip_prefix("*.")
        .or_else(|| site.strip_prefix('.'))
        .unwrap_or(site);
    let host = url::Host::parse(site).ok()?.to_string();
    let mut host = host.trim_end_matches('.');
    while let Some(rest) = host.strip_prefix("www.") {
        host = rest;
    }
    if host.contains('*') || host.split('.').any(str::is_empty) {
        return None;
    }

    let path = url::Url::parse(&format!("https://{host}{path}"))
        .ok()?
        .path()
        .to_ascii_lowercase();
    let pattern = format!("{host}{}", path.trim_end_matches('/'));
    (pattern.len() <= MAX_PATTERN_LEN).then_some(pattern)
}

/// Rules with a valid pattern for a workspace that exists and outlives a
/// session. For a pattern given twice, the later one is kept.
pub fn sanitized_link_routes(routes: Vec<LinkRoute>, workspaces: &[Workspace]) -> Vec<LinkRoute> {
    let lasting: HashSet<WorkspaceId> = workspaces
        .iter()
        .filter(|workspace| !workspace.ephemeral)
        .map(|workspace| workspace.id)
        .collect();
    let mut kept: Vec<LinkRoute> = Vec::new();
    for route in routes {
        let Some(pattern) = normalized_pattern(&route.pattern) else {
            continue;
        };
        if !lasting.contains(&route.workspace_id) {
            continue;
        }

        kept.retain(|earlier| earlier.pattern != pattern);
        kept.push(LinkRoute {
            pattern,
            workspace_id: route.workspace_id,
        });
    }

    kept
}

impl BrowserState {
    /// The workspace of the most specific rule matching `url`.
    pub fn workspace_for_link(&self, url: &url::Url) -> Option<WorkspaceId> {
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }

        let host = url.host_str()?.trim_end_matches('.');
        let host = host.strip_prefix("www.").unwrap_or(host);
        let path = url.path().to_ascii_lowercase();
        self.settings
            .link_routes
            .iter()
            .filter(|route| covers(&route.pattern, host, &path))
            .filter(|route| {
                self.workspace(route.workspace_id)
                    .is_ok_and(|workspace| !workspace.ephemeral)
            })
            .max_by_key(|route| {
                let (site, path) = split(&route.pattern);
                (site.len(), path.len())
            })
            .map(|route| route.workspace_id)
    }
}

fn without_scheme(input: &str) -> &str {
    ["https://", "http://"]
        .into_iter()
        .find_map(|scheme| {
            input
                .get(..scheme.len())
                .filter(|head| head.eq_ignore_ascii_case(scheme))
                .map(|_| &input[scheme.len()..])
        })
        .unwrap_or(input)
}

fn split(pattern: &str) -> (&str, &str) {
    match pattern.find('/') {
        Some(slash) => pattern.split_at(slash),
        None => (pattern, ""),
    }
}

fn covers(pattern: &str, host: &str, path: &str) -> bool {
    let (site, prefix) = split(pattern);
    let on_site = host
        .strip_suffix(site)
        .is_some_and(|sub| sub.is_empty() || sub.ends_with('.'));
    let in_path = path
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
    on_site && in_path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::BrowserState;
    use crate::workspace::AccentColor;

    #[test]
    fn a_pattern_is_a_site_with_an_optional_path() {
        assert_eq!(
            normalized_pattern("Figma.com").as_deref(),
            Some("figma.com")
        );
        assert_eq!(
            normalized_pattern(" https://www.GitHub.com/Acme/ ").as_deref(),
            Some("github.com/acme")
        );
        assert_eq!(
            normalized_pattern("*.slack.com").as_deref(),
            Some("slack.com")
        );
        assert_eq!(
            normalized_pattern("docs.google.com/spreadsheets?x=1#top").as_deref(),
            Some("docs.google.com/spreadsheets")
        );
        for (written, stored) in [
            ("HTTPS://Figma.com", "figma.com"),
            ("bücher.de", "xn--bcher-kva.de"),
            ("figma.com.", "figma.com"),
            (".figma.com", "figma.com"),
            ("github.com/acme/../evil", "github.com/evil"),
            ("github.com/acme/.", "github.com/acme"),
            ("example.com/Café", "example.com/caf%c3%a9"),
            ("www.www.figma.com", "figma.com"),
        ] {
            assert_eq!(
                normalized_pattern(written).as_deref(),
                Some(stored),
                "{written}"
            );
            assert_eq!(
                normalized_pattern(stored).as_deref(),
                Some(stored),
                "{stored}"
            );
        }
        let too_long = format!("example.com/{}", "a".repeat(MAX_PATTERN_LEN));
        let too_long_encoded = format!("ja.wikipedia.org/wiki/{}", "日".repeat(100));
        for refused in [
            "",
            "   ",
            "not a site",
            "javascript:alert(1)",
            "ftp://figma.com",
            "/acme",
            "a b.com",
            "*",
            "www.*.figma.com",
            "figma..com",
            "user@figma.com",
            "example.com:8080",
            &too_long,
            &too_long_encoded,
        ] {
            assert_eq!(normalized_pattern(refused), None, "{refused}");
        }
    }

    fn routes_to(pattern: &str, url: &str) -> bool {
        let (mut state, personal, ..) = workspaces();
        let pattern = normalized_pattern(pattern).expect("a valid pattern");
        state.settings.link_routes = vec![route(&pattern, personal)];
        let url = url::Url::parse(url).expect("a valid url");
        state.workspace_for_link(&url) == Some(personal)
    }

    #[test]
    fn a_site_covers_its_subdomains_and_a_path_its_own_segments() {
        for (pattern, url) in [
            ("figma.com", "https://www.figma.com/file/1"),
            ("figma.com", "https://team.figma.com/"),
            ("figma.com", "http://figma.com:8080/x?y=1#z"),
            ("figma.com", "https://figma.com./"),
            ("figma.com", "https://evil.com@figma.com/"),
            ("bücher.de", "https://BÜCHER.de/"),
            ("github.com/acme", "https://github.com/Acme/repo"),
            ("github.com/acme", "https://github.com/acme"),
            ("github.com/acme", "https://github.com/x/../acme/y"),
            ("example.com/café", "https://example.com/caf%C3%A9/menu"),
            ("192.168.1.1", "http://192.168.1.1/admin"),
        ] {
            assert!(routes_to(pattern, url), "{pattern} {url}");
        }
        for (pattern, url) in [
            ("figma.com", "https://notfigma.com/"),
            ("figma.com", "https://evil-figma.com/"),
            ("figma.com", "https://figma.com.evil.com/"),
            ("figma.com", "https://figma.com@evil.com/"),
            ("figma.com", "file:///Users/me/figma.com"),
            ("github.com/acme", "https://github.com/acmecorp"),
            ("github.com/acme", "https://github.com/?acme"),
            ("github.com/acme", "https://github.com/#/acme"),
        ] {
            assert!(!routes_to(pattern, url), "{pattern} {url}");
        }
    }

    fn workspaces() -> (BrowserState, WorkspaceId, WorkspaceId, WorkspaceId) {
        let state = BrowserState::new_with_defaults();
        let personal = state.active_workspace_id;
        let (state, work) = state.with_new_workspace("Work".into(), AccentColor::Primary);
        let (state, private) =
            state.with_new_private_workspace("Private".into(), AccentColor::Primary);
        (state, personal, work.id, private.id)
    }

    fn route(pattern: &str, workspace_id: WorkspaceId) -> LinkRoute {
        LinkRoute {
            pattern: pattern.into(),
            workspace_id,
        }
    }

    fn link(state: &BrowserState, url: &str) -> Option<WorkspaceId> {
        state.workspace_for_link(&url::Url::parse(url).expect("a valid url"))
    }

    #[test]
    fn the_most_specific_rule_wins_whatever_the_order() {
        let (mut state, personal, work, _) = workspaces();
        state.settings.link_routes = vec![
            route("github.com", personal),
            route("github.com/acme", work),
            route("gist.github.com", personal),
            route("figma.com", work),
        ];

        assert_eq!(link(&state, "https://github.com/acme/app"), Some(work));
        assert_eq!(link(&state, "https://github.com/rust-lang"), Some(personal));
        assert_eq!(
            link(&state, "https://gist.github.com/acme/1"),
            Some(personal)
        );
        assert_eq!(link(&state, "https://example.com/"), None);
    }

    #[test]
    fn a_rule_for_a_private_or_missing_workspace_is_passed_over() {
        let (mut state, personal, _, private) = workspaces();
        state.settings.link_routes = vec![
            route("figma.com", personal),
            route("figma.com/file", private),
            route("figma.com/file/1", WorkspaceId::new()),
        ];

        assert_eq!(link(&state, "https://figma.com/file/1"), Some(personal));
    }

    #[test]
    fn rules_keep_only_valid_patterns_and_existing_workspaces_that_last() {
        let (state, personal, work, private) = workspaces();
        let kept = sanitized_link_routes(
            vec![
                route("https://www.Figma.com/", work),
                route("figma.com", personal),
                route("not a site", work),
                route("secret.com", private),
                route("gone.com", WorkspaceId::new()),
            ],
            &state.workspaces,
        );

        assert_eq!(kept, vec![route("figma.com", personal)]);
    }

    #[test]
    fn deleting_a_workspace_drops_its_rules() {
        let (mut state, personal, work, _) = workspaces();
        state.settings.link_routes = vec![route("figma.com", work), route("github.com", personal)];

        let state = state.with_workspace_removed(work).unwrap();
        assert_eq!(
            state.settings.link_routes,
            vec![route("github.com", personal)]
        );
    }
}
