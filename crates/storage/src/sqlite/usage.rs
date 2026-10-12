use chrono::NaiveDate;
use rusqlite::{params, Connection};
use wis_core::usage::Credit;
use wis_core::WorkspaceId;

use crate::error::StorageResult;
use crate::usage::UsageEntry;

pub fn add(conn: &mut Connection, credits: &[Credit]) -> StorageResult<()> {
    let tx = conn.transaction()?;
    {
        let mut upsert = tx.prepare(
            "INSERT INTO site_usage (workspace_id, day, site, seconds) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(day, workspace_id, site) DO UPDATE SET
                 seconds = site_usage.seconds + excluded.seconds",
        )?;
        for credit in credits {
            upsert.execute(params![
                credit.workspace.as_uuid().to_string(),
                credit.day,
                credit.site,
                credit.seconds
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Per day and site within `from..=to`, summed across workspaces when
/// `workspace_id` is `None`. Busiest first within each day.
pub fn query(
    conn: &Connection,
    workspace_id: Option<WorkspaceId>,
    from: NaiveDate,
    to: NaiveDate,
) -> StorageResult<Vec<UsageEntry>> {
    let ws = workspace_id.map(|w| w.as_uuid().to_string());
    let mut statement = conn.prepare(
        "SELECT day, site, SUM(seconds) FROM site_usage
         WHERE day >= ?1 AND day <= ?2 AND (?3 IS NULL OR workspace_id = ?3)
         GROUP BY day, site ORDER BY day DESC, SUM(seconds) DESC, site",
    )?;
    let rows = statement.query_map(params![from, to, ws], |row| {
        Ok(UsageEntry {
            day: row.get(0)?,
            site: row.get(1)?,
            seconds: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Scoped to `workspace_id` when set; with `from`, only that day and later go.
pub fn clear(
    conn: &Connection,
    workspace_id: Option<WorkspaceId>,
    from: Option<NaiveDate>,
) -> StorageResult<()> {
    conn.execute(
        "DELETE FROM site_usage
         WHERE (?1 IS NULL OR workspace_id = ?1) AND (?2 IS NULL OR day >= ?2)",
        params![workspace_id.map(|w| w.as_uuid().to_string()), from],
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

    fn credit(ws: WorkspaceId, d: u32, site: &str, seconds: u32) -> Credit {
        Credit {
            workspace: ws,
            day: day(d),
            site: site.to_string(),
            seconds,
        }
    }

    fn entry(d: u32, site: &str, seconds: i64) -> UsageEntry {
        UsageEntry {
            day: day(d),
            site: site.to_string(),
            seconds,
        }
    }

    #[test]
    fn credits_accumulate_per_day_and_site() {
        let mut conn = conn();
        let ws = WorkspaceId::new();
        add(&mut conn, &[credit(ws, 10, "a.example", 30)]).unwrap();
        add(
            &mut conn,
            &[
                credit(ws, 10, "a.example", 15),
                credit(ws, 10, "b.example", 90),
            ],
        )
        .unwrap();

        assert_eq!(
            query(&conn, Some(ws), day(10), day(10)).unwrap(),
            vec![entry(10, "b.example", 90), entry(10, "a.example", 45)]
        );
    }

    #[test]
    fn a_range_sums_workspaces_unless_scoped() {
        let mut conn = conn();
        let (a, b) = (WorkspaceId::new(), WorkspaceId::new());
        add(
            &mut conn,
            &[
                credit(a, 9, "x.example", 10),
                credit(b, 9, "x.example", 5),
                credit(a, 11, "x.example", 7),
                credit(a, 12, "x.example", 1),
            ],
        )
        .unwrap();

        assert_eq!(
            query(&conn, None, day(9), day(11)).unwrap(),
            vec![entry(11, "x.example", 7), entry(9, "x.example", 15)]
        );
        assert_eq!(
            query(&conn, Some(b), day(1), day(31)).unwrap(),
            vec![entry(9, "x.example", 5)]
        );
    }

    #[test]
    fn clearing_honours_the_workspace_and_the_first_day() {
        let mut conn = conn();
        let (a, b) = (WorkspaceId::new(), WorkspaceId::new());
        add(
            &mut conn,
            &[
                credit(a, 9, "x.example", 1),
                credit(a, 11, "x.example", 1),
                credit(b, 11, "x.example", 1),
            ],
        )
        .unwrap();

        clear(&conn, Some(a), Some(day(10))).unwrap();

        assert_eq!(
            query(&conn, Some(a), day(1), day(31)).unwrap(),
            vec![entry(9, "x.example", 1)]
        );
        assert_eq!(query(&conn, Some(b), day(1), day(31)).unwrap().len(), 1);

        clear(&conn, None, None).unwrap();
        assert!(query(&conn, None, day(1), day(31)).unwrap().is_empty());
    }
}
