//! Installed browser extensions and the review shown before installing one.

use serde::{Deserialize, Serialize};

use crate::extensions::access::SiteAccess;
use crate::extensions::listing::{same_name, StoreSuggestion};
use crate::extensions::updates::PendingUpdate;
use crate::workspaces::workspace::Workspace;
use crate::{WisError, WisResult, WorkspaceId};

/// An extension copied into Wis's extensions folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledExtension {
    /// Its folder name, and the identifier its storage is kept under.
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub disabled_in: Vec<WorkspaceId>,
    #[serde(default)]
    pub allow_private: bool,
    #[serde(default)]
    pub pinned: bool,
    /// Permissions and match patterns it asked for while running and was allowed.
    #[serde(default)]
    pub granted: Vec<String>,
    #[serde(default)]
    pub site_access: SiteAccess,
    #[serde(default)]
    pub allowed_sites: Vec<String>,
    #[serde(default)]
    pub store_id: Option<String>,
    #[serde(default)]
    pub pending_update: Option<PendingUpdate>,
    #[serde(default)]
    pub store_suggestion: Option<StoreSuggestion>,
    #[serde(default)]
    pub declined_listings: Vec<String>,
    /// When the daily search last looked for its listing, in Unix seconds.
    #[serde(default)]
    pub store_searched_at: Option<u64>,
}

pub const MAX_EXTENSIONS: usize = 64;

/// Blocking rules on by default from which the review warns.
pub const LARGE_RULE_SET: usize = 10_000;

/// What the install review shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtensionReview {
    pub name: String,
    pub version: String,
    pub description: String,
    /// What it can do, in words, most far-reaching first.
    pub abilities: Vec<String>,
    /// Permissions it asks for that Wis cannot give.
    pub unsupported: Vec<String>,
    /// Its blocking rules on by default, when there are enough to warn about.
    pub large_rule_set: Option<usize>,
}

/// What the review is built from: the manifest's own words, and what WebKit
/// made of it.
pub struct ReviewInput<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub description: &'a str,
    /// The manifest's `permissions`.
    pub declared: &'a [String],
    /// The permissions WebKit recognised.
    pub recognised: &'a [String],
    /// Every match pattern it asks for.
    pub patterns: &'a [String],
    pub default_rules: usize,
}

const MAX_NAME: usize = 100;
pub(crate) const MAX_VERSION: usize = 32;
const MAX_DESCRIPTION: usize = 300;
const NAMED_SITES: usize = 3;

const EVERY_SITE: &str = "Read and change everything on every website";

/// Permissions whose reach the site access already covers, or that reach nothing.
const QUIET: [&str; 8] = [
    "activeTab",
    "alarms",
    "contextMenus",
    "declarativeNetRequestFeedback",
    "menus",
    "scripting",
    "storage",
    "unlimitedStorage",
];

const SPOKEN: [(&str, &str); 8] = [
    ("tabs", "Read the address and title of your tabs"),
    ("webNavigation", "See which pages you visit"),
    (
        "cookies",
        "Read and change cookies on the sites it can reach",
    ),
    ("webRequest", "Watch the requests websites make"),
    (
        "declarativeNetRequest",
        "Block or change requests websites make",
    ),
    (
        "declarativeNetRequestWithHostAccess",
        "Block or change requests websites make",
    ),
    ("clipboardWrite", "Change what you copy"),
    ("notifications", "Show notifications"),
];

/// Recognised by WebKit, but only usable through an app helper Wis does not provide.
const NEEDS_APP: [&str; 1] = ["nativeMessaging"];

/// The installed extension, `name` from a folder, or from the store as `store_id`.
/// Used for identifying duplicates when installing a new extension.
pub fn duplicate_of<'a>(
    list: &'a [InstalledExtension],
    name: Option<&str>,
    store_id: Option<&str>,
) -> Option<&'a InstalledExtension> {
    list.iter().find(|e| {
        store_id.is_some_and(|id| e.store_id.as_deref() == Some(id))
            || name.is_some_and(|n| same_name(n, &e.name))
    })
}

