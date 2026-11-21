//! Structured memory (P5): typed durable knowledge with provenance,
//! scope, confidence and lifecycle — facts, decisions, constraints,
//! discoveries, tasks and checkpoints. Raw chat stays in `conversations`.

use aikd_core::MemoryItem;
use anyhow::Result;
use rusqlite::Connection;

fn map_row(row: &rusqlite::Row) -> Result<MemoryItem, rusqlite::Error> {
    Ok(MemoryItem {
        id: row.get(0)?,
        project_path: row.get(1)?,
        workstream: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        body: row.get(5)?,
        source: row.get(6)?,
        status: row.get(7)?,
        confidence: row.get(8)?,
        supersedes: row.get(9)?,
        conflict_note: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

const COLS: &str = "id, project_path, workstream, kind, title, body, source, status, confidence, supersedes, conflict_note, created_at, updated_at";

/// Save a new memory item. IDs are UUIDs; callers may pass `supersedes`
/// to retire an older item in the same call (see [`supersede`]).
#[allow(clippy::too_many_arguments)]
pub fn save_memory(
    conn: &Connection,
    project_path: &str,
    workstream: &str,
    kind: &str,
    title: &str,
    body: &str,
    source: &str,
    confidence: f64,
) -> Result<MemoryItem> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO memory (id, project_path, workstream, kind, title, body, source, status, confidence, supersedes, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,'active',?8,NULL,?9,?9)",
        rusqlite::params![id, project_path, workstream, kind, title, body, source, confidence, now],
    )?;
    Ok(MemoryItem {
        id,
        project_path: project_path.into(),
        workstream: workstream.into(),
        kind: kind.into(),
        title: title.into(),
        body: body.into(),
        source: source.into(),
        status: "active".into(),
        confidence,
        supersedes: None,
        conflict_note: None,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Retire an item and create its replacement atomically.
#[allow(clippy::too_many_arguments)]
pub fn supersede(
    conn: &Connection,
    old_id: &str,
    project_path: &str,
    workstream: &str,
    kind: &str,
    title: &str,
    body: &str,
    source: &str,
    confidence: f64,
) -> Result<MemoryItem> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE memory SET status='superseded', updated_at=?1 WHERE id=?2",
        rusqlite::params![now, old_id],
    )?;
    let item = save_memory(conn, project_path, workstream, kind, title, body, source, confidence)?;
    conn.execute(
        "UPDATE memory SET supersedes=?1 WHERE id=?2",
        rusqlite::params![old_id, item.id],
    )?;
    Ok(MemoryItem { supersedes: Some(old_id.into()), ..item })
}

/// List memories with optional kind/status filters. Superseded items are
/// excluded unless explicitly requested.
pub fn list_memory(
    conn: &Connection,
    project_path: &str,
    workstream: Option<&str>,
    kind: Option<&str>,
    include_superseded: bool,
) -> Result<Vec<MemoryItem>> {
    let mut sql = format!("SELECT {COLS} FROM memory WHERE project_path=?1");
    if workstream.is_some() {
        sql.push_str(" AND workstream=?2");
    }
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_path.to_string())];
    if let Some(ws) = workstream {
        params.push(Box::new(ws.to_string()));
    }
    if let Some(k) = kind {
        sql.push_str(&format!(" AND kind=?{}", params.len() + 1));
        params.push(Box::new(k.to_string()));
    }
    if !include_superseded {
        sql.push_str(" AND status='active'");
    }
    sql.push_str(" ORDER BY updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(&refs[..], map_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
}

/// Active tasks for a workstream (the resumable unit of work).
pub fn active_tasks(conn: &Connection, project_path: &str, workstream: &str) -> Result<Vec<MemoryItem>> {
    list_memory(conn, project_path, Some(workstream), Some("task"), false)
}

/// Latest checkpoint for a workstream (resume point after compaction).
pub fn latest_checkpoint(
    conn: &Connection,
    project_path: &str,
    workstream: &str,
) -> Result<Option<MemoryItem>> {
    let items = list_memory(conn, project_path, Some(workstream), Some("checkpoint"), false)?;
    Ok(items.into_iter().next())
}

/// Mark an item STALE: still stored and listed on request, but excluded
/// from active views, handoff packages and Atlas decisions. The evidence
/// (provenance, supersession chain) is preserved, never deleted.
pub fn mark_stale(conn: &Connection, id: &str) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    Ok(conn.execute(
        "UPDATE memory SET status='stale', updated_at=?1 WHERE id=?2 AND status IN ('active','conflicted')",
        rusqlite::params![now, id],
    )?)
}

