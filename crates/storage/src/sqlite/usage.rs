use chrono::NaiveDate;
use rusqlite::{params, Connection};
use wis_core::library::usage::{Credit, UsageRow};
use wis_core::WorkspaceId;

use crate::error::StorageResult;

pub fn add(conn: &mut Connection, credits: &[Credit]) -> StorageResult<()> {
    let tx = conn.transaction()?;
    {
        let mut upsert = tx.prepare(
            "INSERT INTO site_usage (day, hour, workspace_id, site, seconds)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(day, workspace_id, site, hour) DO UPDATE SET
                 seconds = site_usage.seconds + excluded.seconds",
        )?;
        for credit in credits {
            upsert.execute(params![
                credit.day,
                credit.hour,
                credit.workspace.as_uuid().to_string(),
                credit.site,
                credit.seconds
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Rows within `from..=to`, summed across workspaces when `workspace_id` is
/// `None`.
pub fn query(
    conn: &Connection,
    workspace_id: Option<WorkspaceId>,
    from: NaiveDate,
    to: NaiveDate,
) -> StorageResult<Vec<UsageRow>> {
    let ws = workspace_id.map(|w| w.as_uuid().to_string());
    let mut statement = conn.prepare(
        "SELECT day, hour, site, SUM(seconds) FROM site_usage
         WHERE day >= ?1 AND day <= ?2 AND (?3 IS NULL OR workspace_id = ?3)
         GROUP BY day, hour, site ORDER BY day, hour, site",
    )?;
    let rows = statement.query_map(params![from, to, ws], |row| {
        Ok(UsageRow {
            day: row.get(0)?,
            hour: row.get(1)?,
            site: row.get(2)?,
            seconds: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Scoped to `workspace_id` when set; with `from`, only that local hour and
/// later go.
pub fn clear(
    conn: &Connection,
    workspace_id: Option<WorkspaceId>,
    from: Option<(NaiveDate, u8)>,
) -> StorageResult<()> {
    conn.execute(
        "DELETE FROM site_usage
         WHERE (?1 IS NULL OR workspace_id = ?1)
           AND (?2 IS NULL OR day > ?2 OR (day = ?2 AND hour >= ?3))",
        params![
            workspace_id.map(|w| w.as_uuid().to_string()),
            from.map(|(day, _)| day),
            from.map(|(_, hour)| hour)
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory DB");
        super::super::migrations::run(&mut conn).expect("migrations");
        conn
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn credit(ws: WorkspaceId, d: u32, hour: u8, site: &str, seconds: u32) -> Credit {
        Credit {
            workspace: ws,
            day: day(d),
            hour,
            site: site.to_string(),
            seconds,
        }
    }

    fn row(d: u32, hour: u8, site: &str, seconds: i64) -> UsageRow {
        UsageRow {
            day: day(d),
            hour,
            site: site.to_string(),
            seconds,
        }
    }

    #[test]
    fn credits_accumulate_per_day_hour_and_site() {
        let mut conn = conn();
        let ws = WorkspaceId::new();
        add(&mut conn, &[credit(ws, 10, 9, "a.example", 30)]).unwrap();
        add(
            &mut conn,
            &[
                credit(ws, 10, 9, "a.example", 15),
                credit(ws, 10, 14, "a.example", 5),
            ],
        )
        .unwrap();

        assert_eq!(
            query(&conn, Some(ws), day(10), day(10)).unwrap(),
            vec![row(10, 9, "a.example", 45), row(10, 14, "a.example", 5)]
        );
    }

    #[test]
    fn a_range_sums_workspaces_unless_scoped() {
        let mut conn = conn();
        let (a, b) = (WorkspaceId::new(), WorkspaceId::new());
        add(
            &mut conn,
            &[
                credit(a, 9, 8, "x.example", 10),
                credit(b, 9, 8, "x.example", 5),
                credit(a, 11, 8, "x.example", 7),
                credit(a, 12, 8, "x.example", 1),
            ],
        )
        .unwrap();

        assert_eq!(
            query(&conn, None, day(9), day(11)).unwrap(),
            vec![row(9, 8, "x.example", 15), row(11, 8, "x.example", 7)]
        );
        assert_eq!(
            query(&conn, Some(b), day(1), day(31)).unwrap(),
            vec![row(9, 8, "x.example", 5)]
        );
    }

    #[test]
    fn clearing_from_an_hour_keeps_what_came_before() {
        let mut conn = conn();
        let (a, b) = (WorkspaceId::new(), WorkspaceId::new());
        add(
            &mut conn,
            &[
                credit(a, 9, 23, "x.example", 1),
                credit(a, 10, 8, "x.example", 1),
                credit(a, 10, 9, "x.example", 1),
                credit(a, 11, 0, "x.example", 1),
                credit(b, 11, 0, "x.example", 1),
            ],
        )
        .unwrap();

        clear(&conn, Some(a), Some((day(10), 9))).unwrap();

        assert_eq!(
            query(&conn, Some(a), day(1), day(31)).unwrap(),
            vec![row(9, 23, "x.example", 1), row(10, 8, "x.example", 1)]
        );
        assert_eq!(query(&conn, Some(b), day(1), day(31)).unwrap().len(), 1);

        clear(&conn, None, None).unwrap();
        assert!(query(&conn, None, day(1), day(31)).unwrap().is_empty());
    }
}
