use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use wis_core::library::netscape::{accept_web_url, clamp, MAX_URL_BYTES, NAME_CAP};

use crate::{HistoryItem, MAX_IMPORT_HISTORY};

/// Runs `sql`, which selects `(url, title, visit_count, last visit)` with
/// `?1` as the earliest time, `?2` as the row limit, and `?3` / `?4` as the
/// URL byte and title character caps it must apply, so oversized values never
/// leave SQLite. Times are microseconds from an epoch `epoch_offset_micros`
/// before 1970. Cells of the wrong type are skipped; any other SQLite error
/// fails the read.
pub(crate) fn query(
    conn: &Connection,
    sql: &str,
    since: DateTime<Utc>,
    epoch_offset_micros: i64,
) -> rusqlite::Result<Vec<HistoryItem>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(
        params![
            since.timestamp_micros() + epoch_offset_micros,
            MAX_IMPORT_HISTORY as i64,
            MAX_URL_BYTES as i64,
            NAME_CAP as i64
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    )?;

    let now = Utc::now();
    let mut items = Vec::new();
    for row in rows {
        let (url, title, visit_count, last_visit) = match row {
            Ok(row) => row,
            Err(rusqlite::Error::FromSqlConversionFailure(..))
            | Err(rusqlite::Error::InvalidColumnType(..)) => continue,
            Err(err) => return Err(err),
        };
        let Some(url) = accept_web_url(&url) else {
            continue;
        };
        let Some(visited_at) = last_visit
            .checked_sub(epoch_offset_micros)
            .and_then(DateTime::from_timestamp_micros)
        else {
            continue;
        };
        items.push(HistoryItem {
            url,
            title: clamp(&title.unwrap_or_default()),
            visited_at: visited_at.min(now),
            visit_count: visit_count.max(1),
        });
    }

    Ok(items)
}
