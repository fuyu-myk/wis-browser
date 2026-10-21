//! Which domains share an owner, so Strict privacy can let a site run scripts
//! from its owner's other domains. Two sources: Wis's own list and Google's
//! Related Website Sets.

use serde_json::{json, Value};

const WIS_LIST: &str = include_str!("../data/site_entities.txt");
const RELATED_WEBSITE_SETS: &str = include_str!("../data/related_website_sets.txt");

fn listed(text: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split_whitespace().collect())
}

fn merged(lines: impl Iterator<Item = Vec<&'static str>>) -> Vec<Vec<&'static str>> {
    let mut groups: Vec<Vec<&'static str>> = Vec::new();
    for line in lines {
        let (joined, rest): (Vec<_>, Vec<_>) = groups
            .into_iter()
            .partition(|group| group.iter().any(|domain| line.contains(domain)));
        let mut group: Vec<&'static str> = joined.into_iter().flatten().collect();
        for domain in line {
            if !group.contains(&domain) {
                group.push(domain);
            }
        }
        groups = rest;
        groups.push(group);
    }
    groups.retain(|group| group.len() > 1);
    groups
}

pub fn entities() -> Vec<Vec<&'static str>> {
    merged(listed(WIS_LIST).chain(listed(RELATED_WEBSITE_SETS)))
}

fn host_filter(domain: &str) -> String {
    format!(
        "^[a-z][a-z0-9.+-]*://([^/:]*\\.)?{}[/:]",
        domain.replace('.', "\\.")
    )
}

/// `ignore-previous-rules` exceptions that let a page on one of an owner's
/// domains run scripts from the owner's other domains. They run right
/// after the rule blocking third-party scripts and before the filter lists.
pub fn own_script_exceptions(entities: &[Vec<&str>]) -> Vec<Value> {
    entities
        .iter()
        .flat_map(|entity| {
            let pages: Vec<String> = entity.iter().map(|domain| format!("*{domain}")).collect();
            entity.iter().map(move |domain| {
                json!({
                    "trigger": {
                        "url-filter": host_filter(domain),
                        "resource-type": ["script"],
                        "if-domain": pages,
                    },
                    "action": { "type": "ignore-previous-rules" }
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_of<'a>(
        entities: &'a [Vec<&'static str>],
        domain: &str,
    ) -> Option<&'a Vec<&'static str>> {
        entities.iter().find(|entity| entity.contains(&domain))
    }

    #[test]
    fn every_listed_domain_is_a_bare_lowercase_host() {
        let host = regex::Regex::new(r"^[a-z0-9-]+(\.[a-z0-9-]+)+$").unwrap();
        for domain in listed(WIS_LIST)
            .chain(listed(RELATED_WEBSITE_SETS))
            .flatten()
        {
            assert!(host.is_match(domain), "{domain}");
        }
    }

    #[test]
    fn wis_lists_each_domain_once() {
        let mut seen = std::collections::HashSet::new();
        for domain in listed(WIS_LIST).flatten() {
            assert!(seen.insert(domain), "{domain} is listed twice");
        }
    }

    #[test]
    fn an_owner_groups_its_sites_and_asset_domains() {
        let entities = entities();
        let quora = group_of(&entities, "quora.com").unwrap();
        assert!(quora.contains(&"quoracdn.net"));
        assert!(group_of(&entities, "facebook.com")
            .unwrap()
            .contains(&"fbcdn.net"));
        assert!(group_of(&entities, "github.com")
            .unwrap()
            .contains(&"githubassets.com"));
    }

    #[test]
    fn sets_that_share_a_domain_merge() {
        let entities = merged(
            [
                vec!["a.test", "b.test"],
                vec!["c.test", "d.test"],
                vec!["b.test", "e.test"],
            ]
            .into_iter(),
        );
        assert_eq!(entities.len(), 2);
        let first = group_of(&entities, "a.test").unwrap();
        assert!(first.contains(&"e.test"));
        assert!(!first.contains(&"c.test"));
    }

    #[test]
    fn an_exception_lets_an_owner_s_page_run_scripts_from_its_other_domains() {
        let rules = own_script_exceptions(&[vec!["quora.com", "quoracdn.net"]]);
        assert_eq!(rules.len(), 2);
        let cdn = rules
            .iter()
            .find(|rule| {
                rule["trigger"]["url-filter"]
                    .as_str()
                    .unwrap()
                    .contains("quoracdn")
            })
            .unwrap();
        assert_eq!(cdn["action"]["type"], "ignore-previous-rules");
        assert_eq!(
            cdn["trigger"]["resource-type"],
            serde_json::json!(["script"])
        );
        assert_eq!(
            cdn["trigger"]["if-domain"],
            serde_json::json!(["*quora.com", "*quoracdn.net"])
        );

        let filter = regex::Regex::new(cdn["trigger"]["url-filter"].as_str().unwrap()).unwrap();
        assert!(filter.is_match("https://qsbr.cf2.quoracdn.net/-4-ans.js"));
        assert!(filter.is_match("https://quoracdn.net:8443/a.js"));
        assert!(!filter.is_match("https://evilquoracdn.net/a.js"));
        assert!(!filter.is_match("https://quoracdn.net.evil.example/a.js"));
        assert!(!filter.is_match("https://evil.example/quoracdn.net/a.js"));
    }
}
