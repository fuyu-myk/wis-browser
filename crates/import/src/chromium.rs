use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use wis_core::bookmark_tree::BookmarkNode;
use wis_core::netscape::{MAX_BOOKMARKS_FILE_BYTES, MAX_IMPORT_BOOKMARKS, MAX_IMPORT_FOLDERS};

use crate::fs::read_capped;
use crate::{Browser, ImportError, Roots, Source};

const BOOKMARK_FILES: [&str; 2] = ["Bookmarks", "AccountBookmarks"];
const LOCAL_STATE_CAP: usize = 16 * 1024 * 1024;
const MAX_BOOKMARK_OBJECTS: usize = 4 * (MAX_IMPORT_BOOKMARKS + MAX_IMPORT_FOLDERS);
const MAX_LOCAL_STATE_OBJECTS: usize = 100_000;
const MAX_PROFILES: usize = 64;
const PROFILE_NAME_CAP: usize = 64;

pub(crate) fn discover(roots: &Roots) -> Vec<Source> {
    user_data_dirs(roots)
        .into_iter()
        .flat_map(|(browser, key, dir)| profiles(browser, key, &dir))
        .collect()
}

pub(crate) fn read_bookmarks(profile: &Path) -> Result<Vec<BookmarkNode>, ImportError> {
    let mut tree = Vec::new();
    for file in BOOKMARK_FILES {
        let Some(bytes) = read_capped(&profile.join(file), MAX_BOOKMARKS_FILE_BYTES)? else {
            continue;
        };
        if json_objects(&bytes) > MAX_BOOKMARK_OBJECTS {
            return Err(ImportError::TooLarge {
                file: file.to_string(),
            });
        }

        let nodes = parse_bookmarks(&bytes).map_err(|err| ImportError::Unreadable {
            file: file.to_string(),
            reason: err.to_string(),
        })?;
        tree.extend(nodes);
    }

    Ok(tree)
}

fn profiles(browser: Browser, key: &str, user_data: &Path) -> Vec<Source> {
    let variant = variant(key);
    let mut found: Vec<Source> = profile_names(user_data)
        .into_iter()
        .filter(|(dir_name, _)| is_plain_name(dir_name))
        .take(MAX_PROFILES)
        .map(|(dir_name, name)| (user_data.join(&dir_name), dir_name, name))
        .filter(|(dir, _, _)| has_bookmarks(dir))
        .map(|(dir, dir_name, name)| Source {
            id: format!("{key}:{dir_name}"),
            browser,
            variant,
            profile: name,
            dir,
        })
        .collect();

    if found.is_empty() && has_bookmarks(user_data) {
        found.push(Source {
            id: format!("{key}:"),
            browser,
            variant,
            profile: "Default".to_string(),
            dir: user_data.to_path_buf(),
        });
    }

    found
}

fn variant(key: &str) -> Option<&'static str> {
    if key.ends_with("-flatpak") {
        Some("Flatpak")
    } else if key.ends_with("-snap") {
        Some("Snap")
    } else {
        None
    }
}