/// The extension name a Chrome Web Store item page's title gives.
pub fn store_page_name(title: &str) -> Option<&str> {
    title
        .strip_suffix(" - Chrome Web Store")
        .map(str::trim)
        .filter(|n| !n.is_empty())
}

/// A Chrome Web Store id: 32 letters from `a` to `p`.
pub fn is_store_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// The store id of a Chrome Web Store item page.
pub fn store_id_from_url(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != "https" {
        return None;
    }

    let segments: Vec<&str> = url.path_segments()?.collect();
    let item = match url.host_str()? {
        "chromewebstore.google.com" => segments.strip_prefix(&["detail"])?,
        "chrome.google.com" => segments.strip_prefix(&["webstore", "detail"])?,
        _ => return None,
    };

    item.iter()
        .take(2)
        .find(|s| is_store_id(s))
        .map(|s| s.to_string())
}

pub fn is_extension_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

/// Words cut to `max` characters, with hidden and direction-changing characters dropped.
pub(crate) fn tidy(raw: &str, max: usize) -> String {
    raw.split(|c: char| c.is_whitespace())
        .map(|word| word.chars().filter(|c| !is_hidden(*c)).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// An extension's name as Wis shows it.
pub fn tidy_name(raw: &str) -> String {
    let name = tidy(raw, MAX_NAME);
    if name.is_empty() {
        "Extension".into()
    } else {
        name
    }
}

pub fn with_extension(
    list: &[InstalledExtension],
    ext: InstalledExtension,
) -> WisResult<Vec<InstalledExtension>> {
    let refuse = |message: &str| {
        Err(WisError::InvalidInput {
            message: message.into(),
        })
    };
    if !is_extension_id(&ext.id) {
        return refuse("not an extension id");
    }
    if list.iter().any(|e| e.id == ext.id) {
        return refuse("that extension is already installed");
    }
    if ext.store_id.as_deref().is_some_and(|id| !is_store_id(id)) {
        return refuse("not a Chrome Web Store id");
    }
    if let Some(installed) = duplicate_of(list, Some(&ext.name), ext.store_id.as_deref()) {
        return refuse(&format!("{} is already installed", installed.name));
    }
    if list.len() >= MAX_EXTENSIONS {
        return refuse("too many extensions are installed");
    }

    let mut next = list.to_vec();
    next.push(InstalledExtension {
        id: ext.id,
        name: tidy_name(&ext.name),
        version: tidy(&ext.version, MAX_VERSION),
        store_id: ext.store_id,
        ..Default::default()
    });

    Ok(next)
}

pub fn without_extension(list: &[InstalledExtension], id: &str) -> Vec<InstalledExtension> {
    list.iter().filter(|e| e.id != id).cloned().collect()
}

pub fn runs_in(ext: &InstalledExtension, workspace: &Workspace) -> bool {
    if workspace.ephemeral {
        ext.allow_private
    } else {
        !ext.disabled_in.contains(&workspace.id)
    }
}

/// The ids of the extensions that run in `workspace`.
pub fn wanted_in(list: &[InstalledExtension], workspace: &Workspace) -> Vec<String> {
    list.iter()
        .filter(|e| is_extension_id(&e.id) && runs_in(e, workspace))
        .map(|e| e.id.clone())
        .collect()
}

pub(crate) fn with_changed(
    list: &[InstalledExtension],
    id: &str,
    change: impl FnOnce(&mut InstalledExtension),
) -> WisResult<Vec<InstalledExtension>> {
    let mut next = list.to_vec();
    let ext = next
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| WisError::InvalidInput {
            message: "that extension is not installed".into(),
        })?;
    change(ext);
    Ok(next)
}

pub fn with_workspace_switch(
    list: &[InstalledExtension],
    id: &str,
    workspace: WorkspaceId,
    on: bool,
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| {
        ext.disabled_in.retain(|w| *w != workspace);
        if !on {
            ext.disabled_in.push(workspace);
        }
    })
}

pub fn with_pinned(
    list: &[InstalledExtension],
    id: &str,
    pinned: bool,
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| ext.pinned = pinned)
}

pub fn with_private_allowed(
    list: &[InstalledExtension],
    id: &str,
    allowed: bool,
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| ext.allow_private = allowed)
}

pub fn is_site_pattern(permission: &str) -> bool {
    permission == "<all_urls>" || permission.contains("://")
}

