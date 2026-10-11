use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags};

use crate::ImportError;

const JOURNALS: [&str; 2] = ["-wal", "-journal"];
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);
const QUERY_BUDGET: Duration = Duration::from_secs(30);

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A private copy of a browser's SQLite database and its journals, so a
/// running browser's lock or half-written page never reaches the read. The
/// copy is opened query-only with untrusted schema objects disabled, and
/// deleted on drop.
pub(crate) struct Snapshot {
    pub conn: Connection,
    _dir: ScratchDir,
}

struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Snapshot {
    /// `None` when the database does not exist.
    pub(crate) fn copy(
        db: &Path,
        scratch: &Path,
        cap: u64,
        browser: &str,
    ) -> Result<Option<Self>, ImportError> {
        let name = db
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let unreadable = |reason: String| ImportError::Unreadable {
            file: name.clone(),
            reason,
        };

        let sources: Vec<(PathBuf, String)> = std::iter::once(String::new())
            .chain(JOURNALS.iter().map(|suffix| suffix.to_string()))
            .map(|suffix| (db.with_file_name(format!("{name}{suffix}")), suffix))
            .filter(|(path, _)| path.is_file())
            .collect();
        if !sources.iter().any(|(_, suffix)| suffix.is_empty()) {
            return Ok(None);
        }
        let total: u64 = sources
            .iter()
            .filter_map(|(path, _)| std::fs::metadata(path).ok())
            .map(|meta| meta.len())
            .sum();
        if total > cap {
            return Err(ImportError::TooLarge { file: name });
        }

        let dir = ScratchDir(make_scratch_dir(scratch).map_err(|err| unreadable(err.to_string()))?);
        let copy = dir.0.join(&name);
        for (path, suffix) in &sources {
            std::fs::copy(path, dir.0.join(format!("{name}{suffix}"))).map_err(|err| {
                if is_locked(&err) {
                    ImportError::Busy {
                        browser: browser.to_string(),
                    }
                } else {
                    unreadable(err.to_string())
                }
            })?;
        }

        let conn =
            open_untrusted(&copy, QUERY_BUDGET).map_err(|err| unreadable(err.to_string()))?;
        Ok(Some(Self { conn, _dir: dir }))
    }
}

/// Queries past `budget` are interrupted, so a crafted view cannot spin forever.
fn open_untrusted(path: &Path, budget: Duration) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    conn.pragma_update(None, "query_only", true)?;
    let started = Instant::now();
    conn.progress_handler(10_000, Some(move || started.elapsed() > budget));
    Ok(conn)
}

/// A fresh owner-only directory under `scratch`, after clearing copies an
/// interrupted import left behind.
fn make_scratch_dir(scratch: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(scratch)?;
    let now = SystemTime::now();
    for entry in std::fs::read_dir(scratch)?.flatten() {
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > STALE_AFTER);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }

    let stamp = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    let dir = scratch.join(format!(
        "{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    #[cfg(unix)]
    let builder = {
        let mut builder = std::fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
    };
    #[cfg(not(unix))]
    let builder = std::fs::DirBuilder::new();
    builder.create(&dir)?;

    Ok(dir)
}

fn is_locked(err: &std::io::Error) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    cfg!(windows)
        && matches!(
            err.raw_os_error(),
            Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn a_query_that_never_ends_is_interrupted() {
        let tmp = TempDir::new();
        let path = tmp.path().join("places.sqlite");
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE VIEW endless AS
                 WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n)
                 SELECT x FROM n;",
            )
            .unwrap();

        let conn = open_untrusted(&path, Duration::from_millis(100)).unwrap();
        let started = Instant::now();
        let result: rusqlite::Result<i64> =
            conn.query_row("SELECT count(*) FROM endless", [], |row| row.get(0));

        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