/// `(directory, display name)` from `Local State`, or just `Default`.
fn profile_names(user_data: &Path) -> Vec<(String, String)> {
    #[derive(Deserialize)]
    struct LocalState {
        #[serde(default)]
        profile: ProfileSection,
    }

    #[derive(Deserialize, Default)]
    struct ProfileSection {
        #[serde(default)]
        info_cache: BTreeMap<String, ProfileInfo>,
    }

    #[derive(Deserialize)]
    struct ProfileInfo {
        #[serde(default)]
        name: String,
    }

    let listed: Vec<(String, String)> =
        read_capped(&user_data.join("Local State"), LOCAL_STATE_CAP)
            .ok()
            .flatten()
            .filter(|bytes| json_objects(bytes) <= MAX_LOCAL_STATE_OBJECTS)
            .and_then(|bytes| serde_json::from_slice::<LocalState>(&bytes).ok())
            .map(|state| {
                state
                    .profile
                    .info_cache
                    .into_iter()
                    .map(|(dir, info)| {
                        let name = display_name(&info.name);
                        let name = if name.is_empty() { dir.clone() } else { name };
                        (dir, name)
                    })
                    .collect()
            })
            .unwrap_or_default();

    if listed.is_empty() {
        vec![("Default".to_string(), "Default".to_string())]
    } else {
        listed
    }
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

/// Objects in a JSON text, counted before parsing so a small file cannot
/// expand into millions of nodes.
fn json_objects(bytes: &[u8]) -> usize {
    let (mut count, mut in_string, mut escaped) = (0, false, false);
    for &byte in bytes {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b'{' {
            count += 1;
        }
    }
    count
}

fn has_bookmarks(dir: &Path) -> bool {
    BOOKMARK_FILES.iter().any(|file| dir.join(file).is_file())
}

fn display_name(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(PROFILE_NAME_CAP)
        .collect()
}

fn under(base: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(base.to_path_buf(), |dir, part| dir.join(part))
}

#[cfg(target_os = "macos")]
fn user_data_dirs(roots: &Roots) -> Vec<(Browser, &'static str, PathBuf)> {
    let base = &roots.config;
    vec![
        (
            Browser::Chrome,
            "chrome",
            under(base, &["Google", "Chrome"]),
        ),
        (Browser::Chromium, "chromium", under(base, &["Chromium"])),
        (Browser::Edge, "edge", under(base, &["Microsoft Edge"])),
        (
            Browser::Brave,
            "brave",
            under(base, &["BraveSoftware", "Brave-Browser"]),
        ),
        (Browser::Vivaldi, "vivaldi", under(base, &["Vivaldi"])),
        (
            Browser::Opera,
            "opera",
            under(base, &["com.operasoftware.Opera"]),
        ),
        (Browser::Arc, "arc", under(base, &["Arc", "User Data"])),
    ]
}

#[cfg(windows)]
fn user_data_dirs(roots: &Roots) -> Vec<(Browser, &'static str, PathBuf)> {
    let local = &roots.local_data;
    let mut dirs = vec![
        (
            Browser::Chrome,
            "chrome",
            under(local, &["Google", "Chrome", "User Data"]),
        ),
        (
            Browser::Chromium,
            "chromium",
            under(local, &["Chromium", "User Data"]),
        ),
        (
            Browser::Edge,
            "edge",
            under(local, &["Microsoft", "Edge", "User Data"]),
        ),
        (
            Browser::Brave,
            "brave",
            under(local, &["BraveSoftware", "Brave-Browser", "User Data"]),
        ),
        (
            Browser::Vivaldi,
            "vivaldi",
            under(local, &["Vivaldi", "User Data"]),
        ),
        (
            Browser::Opera,
            "opera",
            under(&roots.config, &["Opera Software", "Opera Stable"]),
        ),
    ];
    let arc = std::fs::read_dir(local.join("Packages"))
        .into_iter()
        .flatten()
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("TheBrowserCompany.Arc_")
        });
    if let Some(package) = arc {
        dirs.push((
            Browser::Arc,
            "arc",
            under(
                &package.path(),
                &["LocalCache", "Local", "Arc", "User Data"],
            ),
        ));
    }

    dirs
}