/// The host a match pattern names, `None` for every host.
fn pattern_host(pattern: &str) -> Option<&str> {
    if pattern == "<all_urls>" {
        return None;
    }

    let rest = pattern.split_once("://").map_or(pattern, |(_, rest)| rest);
    let host = rest.split('/').next().unwrap_or("");
    match host.trim_start_matches("*.") {
        "" | "*" => None,
        named => Some(named),
    }
}

fn site_access(patterns: &[String]) -> Option<String> {
    let mut hosts = std::collections::BTreeSet::new();
    for pattern in patterns {
        match pattern_host(pattern) {
            None => return Some(EVERY_SITE.into()),
            Some(host) => {
                hosts.insert(tidy(host, MAX_NAME));
            }
        }
    }

    let hosts: Vec<String> = hosts.into_iter().collect();
    let named = match hosts.as_slice() {
        [] => return None,
        [only] => only.clone(),
        listed if listed.len() <= NAMED_SITES => {
            format!(
                "{} and {}",
                listed[..listed.len() - 1].join(", "),
                listed[listed.len() - 1]
            )
        }
        listed => {
            let more = listed.len() - NAMED_SITES;
            let noun = if more == 1 { "site" } else { "sites" };
            format!(
                "{} and {more} more {noun}",
                listed[..NAMED_SITES].join(", ")
            )
        }
    };

    Some(format!("Read and change your data on {named}"))
}

/// What `permissions` and `patterns` let an extension do, in words, most
/// far-reaching first.
pub(crate) fn abilities(permissions: &[String], patterns: &[String]) -> Vec<String> {
    let mut abilities: Vec<String> = site_access(patterns).into_iter().collect();
    for (permission, words) in SPOKEN {
        if permissions.iter().any(|p| p == permission) && !abilities.iter().any(|a| a == words) {
            abilities.push(words.into());
        }
    }
    let mut unknown: Vec<&String> = permissions
        .iter()
        .filter(|p| !QUIET.contains(&p.as_str()) && !SPOKEN.iter().any(|(known, _)| known == p))
        .collect();
    unknown.sort();
    unknown.dedup();
    abilities.extend(
        unknown
            .into_iter()
            .map(|p| format!("Use “{}”", tidy(p, MAX_NAME))),
    );

    abilities
}

pub fn review(input: &ReviewInput) -> ExtensionReview {
    let recognised = |p: &str| input.recognised.iter().any(|r| r == p);
    let usable: Vec<String> = input
        .recognised
        .iter()
        .filter(|p| !NEEDS_APP.contains(&p.as_str()))
        .cloned()
        .collect();
    let abilities = abilities(&usable, input.patterns);

    let mut unsupported: Vec<String> = input
        .declared
        .iter()
        .filter(|p| !is_site_pattern(p) && (!recognised(p) || NEEDS_APP.contains(&p.as_str())))
        .map(|p| tidy(p, MAX_NAME))
        .collect();
    unsupported.sort();
    unsupported.dedup();

    ExtensionReview {
        name: tidy_name(input.name),
        version: tidy(input.version, MAX_VERSION),
        description: tidy(input.description, MAX_DESCRIPTION),
        abilities,
        unsupported,
        large_rule_set: (input.default_rules >= LARGE_RULE_SET).then_some(input.default_rules),
    }
}

/// A rule file's path within the extension: a leading `/` names its root,
/// and nothing may climb out of it.
fn inside_path(path: &str) -> Option<&str> {
    let path = path.trim_start_matches('/');
    let climbs = path.split('/').any(|part| part == ".." || part.is_empty());
    (!path.is_empty() && !climbs && !path.contains('\\')).then_some(path)
}

