//! Netscape bookmarks-file (`bookmarks.html`) parsing and serialization.
//!
//! This module is the pure core: a lenient scanner over the quasi-HTML format
//! every browser exports (Chrome, Firefox, Safari), and the serializer for
//! Wis's own export. The shell owns all I/O (file dialogs, reading, storage).
//!
//! Wis folders do not nest, so nested `<DL>` levels flatten into one
//! `" / "`-joined folder name per path. Folder references travel as indexes
//! into [`ParsedBookmarks::folders`], never as storage ids.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{WisError, WisResult};

/// Cap on an imported bookmarks file, rejected before parsing.
pub const MAX_BOOKMARKS_FILE_BYTES: usize = 16 * 1024 * 1024;
/// Cap on how many bookmarks one import may create.
pub const MAX_IMPORT_BOOKMARKS: usize = 20_000;
/// Cap on how many folders one import may create.
pub const MAX_IMPORT_FOLDERS: usize = 5_000;

/// Titles and folder names are clamped to this many characters.
const NAME_CAP: usize = 1024;
/// URLs beyond this are dropped.
const MAX_URL_BYTES: usize = 8 * 1024;
/// Levels beyond this stop being named (frames still balance).
const MAX_NESTING: usize = 32;

/// One parsed bookmark. `folder` indexes into [`ParsedBookmarks::folders`]
/// (`None` = unfiled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedBookmark {
    pub folder: Option<usize>,
    pub url: String,
    pub title: String,
    /// Creation time as unix seconds.
    pub added_at: Option<i64>,
}

/// Flattened content of a Netscape bookmarks file: ordered folder names and
/// the bookmarks that reference them by index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedBookmarks {
    pub folders: Vec<String>,
    pub bookmarks: Vec<ParsedBookmark>,
}

/// Parse a Netscape bookmarks file. The input is untrusted: only http(s)
/// `HREF`s are kept, text is entity-decoded and clamped, unknown markup is
/// skipped, and the bookmark count is capped.
pub fn parse_netscape(source: &str) -> WisResult<ParsedBookmarks> {
    if source.len() > MAX_BOOKMARKS_FILE_BYTES {
        return Err(WisError::InvalidInput {
            message: "bookmarks file is too large".to_string(),
        });
    }

    // ASCII-lowercased copy for case-insensitive scanning; byte offsets stay
    // aligned with `source` (multi-byte characters are untouched).
    let lower = source.to_ascii_lowercase();

    let mut out = ParsedBookmarks::default();
    let mut stack: Vec<Option<usize>> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut folder_index: HashMap<String, usize> = HashMap::new();
    let mut pending_folder: Option<String> = None;
    let mut at = 0;

    for (pos, token) in scan_tokens(&lower) {
        if pos < at {
            continue;
        }

        match token {
            Token::FolderName => {
                // <H3 ...>name</H3>
                let Some(open_end) = find_from(&lower, pos, ">") else {
                    break;
                };
                let Some(close) = find_from(&lower, open_end + 1, "</h3") else {
                    break;
                };

                let name = clamp(&decode_entities(&source[open_end + 1..close]));
                pending_folder = (!name.is_empty()).then_some(name);
                at = close + 4;
            }
            Token::ListOpen => {
                let frame = match pending_folder.take() {
                    Some(name) if stack.len() < MAX_NESTING => {
                        path.push(name);
                        let joined = clamp(&path.join(" / "));
                        let index = *folder_index.entry(joined.clone()).or_insert_with(|| {
                            out.folders.push(joined);
                            out.folders.len() - 1
                        });
                        if out.folders.len() > MAX_IMPORT_FOLDERS {
                            return Err(WisError::InvalidInput {
                                message: format!(
                                    "bookmarks file holds more than {MAX_IMPORT_FOLDERS} folders"
                                ),
                            });
                        }

                        Some(index)
                    }
                    _ => None,
                };
                stack.push(frame);
                at = pos + 3;
            }
            Token::ListClose => {
                if let Some(frame) = stack.pop() {
                    if frame.is_some() {
                        path.pop();
                    }
                }
                at = pos + 4;
            }
            Token::Anchor => {
                let Some(open_end) = find_from(&lower, pos, ">") else {
                    break;
                };
                let Some(close) = find_from(&lower, open_end + 1, "</a") else {
                    break;
                };

                if let Some(url) = attr_value(source, &lower, pos, open_end, "href")
                    .as_deref()
                    .and_then(accept_web_url)
                {
                    let added_at = attr_value(source, &lower, pos, open_end, "add_date")
                        .and_then(|v| v.trim().parse::<i64>().ok());
                    let text = clamp(&decode_entities(&source[open_end + 1..close]));
                    let title = if text.is_empty() { url.clone() } else { text };
                    out.bookmarks.push(ParsedBookmark {
                        folder: stack.iter().rev().find_map(|f| *f),
                        url,
                        title,
                        added_at,
                    });
                    if out.bookmarks.len() > MAX_IMPORT_BOOKMARKS {
                        return Err(WisError::InvalidInput {
                            message: format!(
                                "bookmarks file holds more than {MAX_IMPORT_BOOKMARKS} bookmarks"
                            ),
                        });
                    }
                }

                at = close + 3;
            }
        }
    }

    Ok(out)
}