#[cfg(target_os = "linux")]
fn user_data_dirs(roots: &Roots) -> Vec<(Browser, &'static str, PathBuf)> {
    let config = &roots.config;
    let flatpak = roots.home.join(".var").join("app");
    vec![
        (Browser::Chrome, "chrome", under(config, &["google-chrome"])),
        (Browser::Chromium, "chromium", under(config, &["chromium"])),
        (Browser::Edge, "edge", under(config, &["microsoft-edge"])),
        (
            Browser::Brave,
            "brave",
            under(config, &["BraveSoftware", "Brave-Browser"]),
        ),
        (Browser::Vivaldi, "vivaldi", under(config, &["vivaldi"])),
        (Browser::Opera, "opera", under(config, &["opera"])),
        (
            Browser::Chrome,
            "chrome-flatpak",
            under(&flatpak, &["com.google.Chrome", "config", "google-chrome"]),
        ),
        (
            Browser::Chromium,
            "chromium-flatpak",
            under(&flatpak, &["org.chromium.Chromium", "config", "chromium"]),
        ),
        (
            Browser::Brave,
            "brave-flatpak",
            under(
                &flatpak,
                &[
                    "com.brave.Browser",
                    "config",
                    "BraveSoftware",
                    "Brave-Browser",
                ],
            ),
        ),
        (
            Browser::Edge,
            "edge-flatpak",
            under(
                &flatpak,
                &["com.microsoft.Edge", "config", "microsoft-edge"],
            ),
        ),
        (
            Browser::Chromium,
            "chromium-snap",
            under(&roots.home, &["snap", "chromium", "common", "chromium"]),
        ),
    ]
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn user_data_dirs(_roots: &Roots) -> Vec<(Browser, &'static str, PathBuf)> {
    Vec::new()
}

#[derive(Deserialize)]
struct BookmarksFile {
    #[serde(default)]
    roots: RootSet,
}

#[derive(Deserialize, Default)]
struct RootSet {
    bookmark_bar: Option<Node>,
    other: Option<Node>,
    synced: Option<Node>,
    custom_root: Option<OperaRoots>,
}

/// Opera's own roots. Its trash is left behind.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperaRoots {
    user_root: Option<Node>,
    unsorted: Option<Node>,
    speed_dial: Option<Node>,
}

#[derive(Deserialize)]
struct Node {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    children: Vec<Node>,
}

/// Mirrors Chrome's own HTML export: the bar and mobile roots are folders,
/// "Other bookmarks" sits at the top level.
fn parse_bookmarks(bytes: &[u8]) -> serde_json::Result<Vec<BookmarkNode>> {
    let file: BookmarksFile = serde_json::from_slice(bytes)?;
    let mut tree = Vec::new();
    if let Some(bar) = file.roots.bookmark_bar {
        tree.push(root_folder(bar, "Bookmarks bar"));
    }
    if let Some(other) = file.roots.other {
        tree.extend(other.children.into_iter().map(convert));
    }
    if let Some(mobile) = file.roots.synced {
        tree.push(root_folder(mobile, "Mobile bookmarks"));
    }
    if let Some(opera) = file.roots.custom_root {
        let roots = [
            (opera.user_root, "My Folders"),
            (opera.unsorted, "Unsorted Bookmarks"),
            (opera.speed_dial, "Speed Dial"),
        ];
        tree.extend(
            roots
                .into_iter()
                .filter_map(|(root, fallback)| root.map(|r| root_folder(r, fallback))),
        );
    }

    Ok(tree)
}

fn root_folder(root: Node, fallback: &str) -> BookmarkNode {
    BookmarkNode::Folder {
        name: if root.name.is_empty() {
            fallback.to_string()
        } else {
            root.name
        },
        children: root.children.into_iter().map(convert).collect(),
    }
}

