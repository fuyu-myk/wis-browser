//! Pure download-manager core: filename sanitisation, collision-safe
//! unique-ification, and download status transitions.
//!
//! Suggested filenames arrive from the network (Content-Disposition headers,
//! URL paths) and must never influence where a file lands. The shell always
//! writes into the user's downloads directory; these helpers reduce a hostile
//! suggestion to a plain leaf name first, so a path can only ever be
//! `<downloads dir>/<sanitised leaf>`.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{WisError, WisResult};

/// Byte cap for sanitised filenames. APFS/HFS+ allow 255 bytes per component;
/// the headroom leaves room for the ` (n)` collision suffix.
pub const MAX_FILENAME_BYTES: usize = 200;

/// Name used when sanitisation leaves nothing usable.
pub const FALLBACK_FILENAME: &str = "download";

/// Extensions longer than this are treated as part of the name (a dot inside
/// a sentence), not a real file extension.
const MAX_EXTENSION_BYTES: usize = 16;

/// ` (n)` suffixes tried before falling back to a UUID suffix, so collision
/// resolution always terminates.
const MAX_COLLISION_ATTEMPTS: u32 = 1000;

/// Lifecycle of a download. Starts `InProgress`; `Completed`, `Failed`, and
/// `Cancelled` are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    /// Whether the download can no longer change state.
    pub fn is_terminal(self) -> bool {
        !matches!(self, DownloadStatus::InProgress)
    }

    /// Status after the engine reports the download finished. The shell
    /// treats an `Err` (e.g. a finish racing a cancel) as "keep current".
    pub fn finished(self, success: bool) -> WisResult<DownloadStatus> {
        self.transition(if success {
            DownloadStatus::Completed
        } else {
            DownloadStatus::Failed
        })
    }

    /// Status after the user cancels the download.
    pub fn cancelled(self) -> WisResult<DownloadStatus> {
        self.transition(DownloadStatus::Cancelled)
    }

    fn transition(self, next: DownloadStatus) -> WisResult<DownloadStatus> {
        if self.is_terminal() {
            return Err(WisError::InvalidInput {
                message: format!("download is already {self:?}; cannot become {next:?}"),
            });
        }

        Ok(next)
    }
}

/// Reduce a server-suggested filename to a safe leaf name: keep only the
/// final path component, drop control and invisible formatting characters,
/// replace the characters Windows refuses, strip leading dots (no surprise-
/// hidden files) and trailing dots/spaces, cap the length (preserving the
/// extension), defuse Windows device names, and fall back to
/// [`FALLBACK_FILENAME`] when nothing usable remains.
pub fn sanitize_filename(suggested: &str) -> String {
    let leaf = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = leaf
        .chars()
        .filter(|&c| !c.is_control() && !is_format_char(c))
        .map(|c| match c {
            ':' | '<' | '>' | '"' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned
        .trim_start_matches(['.', ' '])
        .trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return FALLBACK_FILENAME.to_string();
    }

    let name = truncate_preserving_extension(trimmed, MAX_FILENAME_BYTES);
    if is_device_name(&name) {
        format!("_{name}")
    } else {
        name
    }
}

/// Unicode format characters (category Cf): invisible, and able to reorder
/// or hide what a name shows.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// A name Windows opens as a device.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    let numbered = |prefix: &str| {
        let mut chars = stem.chars();
        let head: String = chars.by_ref().take(3).collect();
        head.eq_ignore_ascii_case(prefix)
            && matches!(
                chars.next(),
                Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}')
            )
            && chars.next().is_none()
    };

    ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|device| stem.eq_ignore_ascii_case(device))
        || numbered("COM")
        || numbered("LPT")
}

/// First filename derived from `filename` that `taken` reports as free,
/// counting up Finder-style: `name.ext`, `name (1).ext`, `name (2).ext`, ...
/// After [`MAX_COLLISION_ATTEMPTS`] a UUID suffix guarantees termination.
/// The caller still creates the file with `create_new` semantics, so even a
/// check/create race can never overwrite an existing file.
pub fn unique_filename(filename: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(filename) {
        return filename.to_string();
    }

    let (stem, ext) = split_extension(filename);
    for n in 1..=MAX_COLLISION_ATTEMPTS {
        let candidate = with_suffix(stem, ext, &format!(" ({n})"));
        if !taken(&candidate) {
            return candidate;
        }
    }

    with_suffix(stem, ext, &format!(" ({})", uuid::Uuid::new_v4()))
}

fn with_suffix(stem: &str, ext: Option<&str>, suffix: &str) -> String {
    match ext {
        Some(e) => format!("{stem}{suffix}.{e}"),
        None => format!("{stem}{suffix}"),
    }
}

