//! Pure download-manager core: filename sanitisation, collision-safe
//! unique-ification, and download status transitions.
//!
//! Suggested filenames arrive from the network (Content-Disposition headers,
//! URL paths) and must never influence where a file lands. The shell always
//! writes into the user's downloads directory; these helpers reduce a hostile
//! suggestion to a plain leaf name first, so a path can only ever be
//! `<downloads dir>/<sanitised leaf>`.

use serde::{Deserialize, Serialize};

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
/// final path component, drop control characters, neutralise the legacy HFS
/// `:` separator, strip leading dots (no surprise-hidden files) and trailing
/// dots/spaces, cap the length (preserving the extension), and fall back to
/// [`FALLBACK_FILENAME`] when nothing usable remains.
pub fn sanitize_filename(suggested: &str) -> String {
    let leaf = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = leaf
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if c == ':' { '_' } else { c })
        .collect();
    let trimmed = cleaned
        .trim_start_matches(['.', ' '])
        .trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return FALLBACK_FILENAME.to_string();
    }

    truncate_preserving_extension(trimmed, MAX_FILENAME_BYTES)
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
}