/// Serialize folders and bookmarks as a Netscape bookmarks file (unfiled
/// entries first, then each folder as one flat `<DL>` level).
pub fn to_netscape(export: &ParsedBookmarks) -> String {
    let mut out = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
         <!-- This is an automatically generated file.\n\
         \x20    It will be read and overwritten.\n\
         \x20    DO NOT EDIT! -->\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n\
         <H1>Bookmarks</H1>\n\
         <DL><p>\n",
    );
    for bm in export.bookmarks.iter().filter(|b| b.folder.is_none()) {
        push_anchor(&mut out, 1, bm);
    }
    for (index, name) in export.folders.iter().enumerate() {
        out.push_str(&format!("    <DT><H3>{}</H3>\n    <DL><p>\n", encode(name)));
        for bm in export.bookmarks.iter().filter(|b| b.folder == Some(index)) {
            push_anchor(&mut out, 2, bm);
        }
        out.push_str("    </DL><p>\n");
    }

    out.push_str("</DL><p>\n");
    out
}

#[derive(Clone, Copy)]
enum Token {
    Anchor,
    FolderName,
    ListOpen,
    ListClose,
}

/// All interesting tag positions in document order.
fn scan_tokens(lower: &str) -> Vec<(usize, Token)> {
    let candidates = [
        (Token::Anchor, "<a"),
        (Token::FolderName, "<h3"),
        (Token::ListOpen, "<dl"),
        (Token::ListClose, "</dl"),
    ];
    let mut tokens: Vec<(usize, Token)> = Vec::new();
    for (token, tag) in candidates {
        let mut from = 0;
        while let Some(pos) = find_from(lower, from, tag) {
            // Reject prefix matches like `<abbr`, `<h3x` or `<dlist`.
            let boundary = lower.as_bytes().get(pos + tag.len());
            if matches!(
                boundary,
                Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') | None
            ) {
                tokens.push((pos, token));
            }

            from = pos + 1;
        }
    }

    tokens.sort_by_key(|(pos, _)| *pos);
    tokens
}

fn find_from(haystack: &str, from: usize, needle: &str) -> Option<usize> {
    haystack.get(from..)?.find(needle).map(|i| from + i)
}

/// Value of `name="..."` (or single-quoted) inside the tag span
/// `[tag_start, tag_end)`, entity-decoded.
fn attr_value(
    source: &str,
    lower: &str,
    tag_start: usize,
    tag_end: usize,
    name: &str,
) -> Option<String> {
    let tag = lower.get(tag_start..tag_end)?;
    let needle = format!("{name}=");
    // Require whitespace before the attribute name, so `data-href=` or an
    // occurrence inside another attribute's value cannot match.
    let mut search = 0;
    let attr_at = loop {
        let found = tag.get(search..)?.find(&needle)? + search;
        if matches!(
            tag.as_bytes().get(found.wrapping_sub(1)),
            Some(b' ' | b'\t' | b'\r' | b'\n')
        ) {
            break found + needle.len();
        }

        search = found + 1;
    };

    let quote = *tag.as_bytes().get(attr_at)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }

    let value_start = tag_start + attr_at + 1;
    let value_end = find_from(lower, value_start, char::from(quote).to_string().as_str())?;
    if value_end > tag_end {
        return None;
    }

    Some(decode_entities(&source[value_start..value_end]))
}

