//! Persistent event journal (attachment P1–P3): append-only source records.
//!
//! Adapters (MCP tool = AGY-style live events, CLI = DSH-style batch import)
//! translate their runtime events into the canonical [`AgentEvent`] model.
//! Ingestion is idempotent on `event_id`; ordering uses the monotonic
//! `sequence` column so consumers resume with `since_sequence`.

use aikd_core::AgentEvent;
use anyhow::Result;
use rusqlite::Connection;

fn map_row(row: &rusqlite::Row) -> Result<AgentEvent, rusqlite::Error> {
    Ok(AgentEvent {
        event_id: row.get(0)?,
        project_id: row.get(1)?,
        workstream: row.get(2)?,
        agent_id: row.get(3)?,
        agent_session_id: row.get(4)?,
        event_type: row.get(5)?,
        timestamp: row.get(6)?,
        source_ref: row.get(7)?,
        payload: row.get(8)?,
        sequence: row.get(9)?,
    })
}

/// Append one event. Duplicate `event_id` delivery returns the existing row
/// instead of failing (idempotent ingestion).
#[allow(clippy::too_many_arguments)]
pub fn append_event(
    conn: &Connection,
    project_id: &str,
    workstream: &str,
    agent_id: &str,
    agent_session_id: &str,
    event_type: &str,
    source_ref: &str,
    payload: &str,
) -> Result<AgentEvent> {
    // Deterministic ID = content hash, so retries dedupe naturally.
    let event_id = blake3::hash(
        format!("{project_id}\0{workstream}\0{agent_id}\0{agent_session_id}\0{event_type}\0{source_ref}\0{payload}").as_bytes(),
    )
    .to_hex()
    .to_string();
    if let Ok(ev) = conn.query_row(
        "SELECT event_id, project_id, workstream, agent_id, agent_session_id, event_type, timestamp, source_ref, payload, sequence FROM events WHERE event_id = ?1",
        rusqlite::params![event_id],
        map_row,
    ) {
        return Ok(ev);
    }
    // BEGIN IMMEDIATE serializes concurrent writers: the second blocks until
    // the first commits, so MAX(sequence)+1 always sees the latest value.
    // If the caller already holds a transaction, proceed best-effort.
    let own_tx = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
    let timestamp = chrono::Utc::now().to_rfc3339();
    let result = (|| -> Result<AgentEvent> {
        let sequence: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM events",
            [],
            |r| r.get(0),
        )?;
        // OR IGNORE: a racing duplicate event_id converges instead of failing.
        conn.execute(
            "INSERT OR IGNORE INTO events (event_id, project_id, workstream, agent_id, agent_session_id, event_type, timestamp, source_ref, payload, sequence) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![event_id, project_id, workstream, agent_id, agent_session_id, event_type, timestamp, source_ref, payload, sequence],
        )?;
        Ok(AgentEvent {
            event_id: event_id.clone(),
            project_id: project_id.into(),
            workstream: workstream.into(),
            agent_id: agent_id.into(),
            agent_session_id: agent_session_id.into(),
            event_type: event_type.into(),
            timestamp,
            source_ref: source_ref.into(),
            payload: payload.into(),
            sequence,
        })
    })();
    if own_tx {
        match result {
            Ok(ev) => {
                conn.execute_batch("COMMIT")?;
                // Re-read: a racing duplicate may have won; return the stored row.
                if let Ok(stored) = conn.query_row(
                    "SELECT event_id, project_id, workstream, agent_id, agent_session_id, event_type, timestamp, source_ref, payload, sequence FROM events WHERE event_id = ?1",
                    rusqlite::params![event_id],
                    map_row,
                ) {
                    return Ok(stored);
                }
                return Ok(ev);
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
    }
    result
}

/// List events for a (project, workstream), optionally filtered by type and
/// resumed from a sequence number. Bounded by limit.
pub fn list_events(
    conn: &Connection,
    project_id: &str,
    workstream: &str,
    event_type: Option<&str>,
    since_sequence: i64,
    limit: usize,
) -> Result<Vec<AgentEvent>> {
    let mut sql = String::from(
        "SELECT event_id, project_id, workstream, agent_id, agent_session_id, event_type, timestamp, source_ref, payload, sequence FROM events WHERE project_id=?1 AND workstream=?2 AND sequence>?3",
    );
    if event_type.is_some() {
        sql.push_str(" AND event_type=?4");
        sql.push_str(" ORDER BY sequence ASC LIMIT ?5");
    } else {
        sql.push_str(" ORDER BY sequence ASC LIMIT ?4");
    }
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(project_id.to_string()),
        Box::new(workstream.to_string()),
        Box::new(since_sequence),
    ];
    if let Some(t) = event_type {
        params.push(Box::new(t.to_string()));
    }
    params.push(Box::new(limit as i64));
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(&refs[..], map_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Highest sequence ingested (consumer resume watermark).
pub fn latest_sequence(conn: &Connection, project_id: &str, workstream: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(sequence), 0) FROM events WHERE project_id=?1 AND workstream=?2",
        rusqlite::params![project_id, workstream],
        |r| r.get(0),
    )?)
}

