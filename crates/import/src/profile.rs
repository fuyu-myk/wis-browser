use std::path::{Path, PathBuf};

pub(crate) const MAX_PROFILES: usize = 64;
const PROFILE_NAME_CAP: usize = 64;

pub(crate) fn display_name(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(PROFILE_NAME_CAP)
        .collect()
}

pub(crate) fn under(base: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(base.to_path_buf(), |dir, part| dir.join(part))
}

/// Packaging that installs a second copy, read from the source key's suffix.
pub(crate) fn variant(key: &str) -> Option<&'static str> {
    if key.ends_with("-flatpak") {
        Some("Flatpak")
    } else if key.ends_with("-snap") {
        Some("Snap")
    } else if key.ends_with("-store") {
        Some("Microsoft Store")
    } else {
        None
    }
}

/// A Microsoft Store install's package directory.
#[cfg(windows)]
pub(crate) fn store_package(local: &Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(local.join("Packages"))
        .ok()?
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .map(|entry| entry.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_copies_are_labelled() {
        assert_eq!(variant("chrome-flatpak"), Some("Flatpak"));
        assert_eq!(variant("chromium-snap"), Some("Snap"));
        assert_eq!(variant("firefox-store"), Some("Microsoft Store"));
        assert_eq!(variant("chrome"), None);
    }

    #[test]
    fn display_names_are_trimmed_capped_and_printable() {
        assert_eq!(display_name("  Work\u{7} "), "Work");
        assert_eq!(display_name(&"x".repeat(100)).len(), 64);
    }
}