fn convert(node: Node) -> BookmarkNode {
    if node.kind == "url" {
        BookmarkNode::Link {
            url: node.url,
            title: node.name,
        }
    } else {
        BookmarkNode::Folder {
            name: node.name,
            children: node.children.into_iter().map(convert).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{write, TempDir};

    const BOOKMARKS: &str = r#"{
        "checksum": "x",
        "roots": {
            "bookmark_bar": {
                "name": "Bookmarks bar", "type": "folder",
                "children": [
                    { "name": "A", "type": "url", "url": "https://a.example/" },
                    { "name": "Dev", "type": "folder", "children": [
                        { "name": "B", "type": "url", "url": "https://b.example/" }
                    ] }
                ]
            },
            "other": {
                "name": "Other bookmarks", "type": "folder",
                "children": [ { "name": "C", "type": "url", "url": "https://c.example/" } ]
            },
            "synced": { "name": "Mobile bookmarks", "type": "folder", "children": [] }
        },
        "version": 1
    }"#;

    fn link(url: &str, title: &str) -> BookmarkNode {
        BookmarkNode::Link {
            url: url.to_string(),
            title: title.to_string(),
        }
    }

    fn folder(name: &str, children: Vec<BookmarkNode>) -> BookmarkNode {
        BookmarkNode::Folder {
            name: name.to_string(),
            children,
        }
    }

    fn chrome_dir(roots: &Roots) -> PathBuf {
        user_data_dirs(roots)
            .into_iter()
            .find(|(_, key, _)| *key == "chrome")
            .map(|(_, _, dir)| dir)
            .unwrap()
    }

    #[test]
    fn the_bar_and_mobile_roots_become_folders_and_other_stays_unfiled() {
        let tree = parse_bookmarks(BOOKMARKS.as_bytes()).unwrap();

        assert_eq!(
            tree,
            vec![
                folder(
                    "Bookmarks bar",
                    vec![
                        link("https://a.example/", "A"),
                        folder("Dev", vec![link("https://b.example/", "B")]),
                    ]
                ),
                link("https://c.example/", "C"),
                folder("Mobile bookmarks", vec![]),
            ]
        );
    }

    #[test]
    fn a_file_without_roots_is_empty_and_garbage_is_an_error() {
        assert!(parse_bookmarks(b"{}").unwrap().is_empty());
        assert!(parse_bookmarks(b"{\"roots\": [").is_err());
    }

    #[test]
    fn profiles_come_from_local_state_and_need_a_bookmarks_file() {
        let tmp = TempDir::new();
        let roots = tmp.roots();
        let chrome = chrome_dir(&roots);
        write(
            &chrome.join("Local State"),
            r#"{ "profile": { "info_cache": {
                "Default": { "name": "Personal" },
                "Profile 1": { "name": "Work" },
                "Profile 2": { "name": "Empty" },
                "../escape": { "name": "Nope" }
            } } }"#,
        );
        write(&chrome.join("Default/Bookmarks"), BOOKMARKS);
        write(&chrome.join("Profile 1/AccountBookmarks"), BOOKMARKS);
        std::fs::create_dir_all(chrome.join("Profile 2")).unwrap();
        write(
            &chrome.parent().unwrap().join("escape/Bookmarks"),
            BOOKMARKS,
        );

        let found: Vec<(String, String)> = discover(&roots)
            .into_iter()
            .map(|s| (s.id, s.profile))
            .collect();

        assert_eq!(
            found,
            vec![
                ("chrome:Default".to_string(), "Personal".to_string()),
                ("chrome:Profile 1".to_string(), "Work".to_string()),
            ]
        );
    }

    #[test]
    fn without_local_state_the_default_profile_is_probed() {
        let tmp = TempDir::new();
        let roots = tmp.roots();
        write(&chrome_dir(&roots).join("Default/Bookmarks"), BOOKMARKS);

        let ids: Vec<String> = discover(&roots).into_iter().map(|s| s.id).collect();

        assert_eq!(ids, vec!["chrome:Default"]);
    }

    #[test]
    fn a_user_data_dir_holding_the_profile_itself_is_one_source() {
        let tmp = TempDir::new();
        let roots = tmp.roots();
        write(&chrome_dir(&roots).join("Bookmarks"), BOOKMARKS);

        let ids: Vec<String> = discover(&roots).into_iter().map(|s| s.id).collect();

        assert_eq!(ids, vec!["chrome:"]);
    }

    #[test]
    fn ids_resolve_only_to_discovered_profiles() {
        let tmp = TempDir::new();
        let roots = tmp.roots();
        write(&chrome_dir(&roots).join("Default/Bookmarks"), BOOKMARKS);

        assert!(crate::find(&roots, "chrome:Default").is_some());
        assert!(crate::find(&roots, "chrome:../Default").is_none());
        assert!(crate::find(&roots, "chrome:Profile 3").is_none());
    }

    #[test]
    fn account_bookmarks_are_read_alongside_local_ones() {
        let tmp = TempDir::new();
        let profile = tmp.path().join("Default");
        write(&profile.join("Bookmarks"), BOOKMARKS);
        write(
            &profile.join("AccountBookmarks"),
            r#"{ "roots": { "other": { "children": [
                { "type": "url", "name": "D", "url": "https://d.example/" }
            ] } } }"#,
        );

        let tree = read_bookmarks(&profile).unwrap();

        assert_eq!(tree.len(), 4);
        assert_eq!(tree[3], link("https://d.example/", "D"));
    }

    #[test]
    fn profile_keys_must_be_one_plain_path_segment() {
        for good in ["Default", "Profile 1", "Guest Profile"] {
            assert!(is_plain_name(good), "{good}");
        }
        for bad in ["", ".", "..", "a/b", "a\\b", "C:", "C:..", "nul\0"] {
            assert!(!is_plain_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn objects_are_counted_outside_strings_only() {
        assert_eq!(json_objects(br#"{"a": {"b": "{ \" {"}, "c": [{}]}"#), 3);
    }

    #[test]
    fn a_file_with_too_many_objects_is_refused_before_parsing() {
        let tmp = TempDir::new();
        let children = vec!["{}"; MAX_BOOKMARK_OBJECTS].join(",");
        write(
            &tmp.path().join("Bookmarks"),
            &format!(r#"{{"roots": {{"other": {{"children": [{children}]}}}}}}"#),
        );

        let err = read_bookmarks(tmp.path()).unwrap_err();

        assert!(matches!(err, ImportError::TooLarge { file } if file == "Bookmarks"));
    }

    #[test]
    fn nesting_past_the_parser_limit_is_an_error_not_a_crash() {
        let tmp = TempDir::new();
        let open = r#"{"type": "folder", "children": ["#.repeat(200);
        let close = "]}".repeat(200);
        write(
            &tmp.path().join("Bookmarks"),
            &format!(r#"{{"roots": {{"bookmark_bar": {open}{close}}}}}"#),
        );

        assert!(matches!(
            read_bookmarks(tmp.path()),
            Err(ImportError::Unreadable { .. })
        ));
    }

    #[test]
    fn opera_roots_become_folders_and_its_trash_is_skipped() {
        let tree = parse_bookmarks(
            br#"{ "roots": { "custom_root": {
                "userRoot": { "name": "My Folders", "children": [
                    { "type": "url", "name": "U", "url": "https://u.example/" } ] },
                "unsorted": { "children": [
                    { "type": "url", "name": "S", "url": "https://s.example/" } ] },
                "trash": { "children": [
                    { "type": "url", "name": "T", "url": "https://t.example/" } ] }
            } } }"#,
        )
        .unwrap();

        assert_eq!(
            tree,
            vec![
                folder("My Folders", vec![link("https://u.example/", "U")]),
                folder("Unsorted Bookmarks", vec![link("https://s.example/", "S")]),
            ]
        );
    }

    #[test]
    fn packaged_copies_are_labelled() {
        assert_eq!(variant("chrome-flatpak"), Some("Flatpak"));
        assert_eq!(variant("chromium-snap"), Some("Snap"));
        assert_eq!(variant("chrome"), None);
    }

    #[test]
    fn an_unparseable_file_names_itself() {
        let tmp = TempDir::new();
        write(&tmp.path().join("Bookmarks"), "not json");

        let err = read_bookmarks(tmp.path()).unwrap_err();

        assert!(matches!(err, ImportError::Unreadable { file, .. } if file == "Bookmarks"));
    }
}