/// Blocking rules a manifest turns on by default, given each rule file's
/// text by its path within the extension.
pub fn default_rule_count(
    manifest: &serde_json::Value,
    read: impl Fn(&str) -> Option<String>,
) -> usize {
    let Some(resources) = manifest
        .pointer("/declarative_net_request/rule_resources")
        .and_then(|r| r.as_array())
    else {
        return 0;
    };

    resources
        .iter()
        .filter(|r| r.get("enabled").and_then(|e| e.as_bool()) == Some(true))
        .filter_map(|r| r.get("path").and_then(|p| p.as_str()).and_then(inside_path))
        .filter_map(&read)
        .filter_map(|text| serde_json::from_str::<Vec<serde::de::IgnoredAny>>(&text).ok())
        .map(|rules| rules.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(id: &str) -> InstalledExtension {
        InstalledExtension {
            id: id.into(),
            name: "Dark Reader".into(),
            version: "4.9.133".into(),
            ..Default::default()
        }
    }

    fn regular() -> Workspace {
        Workspace::new("Work".into(), Default::default())
    }

    fn private() -> Workspace {
        Workspace::new_private("Private".into(), Default::default())
    }

    #[test]
    fn an_extension_runs_in_every_regular_workspace_until_switched_off() {
        let (work, home) = (regular(), regular());
        let list = with_workspace_switch(&[ext(ID)], ID, work.id, false).unwrap();
        assert!(!runs_in(&list[0], &work));
        assert!(runs_in(&list[0], &home));

        let list = with_workspace_switch(&list, ID, work.id, true).unwrap();
        assert!(runs_in(&list[0], &work));
        assert!(list[0].disabled_in.is_empty());
    }

    #[test]
    fn private_workspaces_run_only_what_is_allowed_there() {
        let private = private();
        assert!(!runs_in(&ext(ID), &private));
        let list = with_private_allowed(&[ext(ID)], ID, true).unwrap();
        assert!(runs_in(&list[0], &private));
    }

    #[test]
    fn pinning_marks_only_that_extension() {
        let list = with_pinned(&[ext(ID), ext(OTHER)], ID, true).unwrap();
        assert!(list[0].pinned && !list[1].pinned);
        assert!(!with_pinned(&list, ID, false).unwrap()[0].pinned);
        assert!(with_pinned(&list, "missing", true).is_err());
    }

    #[test]
    fn switches_refuse_an_unknown_extension() {
        assert!(with_workspace_switch(&[ext(ID)], OTHER, regular().id, false).is_err());
        assert!(with_private_allowed(&[ext(ID)], OTHER, true).is_err());
    }

    #[test]
    fn switching_one_extension_leaves_the_others() {
        let work = regular();
        let list = with_workspace_switch(&[ext(ID), ext(OTHER)], ID, work.id, false).unwrap();
        assert_eq!(wanted_in(&list, &work), vec![OTHER.to_string()]);
    }

    #[test]
    fn wanted_ids_leave_out_any_that_could_not_name_a_folder() {
        let work = regular();
        assert_eq!(
            wanted_in(&[ext("../x"), ext(ID)], &work),
            vec![ID.to_string()]
        );
    }

    const ID: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210";

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn input<'a>(
        declared: &'a [String],
        recognised: &'a [String],
        patterns: &'a [String],
    ) -> ReviewInput<'a> {
        ReviewInput {
            name: "Dark Reader",
            version: "4.9.133",
            description: "Dark mode for every website.",
            declared,
            recognised,
            patterns,
            default_rules: 0,
        }
    }

    #[test]
    fn ids_are_32_lowercase_hex_digits() {
        assert!(is_extension_id(ID));
        assert!(!is_extension_id("0123456789ABCDEF0123456789ABCDEF"));
        assert!(!is_extension_id("../../etc"));
        assert!(!is_extension_id(""));
        assert!(!is_extension_id(&format!("{ID}0")));
    }

    #[test]
    fn installs_append_and_tidy_their_labels() {
        let added = with_extension(
            &[named(OTHER, "SponsorBlock")],
            InstalledExtension {
                id: ID.into(),
                name: "  Dark\u{202e}  Reader\n".into(),
                version: "4.9\u{0}.133".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(added[1].name, "Dark Reader");
        assert_eq!(added[1].version, "4.9.133");
    }

    const STORE: &str = "eimadpbcbfnmbkopoojfekhnkhdbieeh";

    #[test]
    fn store_ids_are_32_letters_from_a_to_p() {
        assert!(is_store_id(STORE));
        assert!(!is_store_id("eimadpbcbfnmbkopoojfekhnkhdbieez"));
        assert!(!is_store_id("EIMADPBCBFNMBKOPOOJFEKHNKHDBIEEH"));
        assert!(!is_store_id(&STORE[1..]));
    }

    #[test]
    fn store_item_pages_name_their_extension() {
        for url in [
            "https://chromewebstore.google.com/detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh",
            "https://chromewebstore.google.com/detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh?hl=en&pli=1",
            "https://chromewebstore.google.com/detail/eimadpbcbfnmbkopoojfekhnkhdbieeh/reviews",
            "https://chrome.google.com/webstore/detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh",
        ] {
            assert_eq!(store_id_from_url(url).as_deref(), Some(STORE), "{url}");
        }
        for url in [
            "https://chromewebstore.google.com/",
            "https://chromewebstore.google.com/category/extensions",
            "http://chromewebstore.google.com/detail/x/eimadpbcbfnmbkopoojfekhnkhdbieeh",
            "https://evil.test/detail/x/eimadpbcbfnmbkopoojfekhnkhdbieeh",
            "https://chromewebstore.google.com.evil.test/detail/x/eimadpbcbfnmbkopoojfekhnkhdbieeh",
        ] {
            assert_eq!(store_id_from_url(url), None, "{url}");
        }
    }

    fn named(id: &str, name: &str) -> InstalledExtension {
        InstalledExtension {
            name: name.into(),
            ..ext(id)
        }
    }

    #[test]
    fn an_extension_installs_once_by_name_or_store_id() {
        let list = with_extension(&[], ext(ID)).unwrap();
        assert!(with_extension(&list, named(OTHER, "dark  READER ")).is_err());
        assert_eq!(
            duplicate_of(&list, Some(" DARK Reader"), None).map(|e| e.id.as_str()),
            Some(ID)
        );
        assert!(duplicate_of(&list, Some("Dark Reader Pro"), None).is_none());

        let mut from_store = named(OTHER, "SponsorBlock");
        from_store.store_id = Some(STORE.into());
        let list = with_extension(&list, from_store).unwrap();
        assert_eq!(
            duplicate_of(&list, Some("Renamed"), Some(STORE)).map(|e| e.id.as_str()),
            Some(OTHER)
        );
        assert!(with_extension(&list, named("aaaabbbbccccddddeeeeffff00001111", "Vimium")).is_ok());
        assert!(duplicate_of(&list, None, Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")).is_none());
    }

    #[test]
    fn store_page_titles_name_their_extension() {
        assert_eq!(
            store_page_name("Dark Reader - Chrome Web Store"),
            Some("Dark Reader")
        );
        assert_eq!(store_page_name("Chrome Web Store"), None);
        assert_eq!(store_page_name(" - Chrome Web Store"), None);
        assert_eq!(store_page_name("Dark Reader"), None);
    }

    #[test]
    fn a_store_extension_installs_once() {
        let mut from_store = ext(ID);
        from_store.store_id = Some(STORE.into());
        let list = with_extension(&[], from_store).unwrap();
        assert_eq!(list[0].store_id.as_deref(), Some(STORE));
        let mut again = ext(OTHER);
        again.store_id = Some(STORE.into());
        assert!(with_extension(&list, again).is_err());
        let mut bad = ext(OTHER);
        bad.store_id = Some("not-an-id".into());
        assert!(with_extension(&list, bad).is_err());
    }

    #[test]
    fn installs_refuse_bad_ids_duplicates_and_overflow() {
        assert!(with_extension(&[], ext("nope")).is_err());
        assert!(with_extension(&[ext(ID)], ext(ID)).is_err());
        let full: Vec<_> = (0..MAX_EXTENSIONS)
            .map(|i| named(&format!("{i:032x}"), &format!("Extension {i}")))
            .collect();
        assert!(with_extension(&full, ext(ID)).is_err());
    }

    #[test]
    fn a_nameless_extension_is_named_for_what_it_is() {
        let mut nameless = ext(ID);
        nameless.name = " \n ".into();
        assert_eq!(with_extension(&[], nameless).unwrap()[0].name, "Extension");
    }

    #[test]
    fn removal_keeps_the_others() {
        assert_eq!(
            without_extension(&[ext(ID), ext(OTHER)], ID),
            vec![ext(OTHER)]
        );
        assert_eq!(without_extension(&[ext(OTHER)], ID), vec![ext(OTHER)]);
    }

    #[test]
    fn every_site_access_leads_the_review() {
        let declared = strings(&["storage", "tabs"]);
        let recognised = strings(&["storage", "tabs"]);
        for pattern in ["<all_urls>", "*://*/*", "https://*/*"] {
            let patterns = strings(&[pattern]);
            let review = review(&input(&declared, &recognised, &patterns));
            assert_eq!(
                review.abilities,
                strings(&[
                    "Read and change everything on every website",
                    "Read the address and title of your tabs",
                ]),
                "{pattern}"
            );
        }
    }

    #[test]
    fn named_sites_are_listed_up_to_three() {
        let none: Vec<String> = Vec::new();
        let patterns = strings(&[
            "https://*.example.com/*",
            "https://a.test/*",
            "http://a.test/path*",
            "https://b.test/*",
            "https://c.test/*",
        ]);
        assert_eq!(
            review(&input(&none, &none, &patterns)).abilities,
            strings(&["Read and change your data on a.test, b.test, c.test and 1 more site"])
        );
        assert_eq!(
            review(&input(&none, &none, &strings(&["https://a.test/*"]))).abilities,
            strings(&["Read and change your data on a.test"])
        );
    }

    #[test]
    fn quiet_permissions_say_nothing_and_unknown_ones_are_named() {
        let declared = strings(&["storage", "alarms", "scripting", "activeTab", "futureThing"]);
        let recognised = declared.clone();
        let none: Vec<String> = Vec::new();
        assert_eq!(
            review(&input(&declared, &recognised, &none)).abilities,
            strings(&["Use “futureThing”"])
        );
    }

    #[test]
    fn what_webkit_dropped_is_unsupported() {
        let declared = strings(&[
            "storage",
            "history",
            "sidePanel",
            "nativeMessaging",
            "<all_urls>",
            "https://*/*",
            "history",
        ]);
        let recognised = strings(&["storage", "nativeMessaging"]);
        let none: Vec<String> = Vec::new();
        assert_eq!(
            review(&input(&declared, &recognised, &none)).unsupported,
            strings(&["history", "nativeMessaging", "sidePanel"])
        );
    }

    #[test]
    fn labels_in_the_review_are_tidied() {
        let none: Vec<String> = Vec::new();
        let mut raw = input(&none, &none, &none);
        raw.name = "\u{202e}Evil";
        raw.description = "x".repeat(2000).leak();
        let review = review(&raw);
        assert_eq!(review.name, "Evil");
        assert_eq!(review.description.chars().count(), 300);
    }

    #[test]
    fn large_rule_sets_are_flagged() {
        let none: Vec<String> = Vec::new();
        let mut small = input(&none, &none, &none);
        small.default_rules = LARGE_RULE_SET - 1;
        assert_eq!(review(&small).large_rule_set, None);
        let mut large = input(&none, &none, &none);
        large.default_rules = 18_755;
        assert_eq!(review(&large).large_rule_set, Some(18_755));
    }

    #[test]
    fn rule_counts_take_only_the_rulesets_on_by_default() {
        let manifest = serde_json::json!({
            "declarative_net_request": { "rule_resources": [
                { "id": "a", "enabled": true, "path": "/rules/a.json" },
                { "id": "b", "enabled": false, "path": "rules/b.json" },
                { "id": "c", "enabled": true, "path": "rules/c.json" },
                { "id": "d", "enabled": true, "path": "../outside.json" },
                { "id": "e", "enabled": true, "path": "rules/broken.json" }
            ]}
        });
        let read = |path: &str| match path {
            "rules/a.json" => Some("[{}, {}, {}]".to_string()),
            "rules/b.json" => Some("[{}]".to_string()),
            "rules/c.json" => Some("[{}]".to_string()),
            "../outside.json" | "outside.json" => Some("[{}, {}]".to_string()),
            "rules/broken.json" => Some("not json".to_string()),
            _ => None,
        };
        assert_eq!(default_rule_count(&manifest, read), 4);
        assert_eq!(default_rule_count(&serde_json::json!({}), read), 0);
    }
}
