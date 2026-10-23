use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection};
use wis_core::library::bookmark_tree::BookmarkNode;
use wis_core::library::netscape::{
    MAX_IMPORT_BOOKMARKS, MAX_IMPORT_FOLDERS, MAX_URL_BYTES, NAME_CAP,
};

use crate::fs::read_capped;
#[cfg(windows)]
use crate::profile::store_package;
use crate::profile::{display_name, under, variant, MAX_PROFILES};
use crate::snapshot::Snapshot;
use crate::visits;
use crate::{Browser, HistoryItem, ImportError, Roots, Source};

const PLACES: &str = "places.sqlite";
const PROFILES_INI_CAP: usize = 1024 * 1024;
const MAX_PLACES_DB_BYTES: u64 = 512 * 1024 * 1024;
const MAX_BOOKMARK_ROWS: usize = 4 * (MAX_IMPORT_BOOKMARKS + MAX_IMPORT_FOLDERS);
const MAX_BOOKMARK_TEXT_BYTES: usize = 64 * 1024 * 1024;
const MAX_FOLDER_DEPTH: usize = 32;
const TYPE_BOOKMARK: i64 = 1;
const TYPE_FOLDER: i64 = 2;

pub(crate) fn discover(roots: &Roots) -> Vec<Source> {
    data_dirs(roots)
        .into_iter()
        .flat_map(|(browser, key, dir)| profiles(browser, key, &dir))
        .collect()
}

pub(crate) fn read_bookmarks(
    profile: &Path,
    browser: &str,
    scratch: &Path,
) -> Result<Vec<BookmarkNode>, ImportError> {
    let Some(snapshot) = open_places(profile, browser, scratch)? else {
        return Ok(Vec::new());
    };
    let rows = bookmark_rows(&snapshot.conn)?;

    Ok(BookmarkTree::new(&rows).into_roots())
}

pub(crate) fn read_history(
    profile: &Path,
    browser: &str,
    scratch: &Path,
    since: DateTime<Utc>,
) -> Result<Vec<HistoryItem>, ImportError> {
    let Some(snapshot) = open_places(profile, browser, scratch)? else {
        return Ok(Vec::new());
    };

    visits::query(
        &snapshot.conn,
        "SELECT CASE WHEN octet_length(url) <= ?3 THEN url END, substr(title, 1, ?4),
                visit_count, last_visit_date
         FROM moz_places WHERE hidden = 0 AND visit_count > 0 AND last_visit_date >= ?1
         ORDER BY last_visit_date DESC LIMIT ?2",
        since,
        0,
    )
    .map_err(unreadable)
}

fn open_places(
    profile: &Path,
    browser: &str,
    scratch: &Path,
) -> Result<Option<Snapshot>, ImportError> {
    Snapshot::copy(&profile.join(PLACES), scratch, MAX_PLACES_DB_BYTES, browser)
}

fn unreadable(err: rusqlite::Error) -> ImportError {
    ImportError::Unreadable {
        file: PLACES.to_string(),
        reason: err.to_string(),
    }
}