/// The name a download Wis fetches itself is offered under: the
/// `Content-Disposition` name, else the URL's last segment, sanitised, with an
/// extension from the content type when it has none.
pub fn suggested_filename(disposition: Option<&str>, url: &Url, mime: Option<&str>) -> String {
    let name = disposition
        .and_then(disposition_filename)
        .or_else(|| url_filename(url))
        .unwrap_or_default();
    let name = sanitize_filename(&name);
    match (split_extension(&name).1, mime.and_then(extension_for_mime)) {
        (None, Some(ext)) => {
            truncate_preserving_extension(&format!("{name}.{ext}"), MAX_FILENAME_BYTES)
        }
        _ => name,
    }
}

/// The Mark of the Web for a download from `url`: the `Zone.Identifier`
/// stream Windows reads to treat a file as from the internet. A private
/// download names no source.
pub fn zone_identifier(url: &Url, private: bool) -> String {
    let source = if private {
        "about:internet".to_string()
    } else {
        let mut source = url.clone();
        let _ = source.set_username("");
        let _ = source.set_password(None);
        source.set_fragment(None);
        source.to_string()
    };

    format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={source}\r\n")
}

/// The name a `Content-Disposition` value gives, `filename*` (RFC 8187)
/// over `filename`.
fn disposition_filename(value: &str) -> Option<String> {
    let params = disposition_params(value);
    let param = |key: &str| {
        params
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    };

    param("filename*")
        .and_then(decode_ext_value)
        .or_else(|| param("filename").map(str::to_string))
        .filter(|name| !name.trim().is_empty())
}

/// The `name=value` parameters after the disposition type. A quoted value may
/// hold `;` and backslash escapes.
fn disposition_params(value: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let Some((_, mut rest)) = value.split_once(';') else {
        return params;
    };
    loop {
        rest = rest.trim_start_matches([' ', '\t', ';']);
        let Some(at) = rest.find(['=', ';']) else {
            break;
        };
        if rest[at..].starts_with(';') {
            rest = &rest[at + 1..];
            continue;
        }

        let name = rest[..at].trim().to_string();
        let after = rest[at + 1..].trim_start();
        let (value, next) = match after.strip_prefix('"') {
            Some(quoted) => quoted_string(quoted),
            None => {
                let end = after.find(';').unwrap_or(after.len());
                (after[..end].trim_end().to_string(), &after[end..])
            }
        };
        params.push((name, value));
        rest = next;
    }

    params
}

/// A quoted string's value, opening quote already taken, and what follows its
/// closing quote.
fn quoted_string(text: &str) -> (String, &str) {
    let mut value = String::new();
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return (value, &text[at + 1..]),
            '\\' => value.extend(chars.next().map(|(_, escaped)| escaped)),
            _ => value.push(c),
        }
    }

    (value, "")
}

/// An RFC 8187 `charset'language'percent-encoded` value, in UTF-8 or
/// ISO-8859-1, the two charsets the RFC requires.
fn decode_ext_value(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, '\'');
    let (charset, _language, encoded) = (parts.next()?, parts.next()?, parts.next()?);
    let bytes: Vec<u8> = percent_encoding::percent_decode_str(encoded).collect();
    if charset.eq_ignore_ascii_case("utf-8") {
        String::from_utf8(bytes).ok()
    } else if charset.eq_ignore_ascii_case("iso-8859-1") {
        Some(bytes.into_iter().map(char::from).collect())
    } else {
        None
    }
}

fn url_filename(url: &Url) -> Option<String> {
    let segment = url
        .path_segments()?
        .rev()
        .find(|segment| !segment.is_empty())?;
    Some(
        percent_encoding::percent_decode_str(segment)
            .decode_utf8_lossy()
            .into_owned(),
    )
}

/// The extension a name without one takes from its content type.
fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let essence = mime.split(';').next()?.trim().to_ascii_lowercase();
    let ext = match essence.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "image/heic" => "heic",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/ogg" => "ogv",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/aac" => "aac",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/flac" => "flac",
        "audio/webm" => "weba",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "application/json" => "json",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "text/html" => "html",
        _ => return None,
    };

    Some(ext)
}

/// Split `name.ext` at the last dot. No extension when there is no dot, the
/// dot is leading, or the candidate extension is empty, oversized, or
/// contains spaces. `.tar.<comp>` compound extensions stay together so
/// `a.tar.gz` collides to `a (1).tar.gz`.
fn split_extension(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= MAX_EXTENSION_BYTES
                && !ext.contains(' ') =>
        {
            match stem.strip_suffix(".tar") {
                Some(inner) if !inner.is_empty() => (inner, Some(&name[inner.len() + 1..])),
                _ => (stem, Some(ext)),
            }
        }
        _ => (name, None),
    }
}