/// Adapter coverage: event counts per (agent, type) for diagnostics.
pub fn adapter_coverage(
    conn: &Connection,
    project_id: &str,
    workstream: &str,
) -> Result<Vec<(String, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT agent_id, event_type, COUNT(*) FROM events WHERE project_id=?1 AND workstream=?2 GROUP BY agent_id, event_type ORDER BY agent_id, event_type",
    )?;
    let rows = stmt.query_map(rusqlite::params![project_id, workstream], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Global adapter coverage across all projects and workstreams (for `/ui`).
pub fn all_adapter_coverage(conn: &Connection) -> Result<Vec<(String, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT agent_id, event_type, COUNT(*) FROM events GROUP BY agent_id, event_type ORDER BY agent_id, event_type",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn append_is_idempotent_and_ordered() {
        let conn = setup();
        let a = append_event(
            &conn,
            "/p",
            "ws",
            "agy",
            "s1",
            "tool_call",
            "src",
            "{\"tool\":\"edit\"}",
        )
        .unwrap();
        let b = append_event(
            &conn,
            "/p",
            "ws",
            "agy",
            "s1",
            "tool_call",
            "src",
            "{\"tool\":\"edit\"}",
        )
        .unwrap();
        assert_eq!(a.event_id, b.event_id);
        assert_eq!(a.sequence, b.sequence);
        let c = append_event(&conn, "/p", "ws", "agy", "s1", "tool_result", "src", "{}").unwrap();
        assert!(c.sequence > a.sequence);
        let all = list_events(&conn, "/p", "ws", None, 0, 100).unwrap();
        assert_eq!(all.len(), 2);
        let resumed = list_events(&conn, "/p", "ws", None, a.sequence, 100).unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].event_id, c.event_id);
        assert_eq!(latest_sequence(&conn, "/p", "ws").unwrap(), c.sequence);
        let cov = adapter_coverage(&conn, "/p", "ws").unwrap();
        assert_eq!(cov.len(), 2);
    }

    #[test]
    fn workstream_isolation() {
        let conn = setup();
        append_event(&conn, "/p", "ws1", "agy", "s1", "session_start", "", "{}").unwrap();
        assert!(list_events(&conn, "/p", "ws2", None, 0, 100)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn concurrent_appends_converge_with_unique_sequences() {
        // 8 threads hammering the same stream on one shared file DB.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("race.db");
        {
            let conn = Connection::open(&path).unwrap();
            super::super::schema::run_migrations(&conn).unwrap();
        }
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let barrier = barrier.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    let conn = Connection::open(&path).unwrap();
                    conn.execute_batch("PRAGMA busy_timeout=10000").unwrap();
                    barrier.wait();
                    for i in 0..10 {
                        append_event(
                            &conn,
                            "/p",
                            "ws",
                            "agy",
                            "s",
                            "tool_call",
                            &format!("t{t}-i{i}"),
                            "{\"tool\":\"edit\"}",
                        )
                        .unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let conn = Connection::open(&path).unwrap();
        let all = list_events(&conn, "/p", "ws", None, 0, 1000).unwrap();
        assert_eq!(all.len(), 80);
        let mut seqs: Vec<i64> = all.iter().map(|e| e.sequence).collect();
        seqs.sort_unstable();
        seqs.dedup();
        assert_eq!(seqs.len(), 80, "sequences must be unique under concurrency");
        assert_eq!(seqs, (1..=80).collect::<Vec<_>>());
    }
}