/// Install defaults first, then `profiles.ini` order.
fn profiles(browser: Browser, key: &str, data_dir: &Path) -> Vec<Source> {
    let Ok(Some(bytes)) = read_capped(&data_dir.join("profiles.ini"), PROFILES_INI_CAP) else {
        return Vec::new();
    };
    let ini = parse_ini(&String::from_utf8_lossy(&bytes));
    let defaults: HashSet<&str> = ini
        .iter()
        .filter(|(section, _)| section.starts_with("Install"))
        .filter_map(|(_, keys)| keys.get("Default").map(String::as_str))
        .collect();
    let mut listed: Vec<&HashMap<String, String>> = ini
        .iter()
        .filter(|(section, _)| section.starts_with("Profile"))
        .map(|(_, keys)| keys)
        .collect();
    listed.sort_by_key(|keys| {
        !keys
            .get("Path")
            .is_some_and(|path| defaults.contains(&path.as_str()))
    });

    let mut seen = HashSet::new();
    listed
        .into_iter()
        .take(MAX_PROFILES)
        .filter_map(|keys| {
            let path = keys.get("Path")?;
            let relative = keys.get("IsRelative").is_none_or(|flag| flag != "0");
            let dir = resolve(data_dir, path, relative)?;
            if !dir.join(PLACES).is_file() || !seen.insert(dir.clone()) {
                return None;
            }
            let name = display_name(keys.get("Name").map_or("", String::as_str));
            let folder = dir
                .file_name()
                .map(|n| display_name(&n.to_string_lossy()))
                .unwrap_or_default();
            Some(Source {
                id: profile_id(key, path, relative),
                browser,
                variant: variant(key),
                profile: if name.is_empty() { folder } else { name },
                bookmarks: true,
                history: true,
                dir,
            })
        })
        .collect()
}

fn parse_ini(text: &str) -> Vec<(String, HashMap<String, String>)> {
    let mut sections: Vec<(String, HashMap<String, String>)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_string(), HashMap::new()));
        } else if let (Some((key, value)), Some((_, keys))) =
            (line.split_once('='), sections.last_mut())
        {
            keys.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    sections
}

/// Absolute paths are hashed so no home directory reaches the webview.
fn profile_id(key: &str, path: &str, relative: bool) -> String {
    if relative {
        return format!("{key}:{path}");
    }
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{key}:abs-{:016x}", hasher.finish())
}

/// Relative paths stay inside `data_dir`; absolute ones need `IsRelative=0`.
fn resolve(data_dir: &Path, path: &str, relative: bool) -> Option<PathBuf> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    if relative {
        candidate
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
            .then(|| data_dir.join(candidate))
    } else {
        local_absolute(candidate).then(|| candidate.to_path_buf())
    }
}

/// Drive paths only, so a profile path cannot reach a network share or device.
#[cfg(windows)]
fn local_absolute(path: &Path) -> bool {
    use std::path::Prefix;
    path.is_absolute()
        && matches!(
            path.components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        )
}

#[cfg(not(windows))]
fn local_absolute(path: &Path) -> bool {
    path.is_absolute()
}

struct Row {
    id: i64,
    parent: i64,
    kind: i64,
    guid: String,
    title: String,
    url: Option<String>,
}

/// Values are capped inside SQLite and the total text is budgeted, so a small
/// file cannot expand in memory. Unreadable cells are repaired or skipped.
fn bookmark_rows(conn: &Connection) -> Result<Vec<Row>, ImportError> {
    let too_large = || ImportError::TooLarge {
        file: PLACES.to_string(),
    };
    let mut statement = conn
        .prepare(
            "SELECT b.id, b.parent, b.type, substr(b.guid, 1, 12), substr(b.title, 1, ?2),
                    CASE WHEN octet_length(p.url) <= ?3 THEN p.url END
             FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk
             ORDER BY b.parent, b.position LIMIT ?1",
        )
        .map_err(unreadable)?;
    let mut rows = statement
        .query(params![
            MAX_BOOKMARK_ROWS as i64 + 1,
            NAME_CAP as i64,
            MAX_URL_BYTES as i64
        ])
        .map_err(unreadable)?;

    let (mut out, mut text_bytes) = (Vec::new(), 0usize);
    while let Some(row) = rows.next().map_err(unreadable)? {
        let Some(id) = integer(row, 0) else {
            continue;
        };
        let entry = Row {
            id,
            parent: integer(row, 1).unwrap_or(-1),
            kind: integer(row, 2).unwrap_or(0),
            guid: text(row, 3).unwrap_or_default(),
            title: text(row, 4).unwrap_or_default(),
            url: text(row, 5),
        };
        text_bytes += entry.title.len() + entry.url.as_ref().map_or(0, String::len);
        if out.len() == MAX_BOOKMARK_ROWS || text_bytes > MAX_BOOKMARK_TEXT_BYTES {
            return Err(too_large());
        }

        out.push(entry);
    }

    Ok(out)
}

