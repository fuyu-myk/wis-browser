//! Bookmark trees read from other browsers, flattened into Wis's one-level
//! folders the same way [`crate::parse_netscape`] names nested HTML folders.

use std::collections::{HashMap, HashSet};

use crate::error::{WisError, WisResult};
use crate::library::netscape::{
    accept_web_url, clamp, ParsedBookmark, ParsedBookmarks, MAX_IMPORT_BOOKMARKS,
    MAX_IMPORT_FOLDERS, MAX_NESTING,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkNode {
    Folder {
        name: String,
        children: Vec<BookmarkNode>,
    },
    Link {
        url: String,
        title: String,
    },
}

/// Top-level links are unfiled. Folders are named by their `" / "`-joined
/// path and created only when a link lands in them. Repeats of the same
/// (folder, url, title) are dropped, matching the storage dedup key.
pub fn flatten(nodes: &[BookmarkNode]) -> WisResult<ParsedBookmarks> {
    let mut out = ParsedBookmarks::default();
    let mut folder_index = HashMap::new();
    let mut seen = HashSet::new();
    let mut stack = vec![(nodes.iter(), None::<String>, 0usize)];

    while let Some((siblings, path, depth)) = stack.last_mut() {
        let Some(node) = siblings.next() else {
            stack.pop();
            continue;
        };

        match node {
            BookmarkNode::Folder { name, children } => {
                let name = clamp(name);
                let (child_path, child_depth) = if name.is_empty() || *depth >= MAX_NESTING {
                    (path.clone(), *depth)
                } else {
                    let joined = match path.as_deref() {
                        Some(parent) => clamp(&format!("{parent} / {name}")),
                        None => name,
                    };
                    (Some(joined), *depth + 1)
                };
                stack.push((children.iter(), child_path, child_depth));
            }
            BookmarkNode::Link { url, title } => {
                let Some(url) = accept_web_url(url) else {
                    continue;
                };
                let folder = path
                    .as_deref()
                    .map(|p| folder_for(&mut out, &mut folder_index, p))
                    .transpose()?;
                let title = clamp(title);
                let title = if title.is_empty() { url.clone() } else { title };
                if !seen.insert((folder, url.clone(), title.clone())) {
                    continue;
                }

                out.bookmarks.push(ParsedBookmark {
                    folder,
                    url,
                    title,
                    added_at: None,
                });
                if out.bookmarks.len() > MAX_IMPORT_BOOKMARKS {
                    return Err(WisError::InvalidInput {
                        message: format!("more than {MAX_IMPORT_BOOKMARKS} bookmarks to import"),
                    });
                }
            }
        }
    }

    Ok(out)
}

fn folder_for(
    out: &mut ParsedBookmarks,
    index: &mut HashMap<String, usize>,
    path: &str,
) -> WisResult<usize> {
    if let Some(&existing) = index.get(path) {
        return Ok(existing);
    }
    if out.folders.len() == MAX_IMPORT_FOLDERS {
        return Err(WisError::InvalidInput {
            message: format!("more than {MAX_IMPORT_FOLDERS} folders to import"),
        });
    }

    out.folders.push(path.to_string());
    index.insert(path.to_string(), out.folders.len() - 1);
    Ok(out.folders.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::netscape::{ParsedBookmark, MAX_IMPORT_BOOKMARKS, MAX_NESTING};

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

    fn bookmark(folder: Option<usize>, url: &str, title: &str) -> ParsedBookmark {
        ParsedBookmark {
            folder,
            url: url.to_string(),
            title: title.to_string(),
            added_at: None,
        }
    }

    #[test]
    fn nested_folders_flatten_into_joined_names() {
        let tree = vec![
            folder(
                "Bookmarks bar",
                vec![
                    link("https://a.example/", "A"),
                    folder("Dev", vec![link("https://b.example/", "B")]),
                ],
            ),
            link("https://c.example/", "C"),
        ];

        let parsed = flatten(&tree).unwrap();

        assert_eq!(parsed.folders, vec!["Bookmarks bar", "Bookmarks bar / Dev"]);
        assert_eq!(
            parsed.bookmarks,
            vec![
                bookmark(Some(0), "https://a.example/", "A"),
                bookmark(Some(1), "https://b.example/", "B"),
                bookmark(None, "https://c.example/", "C"),
            ]
        );
    }

    #[test]
    fn same_path_reuses_one_folder() {
        let tree = vec![
            folder("Work", vec![link("https://a.example/", "A")]),
            folder("Work", vec![link("https://b.example/", "B")]),
        ];

        let parsed = flatten(&tree).unwrap();

        assert_eq!(parsed.folders, vec!["Work"]);
        assert!(parsed.bookmarks.iter().all(|b| b.folder == Some(0)));
    }

    #[test]
    fn only_web_links_survive_and_empty_folders_are_not_created() {
        let tree = vec![
            folder("Mobile bookmarks", vec![]),
            folder(
                "Scripts",
                vec![
                    link("javascript:alert(1)", "bookmarklet"),
                    link("file:///etc/passwd", "local"),
                ],
            ),
            link("http://ok.example/", ""),
        ];

        let parsed = flatten(&tree).unwrap();

        assert!(parsed.folders.is_empty());
        assert_eq!(
            parsed.bookmarks,
            vec![bookmark(None, "http://ok.example/", "http://ok.example/")]
        );
    }

    #[test]
    fn unnamed_folders_pass_their_links_to_the_parent() {
        let tree = vec![folder(
            "Bar",
            vec![folder("  ", vec![link("https://a.example/", "A")])],
        )];

        let parsed = flatten(&tree).unwrap();

        assert_eq!(parsed.folders, vec!["Bar"]);
        assert_eq!(parsed.bookmarks[0].folder, Some(0));
    }

    #[test]
    fn names_are_trimmed_and_stripped_of_control_characters() {
        let tree = vec![folder(
            " Re\u{7}ad ",
            vec![link(" https://a.example/ ", " T\nitle ")],
        )];

        let parsed = flatten(&tree).unwrap();

        assert_eq!(parsed.folders, vec!["Read"]);
        assert_eq!(parsed.bookmarks[0].title, "Title");
        assert_eq!(parsed.bookmarks[0].url, "https://a.example/");
    }

    #[test]
    fn exact_repeats_are_dropped_but_the_same_url_elsewhere_is_kept() {
        let tree = vec![
            link("https://a.example/", "A"),
            link("https://a.example/", "A"),
            folder("Bar", vec![link("https://a.example/", "A")]),
        ];

        let parsed = flatten(&tree).unwrap();

        assert_eq!(
            parsed.bookmarks,
            vec![
                bookmark(None, "https://a.example/", "A"),
                bookmark(Some(0), "https://a.example/", "A"),
            ]
        );
    }

    #[test]
    fn repeats_do_not_count_toward_the_cap() {
        let tree: Vec<BookmarkNode> = (0..=MAX_IMPORT_BOOKMARKS)
            .map(|_| link("https://same.example/", "x"))
            .collect();

        assert_eq!(flatten(&tree).unwrap().bookmarks.len(), 1);
    }

    #[test]
    fn too_many_bookmarks_is_an_error() {
        let tree: Vec<BookmarkNode> = (0..=MAX_IMPORT_BOOKMARKS)
            .map(|i| link(&format!("https://x.example/{i}"), "x"))
            .collect();

        assert!(flatten(&tree).is_err());
    }

    #[test]
    fn levels_past_the_nesting_cap_are_not_named() {
        let mut node = link("https://deep.example/", "deep");
        for _ in 0..40 {
            node = folder("d", vec![node]);
        }

        let parsed = flatten(&[node]).unwrap();

        assert_eq!(parsed.folders.len(), 1);
        assert_eq!(parsed.folders[0].split(" / ").count(), MAX_NESTING);
    }
}