/// Keep only http(s) URLs with a host, canonicalized. Never re-schemes, so
/// `javascript:`, `file:`, `place:` and friends are dropped here.
fn accept_web_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() > MAX_URL_BYTES {
        return None;
    }

    let parsed = Url::parse(trimmed).ok()?;

    (matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some())
        .then(|| parsed.to_string())
}

fn clamp(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_CAP)
        .collect()
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.as_bytes().iter().take(12).position(|&b| b == b';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };

        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };

        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }

    out.push_str(rest);
    out
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }

    out
}

fn push_anchor(out: &mut String, depth: usize, bm: &ParsedBookmark) {
    let indent = "    ".repeat(depth);
    let add_date = bm
        .added_at
        .map(|ts| format!(" ADD_DATE=\"{ts}\""))
        .unwrap_or_default();
    out.push_str(&format!(
        "{indent}<DT><A HREF=\"{}\"{add_date}>{}</A>\n",
        encode(&bm.url),
        encode(&bm.title)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME_STYLE: &str = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<!-- This is an automatically generated file. -->
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3 ADD_DATE="1700000000" PERSONAL_TOOLBAR_FOLDER="true">Bar</H3>
    <DL><p>
        <DT><A HREF="https://example.com/" ADD_DATE="1700000000" ICON="data:image/png;base64,xx">Example &amp; Co</A>
        <DT><H3>Dev</H3>
        <DL><p>
            <DT><A HREF="https://rust-lang.org/">Rust</A>
        </DL><p>
    </DL><p>
    <DT><A HREF="https://unfiled.example/">Unfiled</A>
    <DT><A HREF="javascript:alert(1)">Bookmarklet</A>
</DL><p>
"#;

    #[test]
    fn parses_nested_export_flattening_folders() {
        let parsed = parse_netscape(CHROME_STYLE).expect("parses");
        assert_eq!(parsed.folders, vec!["Bar", "Bar / Dev"]);
        assert_eq!(
            parsed.bookmarks,
            vec![
                ParsedBookmark {
                    folder: Some(0),
                    url: "https://example.com/".into(),
                    title: "Example & Co".into(),
                    added_at: Some(1_700_000_000),
                },
                ParsedBookmark {
                    folder: Some(1),
                    url: "https://rust-lang.org/".into(),
                    title: "Rust".into(),
                    added_at: None,
                },
                ParsedBookmark {
                    folder: None,
                    url: "https://unfiled.example/".into(),
                    title: "Unfiled".into(),
                    added_at: None,
                },
            ]
        );
    }

    #[test]
    fn drops_non_web_schemes() {
        for href in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "place:sort=8",
            "chrome://settings",
            "ftp://example.com/",
        ] {
            let source = format!(r#"<DL><p><DT><A HREF="{href}">x</A></DL><p>"#);
            let parsed = parse_netscape(&source).expect("parses");
            assert!(parsed.bookmarks.is_empty(), "kept {href}");
        }
    }

    #[test]
    fn decodes_entities_and_falls_back_to_url_title() {
        let source = concat!(
            r##"<DL><p><DT><A HREF="https://a.example/?x=1&amp;y=2">A &lt;b&gt; &#233; &#x41;</A>"##,
            "\n",
            r#"<DT><A HREF='https://b.example/'>   </A></DL><p>"#,
        );
        let parsed = parse_netscape(source).expect("parses");
        assert_eq!(parsed.bookmarks[0].url, "https://a.example/?x=1&y=2");
        assert_eq!(parsed.bookmarks[0].title, "A <b> \u{e9} A");
        assert_eq!(parsed.bookmarks[1].title, "https://b.example/");
    }

    #[test]
    fn reuses_folder_indexes_for_repeated_names() {
        let source = r#"<DL><p>
            <DT><H3>Work</H3>
            <DL><p><DT><A HREF="https://one.example/">One</A></DL><p>
            <DT><H3>Work</H3>
            <DL><p><DT><A HREF="https://two.example/">Two</A></DL><p>
        </DL><p>"#;
        let parsed = parse_netscape(source).expect("parses");
        assert_eq!(parsed.folders, vec!["Work"]);
        assert_eq!(parsed.bookmarks[0].folder, Some(0));
        assert_eq!(parsed.bookmarks[1].folder, Some(0));
    }

    #[test]
    fn tolerates_malformed_input_without_panicking() {
        for source in [
            "",
            "not html at all",
            "<DL><p><DT><A HREF=\"https://x.example/\">unclosed",
            "<DT><A HREF=broken>no quotes</A>",
            "</DL></DL><H3>orphan",
            "<DL><DL><DL><A HREF=\"https://deep.example/\">d</A>",
        ] {
            let parsed = parse_netscape(source).expect("must stay total");
            assert!(parsed.bookmarks.len() <= 1);
        }
    }

    #[test]
    fn rejects_oversized_imports() {
        let mut source = String::from("<DL><p>");
        for i in 0..=MAX_IMPORT_BOOKMARKS {
            source.push_str(&format!("<DT><A HREF=\"https://e.example/{i}\">b</A>\n"));
        }
        source.push_str("</DL><p>");
        let err = parse_netscape(&source).expect_err("over cap");
        assert!(matches!(err, WisError::InvalidInput { .. }));
    }

    #[test]
    fn survives_entity_before_multibyte_text() {
        let source = "<DL><p><DT><A HREF=\"https://x.example/\">&0123456789étail</A></DL><p>";
        let parsed = parse_netscape(source).expect("must stay total");
        assert_eq!(parsed.bookmarks[0].title, "&0123456789étail");
    }

    #[test]
    fn deep_nesting_stays_bounded() {
        let mut source = String::new();
        for i in 0..10_000 {
            source.push_str(&format!("<DT><H3>f{i}</H3>\n<DL><p>\n"));
        }
        source.push_str("<DT><A HREF=\"https://deep.example/\">d</A>\n");
        let parsed = parse_netscape(&source).expect("parses");
        assert!(parsed.folders.len() <= MAX_NESTING);
        assert!(parsed.folders.iter().all(|f| f.chars().count() <= NAME_CAP));
        assert_eq!(parsed.bookmarks.len(), 1);
    }

    #[test]
    fn rejects_too_many_folders() {
        let mut source = String::from("<DL><p>");
        for i in 0..=MAX_IMPORT_FOLDERS {
            source.push_str(&format!("<DT><H3>f{i}</H3><DL><p></DL><p>"));
        }
        let err = parse_netscape(&source).expect_err("over folder cap");
        assert!(matches!(err, WisError::InvalidInput { .. }));
    }

    #[test]
    fn drops_oversized_urls_and_reads_whitespace_split_anchors() {
        let long = format!("https://x.example/{}", "a".repeat(9000));
        let source = format!(
            "<DL><p><DT><A HREF=\"{long}\">big</A>\n<DT><A\nhref=\"https://ok.example/\">ok</A>\n<DT><A HREF=\"https://ctrl.example/\">a\u{7}b</A></DL><p>"
        );
        let parsed = parse_netscape(&source).expect("parses");
        assert_eq!(parsed.bookmarks.len(), 2);
        assert_eq!(parsed.bookmarks[0].url, "https://ok.example/");
        assert_eq!(parsed.bookmarks[1].title, "ab");
    }

    #[test]
    fn round_trips_through_serializer() {
        let original = ParsedBookmarks {
            folders: vec!["Work".into(), "Work / Sub \"quoted\"".into()],
            bookmarks: vec![
                ParsedBookmark {
                    folder: None,
                    url: "https://plain.example/".into(),
                    title: "Plain & simple <tag>".into(),
                    added_at: Some(1_700_000_000),
                },
                ParsedBookmark {
                    folder: Some(0),
                    url: "https://work.example/".into(),
                    title: "Work item".into(),
                    added_at: None,
                },
                ParsedBookmark {
                    folder: Some(1),
                    url: "https://sub.example/".into(),
                    title: "Sub item".into(),
                    added_at: Some(-1),
                },
            ],
        };
        let html = to_netscape(&original);
        let reparsed = parse_netscape(&html).expect("round trip parses");
        assert_eq!(reparsed, original);
    }
}