fn integer(row: &rusqlite::Row, index: usize) -> Option<i64> {
    row.get::<_, Option<i64>>(index).ok().flatten()
}

/// Invalid UTF-8 is replaced rather than failing the import.
fn text(row: &rusqlite::Row, index: usize) -> Option<String> {
    match row.get_ref(index).ok()? {
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => None,
    }
}

struct BookmarkTree<'a> {
    rows: &'a [Row],
    children: HashMap<i64, Vec<&'a Row>>,
    visited: HashSet<i64>,
}

impl<'a> BookmarkTree<'a> {
    fn new(rows: &'a [Row]) -> Self {
        let mut children: HashMap<i64, Vec<&Row>> = HashMap::new();
        for row in rows {
            children.entry(row.parent).or_default().push(row);
        }
        Self {
            rows,
            children,
            visited: HashSet::new(),
        }
    }

    /// Menu items sit at the top level and the toolbar and unfiled roots become
    /// folders, as in Firefox's own HTML export, plus the mobile root. Tags are
    /// skipped.
    fn into_roots(mut self) -> Vec<BookmarkNode> {
        let root = |guid: &str| {
            self.rows
                .iter()
                .find(|row| row.guid == guid && row.kind == TYPE_FOLDER)
                .map(|row| row.id)
        };
        let menu = root("menu________");
        let folders = [
            (root("toolbar_____"), "Bookmarks Toolbar"),
            (root("unfiled_____"), "Other Bookmarks"),
            (root("mobile______"), "Mobile Bookmarks"),
        ];

        let mut tree = menu.map(|id| self.nodes(id, 0)).unwrap_or_default();
        for (id, name) in folders {
            if let Some(id) = id {
                let children = self.nodes(id, 1);
                tree.push(BookmarkNode::Folder {
                    name: name.to_string(),
                    children,
                });
            }
        }

        tree
    }

    fn nodes(&mut self, folder: i64, depth: usize) -> Vec<BookmarkNode> {
        if !self.visited.insert(folder) {
            return Vec::new();
        }

        let mut out = Vec::new();
        for row in self.children.remove(&folder).unwrap_or_default() {
            match row.kind {
                TYPE_BOOKMARK => out.extend(link(row)),
                TYPE_FOLDER if depth < MAX_FOLDER_DEPTH => {
                    let children = self.nodes(row.id, depth + 1);
                    out.push(BookmarkNode::Folder {
                        name: row.title.clone(),
                        children,
                    });
                }
                TYPE_FOLDER => out.extend(self.links_below(row.id)),
                _ => {}
            }
        }

        out
    }

    fn links_below(&mut self, folder: i64) -> Vec<BookmarkNode> {
        let mut out = Vec::new();
        let mut pending = vec![folder];
        while let Some(id) = pending.pop() {
            if !self.visited.insert(id) {
                continue;
            }
            for row in self.children.remove(&id).unwrap_or_default() {
                match row.kind {
                    TYPE_BOOKMARK => out.extend(link(row)),
                    TYPE_FOLDER => pending.push(row.id),
                    _ => {}
                }
            }
        }

        out
    }
}

fn link(row: &Row) -> Option<BookmarkNode> {
    row.url.as_ref().map(|url| BookmarkNode::Link {
        url: url.clone(),
        title: row.title.clone(),
    })
}

#[cfg(target_os = "macos")]
fn data_dirs(roots: &Roots) -> Vec<(Browser, &'static str, PathBuf)> {
    let base = &roots.config;
    vec![
        (Browser::Firefox, "firefox", under(base, &["Firefox"])),
        (Browser::Zen, "zen", under(base, &["zen"])),
        (Browser::LibreWolf, "librewolf", under(base, &["librewolf"])),
    ]
}

