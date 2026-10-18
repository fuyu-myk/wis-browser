//! An extension's files copied from a folder or unpacked from a `.zip` into a
//! folder of Wis's own, within limits and never through a link.

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use wis_core::{WisError, WisResult};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FILES: usize = 10_000;
const MAX_DEPTH: usize = 24;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_RULE_FILE_BYTES: u64 = 64 * 1024 * 1024;

fn refused(message: impl Into<String>) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

fn io_failed(err: io::Error) -> WisError {
    refused(format!("the extension could not be copied: {err}"))
}

/// What has been written so far, against the limits.
#[derive(Default)]
struct Budget {
    files: usize,
    bytes: u64,
}

impl Budget {
    fn take_entry(&mut self) -> WisResult<()> {
        self.files += 1;
        if self.files > MAX_FILES {
            return Err(refused(format!(
                "the extension has more than {MAX_FILES} files"
            )));
        }
        Ok(())
    }

    fn room(&self) -> u64 {
        MAX_BYTES - self.bytes
    }

    /// Copies at most the room left, failing past it.
    fn copy(&mut self, from: &mut impl Read, to: &Path) -> WisResult<()> {
        let mut out = fs::File::create(to).map_err(io_failed)?;
        let written = io::copy(&mut from.take(self.room() + 1), &mut out).map_err(io_failed)?;
        if written > self.room() {
            return Err(refused(format!(
                "the extension is larger than {} MB",
                MAX_BYTES / 1024 / 1024
            )));
        }

        self.bytes += written;
        Ok(())
    }
}

fn depth_ok(relative: &Path) -> WisResult<()> {
    if relative.components().count() > MAX_DEPTH {
        return Err(refused("the extension's folders nest too deeply"));
    }
    Ok(())
}

/// Opens a regular file, never through a link that replaced it after the
/// folder was read, and never waiting on a pipe.
fn open_file(path: &Path) -> WisResult<fs::File> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| refused(format!("{} could not be read as a file", path.display())))?;
    if !file.metadata().map_err(io_failed)?.is_file() {
        return Err(refused(format!("{} is not a file", path.display())));
    }
    Ok(file)
}

/// Archive litter Finder adds: the `__MACOSX` folder and `._` files.
fn is_finder_litter(relative: &Path) -> bool {
    relative.components().enumerate().any(|(depth, part)| {
        let name = part.as_os_str().to_string_lossy();
        (depth == 0 && name == "__MACOSX") || name.starts_with("._")
    })
}

/// Copies `from` into the new folder `to`, refusing links.
pub(crate) fn copy_folder(from: &Path, to: &Path) -> WisResult<()> {
    let mut budget = Budget::default();
    fs::create_dir(to).map_err(io_failed)?;
    copy_tree(from, to, Path::new(""), &mut budget)
}

fn copy_tree(from: &Path, to: &Path, relative: &Path, budget: &mut Budget) -> WisResult<()> {
    depth_ok(relative)?;
    for entry in fs::read_dir(from.join(relative)).map_err(io_failed)? {
        let entry = entry.map_err(io_failed)?;
        let kind = entry.file_type().map_err(io_failed)?;
        let path = relative.join(entry.file_name());
        if kind.is_symlink() {
            return Err(refused(format!(
                "the extension links to {}",
                path.display()
            )));
        }

        if kind.is_dir() {
            budget.take_entry()?;
            fs::create_dir(to.join(&path)).map_err(io_failed)?;
            copy_tree(from, to, &path, budget)?;
        } else if kind.is_file() {
            budget.take_entry()?;
            let mut source = open_file(&from.join(&path))?;
            budget.copy(&mut source, &to.join(&path))?;
        }
    }

    Ok(())
}

/// Unpacks `archive` into the new folder `to`, refusing links and any entry
/// that would land outside it.
pub(crate) fn unzip(archive: &Path, to: &Path) -> WisResult<()> {
    let file = fs::File::open(archive).map_err(io_failed)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| refused("that is not a .zip file"))?;
    if zip.len() > MAX_FILES {
        return Err(refused(format!(
            "the extension has more than {MAX_FILES} files"
        )));
    }

    let mut budget = Budget::default();
    fs::create_dir(to).map_err(io_failed)?;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|_| refused("the .zip file is damaged"))?;
        let shown = entry
            .name()
            .map(|name| name.into_owned())
            .unwrap_or_else(|_| "an entry".into());
        if entry.is_symlink() {
            return Err(refused(format!("the extension links to {shown}")));
        }

        let Some(relative) = entry.enclosed_name() else {
            return Err(refused(format!("{shown} would land outside the extension")));
        };
        if is_finder_litter(&relative) {
            continue;
        }

        depth_ok(&relative)?;
        let target = to.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(io_failed)?;
            continue;
        }

        budget.take_entry()?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_failed)?;
        }
        budget.copy(&mut entry, &target)?;
    }

    Ok(())
}

/// The folder holding `manifest.json`: the staged folder itself, or its only
/// subfolder, as a zip of a folder unpacks.
pub(crate) fn manifest_root(staged: &Path) -> Option<PathBuf> {
    if staged.join("manifest.json").is_file() {
        return Some(staged.to_path_buf());
    }

    let mut entries = fs::read_dir(staged)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            !name.starts_with('.') && name != "__MACOSX" && e.file_type().is_ok_and(|t| t.is_dir())
        });
    let only = entries.next()?;
    if entries.next().is_some() {
        return None;
    }

    let root = only.path();
    root.join("manifest.json").is_file().then_some(root)
}