fn truncate_preserving_extension(name: &str, max_bytes: usize) -> String {
    if name.len() <= max_bytes {
        return name.to_string();
    }

    let (stem, ext) = split_extension(name);
    let ext_overhead = ext.map(|e| e.len() + 1).unwrap_or(0);
    let stem_budget = max_bytes.saturating_sub(ext_overhead).max(1);
    let truncated = truncate_at_char_boundary(stem, stem_budget);
    if truncated.is_empty() {
        return FALLBACK_FILENAME.to_string();
    }

    match ext {
        Some(e) => format!("{truncated}.{e}"),
        None => truncated.to_string(),
    }
}

fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }

    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_plain_names() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("photo 2026.jpeg"), "photo 2026.jpeg");
    }

    #[test]
    fn sanitize_strips_path_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("/etc/shadow"), "shadow");
        assert_eq!(sanitize_filename("..\\..\\evil.exe"), "evil.exe");
        assert_eq!(sanitize_filename("C:\\Windows\\boot.ini"), "boot.ini");
    }

    #[test]
    fn sanitize_strips_control_chars_and_colons() {
        assert_eq!(sanitize_filename("a\u{0}b\u{1f}c.txt"), "abc.txt");
        assert_eq!(sanitize_filename("a:b.txt"), "a_b.txt");
        assert_eq!(sanitize_filename("line\nbreak.txt"), "linebreak.txt");
    }

    #[test]
    fn sanitize_strips_invisible_format_chars() {
        assert_eq!(
            sanitize_filename("invoice\u{202E}fdp.exe"),
            "invoicefdp.exe"
        );
        assert_eq!(sanitize_filename("a\u{200B}b\u{FEFF}.txt"), "ab.txt");
    }

    #[test]
    fn sanitize_replaces_characters_windows_refuses() {
        assert_eq!(sanitize_filename("a<b>|c?*\".txt"), "a_b__c___.txt");
    }

    #[test]
    fn sanitize_defuses_windows_device_names() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_filename("com1.tar.gz"), "_com1.tar.gz");
        assert_eq!(sanitize_filename("console.txt"), "console.txt");
        assert_eq!(sanitize_filename("COM0.txt"), "_COM0.txt");
        assert_eq!(sanitize_filename("lpt\u{b9}"), "_lpt\u{b9}");
        assert_eq!(sanitize_filename("com10"), "com10");
        assert_eq!(sanitize_filename("\u{ff}\u{ff}.txt"), "\u{ff}\u{ff}.txt");
    }

    #[test]
    fn sanitize_strips_leading_dots_and_trailing_dots_spaces() {
        assert_eq!(sanitize_filename(".bashrc"), "bashrc");
        assert_eq!(sanitize_filename("..hidden"), "hidden");
        assert_eq!(sanitize_filename("name. "), "name");
        assert_eq!(sanitize_filename(" padded.txt "), "padded.txt");
    }

    #[test]
    fn sanitize_falls_back_when_nothing_remains() {
        assert_eq!(sanitize_filename(""), FALLBACK_FILENAME);
        assert_eq!(sanitize_filename("   "), FALLBACK_FILENAME);
        assert_eq!(sanitize_filename(".."), FALLBACK_FILENAME);
        assert_eq!(sanitize_filename("///"), FALLBACK_FILENAME);
        assert_eq!(sanitize_filename("\u{0}\u{1}"), FALLBACK_FILENAME);
    }

    #[test]
    fn sanitize_caps_length_preserving_extension() {
        let long = format!("{}.pdf", "x".repeat(500));
        let out = sanitize_filename(&long);
        assert!(out.len() <= MAX_FILENAME_BYTES);
        assert!(out.ends_with(".pdf"));

        let no_ext = "y".repeat(500);
        let out = sanitize_filename(&no_ext);
        assert_eq!(out.len(), MAX_FILENAME_BYTES);
    }

    #[test]
    fn sanitize_truncates_on_char_boundaries() {
        let long = "\u{00e9}".repeat(300);
        let out = sanitize_filename(&long);
        assert!(out.len() <= MAX_FILENAME_BYTES);
        assert!(out.chars().all(|c| c == '\u{00e9}'));
    }

    #[test]
    fn unique_returns_free_name_unchanged() {
        assert_eq!(unique_filename("a.txt", |_| false), "a.txt");
    }

    #[test]
    fn unique_counts_up_on_collision() {
        let taken = |name: &str| matches!(name, "a.txt" | "a (1).txt");
        assert_eq!(unique_filename("a.txt", taken), "a (2).txt");
    }

    #[test]
    fn unique_suffixes_before_the_extension() {
        assert_eq!(unique_filename("a.txt", |n| n == "a.txt"), "a (1).txt");
        assert_eq!(unique_filename("noext", |n| n == "noext"), "noext (1)");
    }

    #[test]
    fn unique_keeps_tar_compound_extensions_together() {
        assert_eq!(
            unique_filename("a.tar.gz", |n| n == "a.tar.gz"),
            "a (1).tar.gz"
        );
    }

    #[test]
    fn unique_terminates_when_everything_is_taken() {
        let name = unique_filename("a.txt", |_| true);
        assert!(name.starts_with("a ("));
        assert!(name.ends_with(").txt"));
    }

    #[test]
    fn status_transitions_from_in_progress() {
        assert_eq!(
            DownloadStatus::InProgress.finished(true).unwrap(),
            DownloadStatus::Completed
        );
        assert_eq!(
            DownloadStatus::InProgress.finished(false).unwrap(),
            DownloadStatus::Failed
        );
        assert_eq!(
            DownloadStatus::InProgress.cancelled().unwrap(),
            DownloadStatus::Cancelled
        );
    }

    #[test]
    fn status_terminal_states_reject_transitions() {
        for terminal in [
            DownloadStatus::Completed,
            DownloadStatus::Failed,
            DownloadStatus::Cancelled,
        ] {
            assert!(terminal.is_terminal());
            assert!(terminal.finished(true).is_err());
            assert!(terminal.cancelled().is_err());
        }
        assert!(!DownloadStatus::InProgress.is_terminal());
    }

    #[test]
    fn status_serde_uses_snake_case() {
        let json = serde_json::to_string(&DownloadStatus::InProgress).unwrap();
        assert_eq!(json, "\"in_progress\"");
        let back: DownloadStatus = serde_json::from_str("\"cancelled\"").unwrap();
        assert_eq!(back, DownloadStatus::Cancelled);
    }

    fn named(disposition: Option<&str>, url: &str, mime: Option<&str>) -> String {
        suggested_filename(disposition, &url::Url::parse(url).unwrap(), mime)
    }

    #[test]
    fn the_encoded_disposition_name_wins() {
        let disposition =
            "attachment; filename=\"fallback.pdf\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf";
        assert_eq!(
            named(Some(disposition), "https://example.com/get", None),
            "résumé.pdf"
        );
        let latin = "attachment; filename*=iso-8859-1'en'%A3%20rates.txt";
        assert_eq!(
            named(Some(latin), "https://example.com/get", None),
            "£ rates.txt"
        );
    }

    #[test]
    fn a_quoted_or_plain_disposition_name_is_read() {
        let quoted = "attachment; filename=\"report; \\\"final\\\".pdf\"";
        assert_eq!(
            named(Some(quoted), "https://example.com/get", None),
            "report; _final_.pdf"
        );
        let plain = "attachment;filename=data.csv";
        assert_eq!(
            named(Some(plain), "https://example.com/get", None),
            "data.csv"
        );
    }

    #[test]
    fn an_unreadable_encoded_name_falls_back_to_the_plain_one() {
        let disposition = "attachment; filename*=klingon''abc.txt; filename=plain.txt";
        assert_eq!(
            named(Some(disposition), "https://example.com/get", None),
            "plain.txt"
        );
    }

    #[test]
    fn without_a_disposition_name_the_url_names_the_file() {
        let url = "https://example.com/files/My%20Song.mp3?session=1";
        assert_eq!(named(Some("inline"), url, None), "My Song.mp3");
        assert_eq!(named(None, url, None), "My Song.mp3");
    }

    #[test]
    fn a_name_without_an_extension_takes_one_from_the_type() {
        let url = "https://cdn.example.com/photo?w=800";
        assert_eq!(named(None, url, Some("image/JPEG; q=1")), "photo.jpg");
        assert_eq!(
            named(None, "https://example.com/", Some("application/pdf")),
            "download.pdf"
        );
        assert_eq!(named(None, url, Some("application/octet-stream")), "photo");
        assert_eq!(named(None, url, None), "photo");
    }

    #[test]
    fn a_name_with_an_extension_keeps_it() {
        assert_eq!(
            named(None, "https://example.com/a.png", Some("image/jpeg")),
            "a.png"
        );
    }

    #[test]
    fn a_disposition_name_cannot_leave_its_folder() {
        let disposition = "attachment; filename=\"../../evil.exe\"";
        assert_eq!(
            named(Some(disposition), "https://example.com/get", None),
            "evil.exe"
        );
    }

    #[test]
    fn the_zone_identifier_marks_the_internet_and_the_source() {
        let url = url::Url::parse("https://user:secret@example.com/a.exe#top").unwrap();
        assert_eq!(
            zone_identifier(&url, false),
            "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/a.exe\r\n"
        );
    }

    #[test]
    fn a_private_zone_identifier_names_no_source() {
        let url = url::Url::parse("https://example.com/a.exe").unwrap();
        assert_eq!(
            zone_identifier(&url, true),
            "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=about:internet\r\n"
        );
    }
}