#[cfg(windows)]
fn data_dirs(roots: &Roots) -> Vec<(Browser, &'static str, PathBuf)> {
    let roaming = &roots.config;
    let mut dirs = vec![
        (
            Browser::Firefox,
            "firefox",
            under(roaming, &["Mozilla", "Firefox"]),
        ),
        (Browser::Zen, "zen", under(roaming, &["zen"])),
        (
            Browser::LibreWolf,
            "librewolf",
            under(roaming, &["librewolf"]),
        ),
    ];
    if let Some(package) = store_package(&roots.local_data, "Mozilla.Firefox_") {
        dirs.push((
            Browser::Firefox,
            "firefox-store",
            under(&package, &["LocalCache", "Roaming", "Mozilla", "Firefox"]),
        ));
    }

    dirs
}

#[cfg(target_os = "linux")]
fn data_dirs(roots: &Roots) -> Vec<(Browser, &'static str, PathBuf)> {
    let home = &roots.home;
    let flatpak = home.join(".var").join("app");
    vec![
        (
            Browser::Firefox,
            "firefox",
            under(home, &[".mozilla", "firefox"]),
        ),
        (
            Browser::Firefox,
            "firefox-xdg",
            under(&roots.config, &["mozilla", "firefox"]),
        ),
        (
            Browser::Firefox,
            "firefox-snap",
            under(home, &["snap", "firefox", "common", ".mozilla", "firefox"]),
        ),
        (
            Browser::Firefox,
            "firefox-flatpak",
            under(&flatpak, &["org.mozilla.firefox", ".mozilla", "firefox"]),
        ),
        (Browser::Zen, "zen", under(home, &[".zen"])),
        (
            Browser::Zen,
            "zen-flatpak",
            under(&flatpak, &["app.zen_browser.zen", ".zen"]),
        ),
        (
            Browser::LibreWolf,
            "librewolf",
            under(home, &[".librewolf"]),
        ),
        (
            Browser::LibreWolf,
            "librewolf-flatpak",
            under(&flatpak, &["io.gitlab.librewolf-community", ".librewolf"]),
        ),
    ]
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn data_dirs(_roots: &Roots) -> Vec<(Browser, &'static str, PathBuf)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use rusqlite::{params, Connection};
    use wis_core::library::bookmark_tree::flatten;
    use wis_core::library::netscape::MAX_URL_BYTES;

    use super::*;
    use crate::test_support::{write, TempDir};

    fn firefox_dir(roots: &Roots) -> PathBuf {
        data_dirs(roots)
            .into_iter()
            .find(|(_, key, _)| *key == "firefox")
            .map(|(_, _, dir)| dir)
            .unwrap()
    }

    /// Firefox's own root rows. The connection stays open so writes stay in the WAL.
    fn places_db(profile: &Path) -> Connection {
        std::fs::create_dir_all(profile).unwrap();
        let conn = Connection::open(profile.join("places.sqlite")).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR,
                 visit_count INTEGER DEFAULT 0, hidden INTEGER DEFAULT 0 NOT NULL,
                 last_visit_date INTEGER);
             CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER,
                 parent INTEGER, position INTEGER, title LONGVARCHAR, guid TEXT);
             INSERT INTO moz_bookmarks (id, type, parent, position, title, guid) VALUES
                 (1, 2, 0, 0, '', 'root________'),
                 (2, 2, 1, 0, 'menu', 'menu________'),
                 (3, 2, 1, 1, 'toolbar', 'toolbar_____'),
                 (4, 2, 1, 2, 'tags', 'tags________'),
                 (5, 2, 1, 3, 'unfiled', 'unfiled_____'),
                 (6, 2, 1, 4, 'mobile', 'mobile______');",
        )
        .unwrap();
        conn
    }

    fn place(conn: &Connection, id: i64, url: &str, visits: i64, hidden: bool, at: DateTime<Utc>) {
        conn.execute(
            "INSERT INTO moz_places (id, url, title, visit_count, hidden, last_visit_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                url,
                format!("T{id}"),
                visits,
                hidden,
                at.timestamp_micros()
            ],
        )
        .unwrap();
    }

    fn mark(conn: &Connection, id: i64, kind: i64, fk: Option<i64>, parent: i64, title: &str) {
        conn.execute(
            "INSERT INTO moz_bookmarks (id, type, fk, parent, position, title, guid)
             VALUES (?1, ?2, ?3, ?4, ?1, ?5, ?6)",
            params![id, kind, fk, parent, title, format!("g{id}")],
        )
        .unwrap();
    }

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

    #[test]
    fn ini_sections_keep_their_order_and_keys() {
        let ini = parse_ini("[General]\nVersion=2\n\n[Profile0]\nName = a\nPath=P/a\n; note\n");

        assert_eq!(ini.len(), 2);
        assert_eq!(ini[1].0, "Profile0");
        assert_eq!(ini[1].1.get("Name").map(String::as_str), Some("a"));
        assert_eq!(ini[1].1.get("Path").map(String::as_str), Some("P/a"));
    }

    #[test]
    fn profile_paths_cannot_leave_the_data_dir_unless_marked_absolute() {
        let base = Path::new("/data");
        assert_eq!(
            resolve(base, "Profiles/a.default", true),
            Some(base.join("Profiles/a.default"))
        );
        assert_eq!(resolve(base, "../escape", true), None);
        assert_eq!(resolve(base, "Profiles/../../x", true), None);
        assert_eq!(resolve(base, "", true), None);
        assert_eq!(resolve(base, "/abs", true), None);

        let absolute = std::env::temp_dir().join("custom-profile");
        let absolute = absolute.to_string_lossy();
        assert!(resolve(base, &absolute, false).is_some());
        assert_eq!(resolve(base, "relative/only", false), None);
        assert_eq!(resolve(base, &format!("{absolute}/../x"), false), None);
    }

    #[test]
    fn install_defaults_come_first_and_profiles_need_places() {
        let tmp = TempDir::new();
        let roots = tmp.roots();
        let firefox = firefox_dir(&roots);
        write(
            &firefox.join("profiles.ini"),
            "[Profile1]\nName=default\nIsRelative=1\nPath=Profiles/b.default\nDefault=1\n\n\
             [Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/a.default-release\n\n\
             [Profile2]\nName=empty\nIsRelative=1\nPath=Profiles/c.empty\n\n\
             [Profile3]\nName=sneaky\nIsRelative=1\nPath=../outside\n\n\
             [Install4F96D1932A9F858E]\nDefault=Profiles/a.default-release\nLocked=1\n",
        );
        drop(places_db(&firefox.join("Profiles/a.default-release")));
        drop(places_db(&firefox.join("Profiles/b.default")));
        std::fs::create_dir_all(firefox.join("Profiles/c.empty")).unwrap();
        drop(places_db(&firefox.parent().unwrap().join("outside")));

        let found: Vec<(String, String, bool, bool)> = discover(&roots)
            .into_iter()
            .map(|s| (s.id, s.profile, s.bookmarks, s.history))
            .collect();

        assert_eq!(
            found,
            vec![
                (
                    "firefox:Profiles/a.default-release".to_string(),
                    "default-release".to_string(),
                    true,
                    true
                ),
                (
                    "firefox:Profiles/b.default".to_string(),
                    "default".to_string(),
                    true,
                    true
                ),
            ]
        );
    }

    #[test]
    fn bookmarks_mirror_firefoxs_own_export() {
        let tmp = TempDir::new();
        let profile = tmp.path().join("p");
        let conn = places_db(&profile);
        let now = Utc::now();
        for (id, url) in [
            (100, "https://menu.example/"),
            (101, "https://bar.example/"),
            (102, "https://dev.example/"),
            (103, "https://other.example/"),
        ] {
            place(&conn, id, url, 1, false, now);
        }
        mark(&conn, 10, 1, Some(100), 2, "Menu link");
        mark(&conn, 11, 1, Some(101), 3, "Bar link");
        mark(&conn, 12, 3, None, 3, "");
        mark(&conn, 13, 2, None, 3, "Dev");
        mark(&conn, 14, 1, Some(102), 13, "Dev link");
        mark(&conn, 15, 1, Some(103), 5, "Other link");
        mark(&conn, 16, 2, None, 4, "a-tag");
        mark(&conn, 17, 1, Some(101), 16, "");

        let tree = read_bookmarks(&profile, "Firefox", &tmp.path().join("s")).unwrap();

        assert_eq!(
            tree,
            vec![
                link("https://menu.example/", "Menu link"),
                folder(
                    "Bookmarks Toolbar",
                    vec![
                        link("https://bar.example/", "Bar link"),
                        folder("Dev", vec![link("https://dev.example/", "Dev link")]),
                    ]
                ),
                folder(
                    "Other Bookmarks",
                    vec![link("https://other.example/", "Other link")]
                ),
                folder("Mobile Bookmarks", vec![]),
            ]
        );
    }

    #[test]
    fn a_folder_loop_ends() {
        let tmp = TempDir::new();
        let profile = tmp.path().join("p");
        let conn = places_db(&profile);
        place(&conn, 100, "https://a.example/", 1, false, Utc::now());
        mark(&conn, 20, 2, None, 3, "Loop");
        mark(&conn, 21, 1, Some(100), 20, "A");
        conn.execute("UPDATE moz_bookmarks SET parent = 20 WHERE id = 3", [])
            .unwrap();

        let tree = read_bookmarks(&profile, "Firefox", &tmp.path().join("s")).unwrap();

        assert_eq!(flatten(&tree).unwrap().bookmarks.len(), 1);
    }

    #[test]
    fn links_below_the_depth_cap_are_kept() {
        let tmp = TempDir::new();
        let profile = tmp.path().join("p");
        let conn = places_db(&profile);
        place(&conn, 100, "https://deep.example/", 1, false, Utc::now());
        let mut parent = 3;
        for id in 200..250 {
            mark(&conn, id, 2, None, parent, "d");
            parent = id;
        }
        mark(&conn, 300, 1, Some(100), parent, "Deep");

        let tree = read_bookmarks(&profile, "Firefox", &tmp.path().join("s")).unwrap();

        assert_eq!(flatten(&tree).unwrap().bookmarks.len(), 1);
    }

    #[test]
    fn a_huge_url_shared_by_many_bookmarks_never_leaves_sqlite() {
        let tmp = TempDir::new();
        let profile = tmp.path().join("p");
        let conn = places_db(&profile);
        let huge = format!("https://big.example/{}", "x".repeat(MAX_URL_BYTES));
        place(&conn, 100, &huge, 1, false, Utc::now());
        for id in 10..1010 {
            mark(&conn, id, 1, Some(100), 3, "Big");
        }

        let tree = read_bookmarks(&profile, "Firefox", &tmp.path().join("s")).unwrap();

        assert_eq!(tree[0], folder("Bookmarks Toolbar", vec![]));
    }

    #[test]
    fn a_title_that_is_not_utf8_is_repaired_not_fatal() {
        let tmp = TempDir::new();
        let profile = tmp.path().join("p");
        let conn = places_db(&profile);
        place(&conn, 100, "https://a.example/", 1, false, Utc::now());
        mark(&conn, 10, 1, Some(100), 2, "");
        conn.execute(
            "UPDATE moz_bookmarks SET title = CAST(X'4142ED' AS TEXT) WHERE id = 10",
            [],
        )
        .unwrap();

        let tree = read_bookmarks(&profile, "Firefox", &tmp.path().join("s")).unwrap();

        assert_eq!(tree[0], link("https://a.example/", "AB\u{FFFD}"));
    }

    #[test]
    fn oversized_history_urls_are_skipped() {
        let tmp = TempDir::new();
        let profile = tmp.path().join("p");
        let conn = places_db(&profile);
        let huge = format!("https://big.example/{}", "x".repeat(MAX_URL_BYTES));
        place(&conn, 1, &huge, 1, false, Utc::now());
        place(&conn, 2, "https://ok.example/", 1, false, Utc::now());

        let items = read_history(
            &profile,
            "Firefox",
            &tmp.path().join("s"),
            Utc::now() - Duration::days(1),
        )
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://ok.example/");
    }

    #[test]
    fn absolute_profiles_get_opaque_ids_and_duplicates_collapse() {
        let tmp = TempDir::new();
        let roots = tmp.roots();
        let firefox = firefox_dir(&roots);
        let elsewhere = tmp.path().join("elsewhere");
        write(
            &firefox.join("profiles.ini"),
            &format!(
                "[Profile0]\nIsRelative=0\nPath={}\n\n\
                 [Profile1]\nName=p\nPath=Profiles/p\n\n\
                 [Profile2]\nName=again\nPath=Profiles/p\n\n\
                 [Profile3]\nName=dotted\nPath=./Profiles/p\n",
                elsewhere.display()
            ),
        );
        drop(places_db(&elsewhere));
        drop(places_db(&firefox.join("Profiles/p")));

        let found = discover(&roots);

        assert_eq!(found.len(), 2);
        assert!(found[0].id.starts_with("firefox:abs-"));
        assert!(!found[0].id.contains(&*elsewhere.to_string_lossy()));
        assert_eq!(found[0].profile, "elsewhere");
        assert_eq!(found[1].id, "firefox:Profiles/p");
    }

    #[cfg(windows)]
    #[test]
    fn windows_absolute_profiles_must_be_on_a_drive() {
        let base = Path::new(r"C:\data");
        assert!(resolve(base, r"C:\profiles\p", false).is_some());
        assert!(resolve(base, r"\\?\C:\profiles\p", false).is_some());
        assert_eq!(resolve(base, r"\\server\share\p", false), None);
        assert_eq!(resolve(base, r"\\.\pipe\x", false), None);
        assert_eq!(resolve(base, "C:foo", true), None);
        assert_eq!(resolve(base, r"\foo", true), None);
    }

    #[test]
    fn recent_visible_visits_come_back_newest_first() {
        let tmp = TempDir::new();
        let profile = tmp.path().join("p");
        let conn = places_db(&profile);
        let now = Utc::now();
        place(
            &conn,
            1,
            "https://a.example/",
            4,
            false,
            now - Duration::days(2),
        );
        place(
            &conn,
            2,
            "https://b.example/",
            1,
            false,
            now - Duration::days(1),
        );
        place(
            &conn,
            3,
            "https://old.example/",
            1,
            false,
            now - Duration::days(400),
        );
        place(
            &conn,
            4,
            "https://hidden.example/",
            1,
            true,
            now - Duration::days(1),
        );
        place(
            &conn,
            5,
            "https://never.example/",
            0,
            false,
            now - Duration::days(1),
        );
        place(&conn, 6, "about:config", 1, false, now - Duration::days(1));

        let items = read_history(
            &profile,
            "Firefox",
            &tmp.path().join("s"),
            now - Duration::days(180),
        )
        .unwrap();

        let found: Vec<(&str, i64)> = items
            .iter()
            .map(|i| (i.url.as_str(), i.visit_count))
            .collect();
        assert_eq!(
            found,
            vec![("https://b.example/", 1), ("https://a.example/", 4)]
        );
        let drift = items[1].visited_at - (now - Duration::days(2));
        assert!(drift.num_milliseconds().abs() < 1);
    }
}