/// Archive an item: long-term retention without active visibility.
pub fn archive(conn: &Connection, id: &str) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    Ok(conn.execute(
        "UPDATE memory SET status='archived', updated_at=?1 WHERE id=?2 AND status IN ('active','superseded','stale','conflicted')",
        rusqlite::params![now, id],
    )?)
}

/// Flag two items as CONFLICTED, preserving both bodies plus a note that
/// references the other side. Never silently converts either into fact.
pub fn flag_conflict(conn: &Connection, id_a: &str, id_b: &str, note: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    for (id, other) in [(id_a, id_b), (id_b, id_a)] {
        let n = conn.execute(
            "UPDATE memory SET status='conflicted', conflict_note=?1, updated_at=?2 WHERE id=?3 AND status IN ('active','stale')",
            rusqlite::params![format!("conflicts with {other}: {note}"), now, id],
        )?;
        if n == 0 {
            anyhow::bail!("cannot flag conflict: item {id} not found or not flaggable (status must be active/stale)");
        }
    }
    Ok(())
}

/// All conflicted items in scope, with their notes — for handoff/Atlas.
pub fn conflicts(conn: &Connection, project_path: &str, workstream: Option<&str>) -> Result<Vec<MemoryItem>> {
    let mut sql = format!("SELECT {COLS} FROM memory WHERE project_path=?1 AND status='conflicted'");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_path.to_string())];
    if let Some(ws) = workstream {
        sql.push_str(" AND workstream=?2");
        params.push(Box::new(ws.to_string()));
    }
    sql.push_str(" ORDER BY updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(&refs[..], map_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
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
    fn save_list_supersede_lifecycle() {
        let conn = setup();
        let m1 = save_memory(&conn, "/p", "ws", "decision", "Use SQLite", "No server DB", "explicit", 1.0).unwrap();
        assert_eq!(m1.status, "active");
        assert_eq!(list_memory(&conn, "/p", Some("ws"), Some("decision"), false).unwrap().len(), 1);
        let m2 = supersede(&conn, &m1.id, "/p", "ws", "decision", "Use Postgres", "Ops requires it", "agent-asserted", 0.8).unwrap();
        assert_eq!(m2.supersedes.as_deref(), Some(m1.id.as_str()));
        // Old decision no longer active; new one is.
        let active = list_memory(&conn, "/p", Some("ws"), Some("decision"), false).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, m2.id);
        let all = list_memory(&conn, "/p", Some("ws"), Some("decision"), true).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn task_and_checkpoint_queries() {
        let conn = setup();
        assert!(active_tasks(&conn, "/p", "ws").unwrap().is_empty());
        assert!(latest_checkpoint(&conn, "/p", "ws").unwrap().is_none());
        save_memory(&conn, "/p", "ws", "task", "T-042", "IN_PROGRESS\nDONE: login\nREMAINING: logout", "explicit", 1.0).unwrap();
        save_memory(&conn, "/p", "ws", "checkpoint", "cp-1", "state snapshot", "explicit", 1.0).unwrap();
        assert_eq!(active_tasks(&conn, "/p", "ws").unwrap().len(), 1);
        assert!(latest_checkpoint(&conn, "/p", "ws").unwrap().is_some());
        // Other workstream isolated.
        assert!(active_tasks(&conn, "/p", "other").unwrap().is_empty());
    }
    #[test]
    fn stale_archive_conflict_lifecycle() {
        let conn = setup();
        let a = save_memory(&conn, "/p", "ws", "decision", "A", "body-a", "explicit", 1.0).unwrap();
        let b = save_memory(&conn, "/p", "ws", "decision", "B", "body-b", "agent-asserted", 0.7).unwrap();
        // Stale hides from active views but keeps evidence.
        assert_eq!(mark_stale(&conn, &a.id).unwrap(), 1);
        assert!(list_memory(&conn, "/p", Some("ws"), Some("decision"), false).unwrap().iter().all(|m| m.id != a.id));
        assert_eq!(list_memory(&conn, "/p", Some("ws"), Some("decision"), true).unwrap().len(), 2);
        // Conflict preserves both with cross-referencing notes.
        flag_conflict(&conn, &a.id, &b.id, "opposite DB choice").unwrap();
        let cf = conflicts(&conn, "/p", Some("ws")).unwrap();
        assert_eq!(cf.len(), 2);
        assert!(cf.iter().all(|m| m.status == "conflicted"));
        assert!(cf.iter().all(|m| m.conflict_note.as_deref().unwrap_or("").contains("opposite DB choice")));
        // Neither side became fact; bodies intact.
        assert!(cf.iter().any(|m| m.body == "body-a"));
        // Archived items leave every listing except explicit status queries.
        assert_eq!(archive(&conn, &a.id).unwrap(), 1);
        assert!(conflicts(&conn, "/p", Some("ws")).unwrap().iter().all(|m| m.id != a.id));
    }
}