fn read_capped(path: &Path, max: u64) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > max {
        return None;
    }
    let mut text = String::new();
    file.take(max).read_to_string(&mut text).ok()?;
    Some(text)
}

pub(crate) fn read_manifest(root: &Path) -> WisResult<serde_json::Value> {
    let text = read_capped(&root.join("manifest.json"), MAX_MANIFEST_BYTES)
        .ok_or_else(|| refused("the extension's manifest.json could not be read"))?;
    let manifest: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| refused("the extension's manifest.json is not valid JSON"))?;
    if !manifest.is_object() {
        return Err(refused("the extension's manifest.json is not valid JSON"));
    }
    Ok(manifest)
}

/// The manifest's `permissions`, as strings.
pub(crate) fn declared_permissions(manifest: &serde_json::Value) -> Vec<String> {
    manifest
        .get("permissions")
        .and_then(|p| p.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|p| p.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Blocking rules the extension turns on by default.
pub(crate) fn default_rules(root: &Path, manifest: &serde_json::Value) -> usize {
    wis_core::extensions::default_rule_count(manifest, |inside| {
        let path = root.join(inside);
        let stays_inside = Path::new(inside)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
        if !stays_inside || fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
            return None;
        }
        read_capped(&path, MAX_RULE_FILE_BYTES)
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("wis-ext-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn zip_of(path: &Path, entries: &[(&str, &str)]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, text) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn a_folder_is_copied_whole() {
        let scratch = Scratch::new();
        let from = scratch.0.join("src");
        write(&from.join("manifest.json"), "{}");
        write(&from.join("js/bg.js"), "1");
        copy_folder(&from, &scratch.0.join("out")).unwrap();
        assert_eq!(
            fs::read_to_string(scratch.0.join("out/js/bg.js")).unwrap(),
            "1"
        );
    }

    #[test]
    fn a_folder_with_a_link_is_refused() {
        let scratch = Scratch::new();
        let from = scratch.0.join("src");
        write(&from.join("manifest.json"), "{}");
        std::os::unix::fs::symlink("/etc/hosts", from.join("hosts")).unwrap();
        assert!(copy_folder(&from, &scratch.0.join("out")).is_err());
    }

    #[test]
    fn a_zip_unpacks_into_place() {
        let scratch = Scratch::new();
        let archive = scratch.0.join("ext.zip");
        zip_of(&archive, &[("manifest.json", "{}"), ("js/bg.js", "1")]);
        unzip(&archive, &scratch.0.join("out")).unwrap();
        assert_eq!(
            fs::read_to_string(scratch.0.join("out/js/bg.js")).unwrap(),
            "1"
        );
    }

    #[test]
    fn finder_metadata_is_left_out_of_a_zip() {
        let scratch = Scratch::new();
        let archive = scratch.0.join("ext.zip");
        zip_of(
            &archive,
            &[
                ("ext/manifest.json", "{}"),
                ("__MACOSX/ext/._manifest.json", "x"),
                ("ext/._bg.js", "x"),
            ],
        );
        let out = scratch.0.join("out");
        unzip(&archive, &out).unwrap();
        assert!(!out.join("__MACOSX").exists());
        assert!(!out.join("ext/._bg.js").exists());
        assert_eq!(manifest_root(&out), Some(out.join("ext")));
    }

    #[test]
    fn a_zip_entry_climbing_out_is_refused() {
        let scratch = Scratch::new();
        let archive = scratch.0.join("ext.zip");
        zip_of(&archive, &[("manifest.json", "{}"), ("../escaped.js", "1")]);
        assert!(unzip(&archive, &scratch.0.join("out")).is_err());
        assert!(!scratch.0.join("escaped.js").exists());
    }

    #[test]
    fn something_that_is_not_a_zip_is_refused() {
        let scratch = Scratch::new();
        let archive = scratch.0.join("ext.zip");
        write(&archive, "not a zip");
        assert!(unzip(&archive, &scratch.0.join("out")).is_err());
    }

    #[test]
    fn the_manifest_may_sit_one_folder_down() {
        let scratch = Scratch::new();
        write(&scratch.0.join("top/manifest.json"), "{}");
        assert_eq!(manifest_root(&scratch.0), Some(scratch.0.join("top")));

        write(&scratch.0.join("second/manifest.json"), "{}");
        assert_eq!(manifest_root(&scratch.0), None);

        write(&scratch.0.join("manifest.json"), "{}");
        assert_eq!(manifest_root(&scratch.0), Some(scratch.0.clone()));
    }

    #[test]
    fn manifests_are_read_with_their_permissions() {
        let scratch = Scratch::new();
        write(
            &scratch.0.join("manifest.json"),
            "\u{feff}{\"permissions\": [\"storage\", 3, \"tabs\"]}",
        );
        let manifest = read_manifest(&scratch.0).unwrap();
        assert_eq!(declared_permissions(&manifest), vec!["storage", "tabs"]);

        write(&scratch.0.join("manifest.json"), "[1, 2]");
        assert!(read_manifest(&scratch.0).is_err());
    }

    #[test]
    fn rule_files_are_counted_from_inside_the_extension() {
        let scratch = Scratch::new();
        write(&scratch.0.join("rules/a.json"), "[{}, {}]");
        let manifest = serde_json::json!({
            "declarative_net_request": { "rule_resources": [
                { "id": "a", "enabled": true, "path": "/rules/a.json" }
            ]}
        });
        assert_eq!(default_rules(&scratch.0, &manifest), 2);
    }
}
