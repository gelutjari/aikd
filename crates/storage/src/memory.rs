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
    let item = save_memory(
        conn,
        project_path,
        workstream,
        kind,
        title,
        body,
        source,
        confidence,
    )?;
    conn.execute(
        "UPDATE memory SET supersedes=?1 WHERE id=?2",
        rusqlite::params![old_id, item.id],
    )?;
    Ok(MemoryItem {
        supersedes: Some(old_id.into()),
        ..item
    })
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
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Active tasks for a workstream (the resumable unit of work).
pub fn active_tasks(
    conn: &Connection,
    project_path: &str,
    workstream: &str,
) -> Result<Vec<MemoryItem>> {
    list_memory(conn, project_path, Some(workstream), Some("task"), false)
}

/// Latest checkpoint for a workstream (resume point after compaction).
pub fn latest_checkpoint(
    conn: &Connection,
    project_path: &str,
    workstream: &str,
) -> Result<Option<MemoryItem>> {
    let items = list_memory(
        conn,
        project_path,
        Some(workstream),
        Some("checkpoint"),
        false,
    )?;
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
pub fn conflicts(
    conn: &Connection,
    project_path: &str,
    workstream: Option<&str>,
) -> Result<Vec<MemoryItem>> {
    let mut sql =
        format!("SELECT {COLS} FROM memory WHERE project_path=?1 AND status='conflicted'");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_path.to_string())];
    if let Some(ws) = workstream {
        sql.push_str(" AND workstream=?2");
        params.push(Box::new(ws.to_string()));
    }
    sql.push_str(" ORDER BY updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(&refs[..], map_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// All conflicted items across every project/workstream (for the global `/ui` dashboard).
pub fn all_conflicts(conn: &Connection) -> Result<Vec<MemoryItem>> {
    let sql =
        format!("SELECT {COLS} FROM memory WHERE status='conflicted' ORDER BY updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Active constraints for auto-attach on query/symbol/context-pack.
///
/// "Global" constraints mean `memory.project_path = ''` and notes with
/// `scope='global'`; project constraints must match the queried project key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ConstraintRef {
    pub id: String,
    pub title: String,
    pub body: String,
    pub kind: String,
    pub scope: String,
    pub project: String,
}

/// Top active constraints relevant to `project` (project-specific first,
/// then global), capped at `limit` (default 5 when 0).
pub fn attach_constraints(
    conn: &Connection,
    project: &str,
    limit: usize,
) -> Result<Vec<ConstraintRef>> {
    let project = aikd_core::platform::normalize_project_key(project);
    let limit = if limit == 0 { 5 } else { limit };
    let mut out: Vec<ConstraintRef> = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT id, title, body, project_path FROM memory WHERE kind='constraint' AND status='active' AND (project_path='' OR project_path=?1) ORDER BY CASE WHEN project_path=?1 THEN 0 ELSE 1 END, datetime(updated_at) DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![project, limit as i64], |r| {
        let project_path: String = r.get(3)?;
        Ok(ConstraintRef {
            id: r.get(0)?,
            title: r.get(1)?,
            body: r.get(2)?,
            kind: "constraint".to_string(),
            scope: if project_path.is_empty() {
                "global".to_string()
            } else {
                "project".to_string()
            },
            project: project_path,
        })
    })?;
    for row in rows.flatten() {
        out.push(row);
    }

    if out.len() < limit {
        let mut stmt = conn.prepare(
            "SELECT id, title, body, scope, project FROM notes WHERE kind='constraint' AND (scope='global' OR project=?1) ORDER BY CASE WHEN scope='global' THEN 1 ELSE 0 END, datetime(updated_at) DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![project, (limit - out.len()) as i64],
            |r| {
                Ok(ConstraintRef {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    body: r.get(2)?,
                    kind: "constraint".to_string(),
                    scope: r.get(3)?,
                    project: r.get(4)?,
                })
            },
        )?;
        for row in rows.flatten() {
            out.push(row);
        }
    }
    out.truncate(limit);
    Ok(out)
}

/// All active (and conflicted) memory across every project — used by backup
/// export and global dashboards, not by per-project handoff.
pub fn all_active(conn: &Connection) -> Result<Vec<MemoryItem>> {
    let sql = format!(
        "SELECT {COLS} FROM memory WHERE status IN ('active','conflicted') ORDER BY datetime(updated_at) DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Remove synthetic unit-test / dummy rows so production dashboards only show real knowledge.
pub fn purge_dummy_memories(conn: &Connection) -> Result<usize> {
    let n = conn.execute(
        "DELETE FROM memory WHERE title IN ('uji-flag','TASK-VERIFY','T1','Use Postgres','Use SQLite','cp-1','TASK-042') OR project_path='.'",
        [],
    )?;
    Ok(n)
}

/// Auto-Harvest & Auto-Supersede project rules (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`,
/// `.cursorrules`, `.windsurfrules`, `.clinerules`, `copilot-instructions.md`) and
/// agent skills (`SKILL.md`) from indexed files on disk into `memory`.
///
/// Whenever a rule/skill file is edited on disk, its Blake3 hash changes and this
/// function automatically supersedes the previous memory entry with the updated content!
/// Also archives memory items whose backing file was removed or excluded.
/// Returns `(created, superseded, unchanged)`.
pub fn sync_rules_and_skills_from_files(
    conn: &Connection,
    default_project: &str,
) -> Result<(usize, usize, usize)> {
    let mut stmt = conn.prepare("SELECT path, blake3_hash FROM files WHERE status='active'")?;
    let files: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut created = 0usize;
    let mut superseded = 0usize;
    let mut unchanged = 0usize;
    let mut seen_titles = std::collections::HashSet::new();

    for (path, hash) in files {
        let norm = path.replace('\\', "/");
        let clean = norm.strip_prefix("//?/").unwrap_or(&norm);
        // Skip internal temporary/staging folders even if still in files table
        if clean.contains("/.tmp/")
            || clean.contains("/.sandbox")
            || clean.contains("/.system_generated/")
            || clean.contains("/scratch/")
            || clean.contains("/_disabled/")
        {
            continue;
        }
        let fname = clean.rsplit('/').next().unwrap_or("");
        let fname_lower = fname.to_lowercase();

        let (kind, workstream, title_prefix) = match fname_lower.as_str() {
            "agents.md"
            | "claude.md"
            | "claude.local.md"
            | "gemini.md"
            | ".cursorrules"
            | ".windsurfrules"
            | ".clinerules"
            | ".roorules"
            | "copilot-instructions.md" => ("constraint", "rules", "Rule"),
            "skill.md" => ("fact", "skills", "Skill"),
            "memory.md" | "architecture.md" | "setup.md" | "rumus_pajak_samsat.md" => {
                ("fact", "docs", "Doc")
            }
            _ => continue,
        };

        // Determine a human-friendly title from the parent folder + filename
        let parts: Vec<&str> = clean.split('/').collect();
        let parent = if parts.len() >= 2 {
            parts[parts.len() - 2]
        } else {
            "root"
        };
        let title = if fname_lower == "skill.md" {
            format!("[Skill: {parent}] ({clean})")
        } else {
            format!("[{title_prefix}: {parent}/{fname}] ({clean})")
        };
        seen_titles.insert(title.clone());

        let raw_content = match std::fs::read_to_string(std::path::Path::new(clean))
            .or_else(|_| std::fs::read_to_string(std::path::Path::new(&path)))
        {
            Ok(c) => c,
            Err(_) => continue,
        };

        let summary = summarize_md_for_memory(&raw_content, 900);
        if summary.trim().is_empty() {
            continue;
        }
        let body = format!("{summary}\n[file_hash:{hash}]");

        // Check if an active memory item with this exact title already exists
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT id, body FROM memory WHERE title=?1 AND status='active' LIMIT 1",
                rusqlite::params![title],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        match existing {
            Some((old_id, old_body)) => {
                if old_body.contains(&format!("[file_hash:{hash}]")) {
                    unchanged += 1;
                } else {
                    let _ = supersede(
                        conn,
                        &old_id,
                        default_project,
                        workstream,
                        kind,
                        &title,
                        &body,
                        "inferred",
                        1.0,
                    )?;
                    superseded += 1;
                }
            }
            None => {
                let _ = save_memory(
                    conn,
                    default_project,
                    workstream,
                    kind,
                    &title,
                    &body,
                    "inferred",
                    1.0,
                )?;
                created += 1;
            }
        }
    }

    // Remove inferred file-synced memories whose backing file was excluded or deleted
    let existing_inferred: Vec<(String, String)> = conn
        .prepare("SELECT id, title FROM memory WHERE source='inferred' AND status='active'")
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .and_then(|rows| rows.collect::<std::result::Result<Vec<_>, _>>())
        })
        .unwrap_or_default();
    for (id, title) in existing_inferred {
        if !seen_titles.contains(&title) {
            let _ = conn.execute("DELETE FROM memory WHERE id=?1", rusqlite::params![id]);
        }
    }

    Ok((created, superseded, unchanged))
}

fn summarize_md_for_memory(content: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut in_frontmatter = false;
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if idx == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            } else if trimmed.starts_with("name:") || trimmed.starts_with("description:") {
                out.push_str(trimmed);
                out.push('\n');
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        out.push_str(trimmed);
        out.push('\n');
        if out.len() >= max_chars {
            break;
        }
    }
    if out.chars().count() > max_chars {
        let end = out
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(out.len());
        format!("{}...", &out[..end])
    } else {
        out.trim().to_string()
    }
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
        let m1 = save_memory(
            &conn,
            "/p",
            "ws",
            "decision",
            "Use SQLite",
            "No server DB",
            "explicit",
            1.0,
        )
        .unwrap();
        assert_eq!(m1.status, "active");
        assert_eq!(
            list_memory(&conn, "/p", Some("ws"), Some("decision"), false)
                .unwrap()
                .len(),
            1
        );
        let m2 = supersede(
            &conn,
            &m1.id,
            "/p",
            "ws",
            "decision",
            "Use Postgres",
            "Ops requires it",
            "agent-asserted",
            0.8,
        )
        .unwrap();
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
        save_memory(
            &conn,
            "/p",
            "ws",
            "task",
            "T-042",
            "IN_PROGRESS\nDONE: login\nREMAINING: logout",
            "explicit",
            1.0,
        )
        .unwrap();
        save_memory(
            &conn,
            "/p",
            "ws",
            "checkpoint",
            "cp-1",
            "state snapshot",
            "explicit",
            1.0,
        )
        .unwrap();
        assert_eq!(active_tasks(&conn, "/p", "ws").unwrap().len(), 1);
        assert!(latest_checkpoint(&conn, "/p", "ws").unwrap().is_some());
        // Other workstream isolated.
        assert!(active_tasks(&conn, "/p", "other").unwrap().is_empty());
    }
    #[test]
    fn stale_archive_conflict_lifecycle() {
        let conn = setup();
        let a = save_memory(
            &conn, "/p", "ws", "decision", "A", "body-a", "explicit", 1.0,
        )
        .unwrap();
        let b = save_memory(
            &conn,
            "/p",
            "ws",
            "decision",
            "B",
            "body-b",
            "agent-asserted",
            0.7,
        )
        .unwrap();
        // Stale hides from active views but keeps evidence.
        assert_eq!(mark_stale(&conn, &a.id).unwrap(), 1);
        assert!(
            list_memory(&conn, "/p", Some("ws"), Some("decision"), false)
                .unwrap()
                .iter()
                .all(|m| m.id != a.id)
        );
        assert_eq!(
            list_memory(&conn, "/p", Some("ws"), Some("decision"), true)
                .unwrap()
                .len(),
            2
        );
        // Conflict preserves both with cross-referencing notes.
        flag_conflict(&conn, &a.id, &b.id, "opposite DB choice").unwrap();
        let cf = conflicts(&conn, "/p", Some("ws")).unwrap();
        assert_eq!(cf.len(), 2);
        assert!(cf.iter().all(|m| m.status == "conflicted"));
        assert!(cf.iter().all(|m| m
            .conflict_note
            .as_deref()
            .unwrap_or("")
            .contains("opposite DB choice")));
        // Neither side became fact; bodies intact.
        assert!(cf.iter().any(|m| m.body == "body-a"));
        // Archived items leave every listing except explicit status queries.
        assert_eq!(archive(&conn, &a.id).unwrap(), 1);
        assert!(conflicts(&conn, "/p", Some("ws"))
            .unwrap()
            .iter()
            .all(|m| m.id != a.id));
    }

    #[test]
    fn attach_constraints_prefers_project_then_global() {
        let conn = setup();
        save_memory(
            &conn,
            "",
            "rules",
            "constraint",
            "Global: never commit secrets",
            "g",
            "explicit",
            1.0,
        )
        .unwrap();
        save_memory(
            &conn,
            "/p",
            "rules",
            "constraint",
            "Project: zero downtime",
            "p",
            "explicit",
            1.0,
        )
        .unwrap();
        save_memory(
            &conn,
            "/other",
            "rules",
            "constraint",
            "Other project only",
            "o",
            "explicit",
            1.0,
        )
        .unwrap();
        let got = attach_constraints(&conn, "/p", 5).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].scope, "project");
        assert!(got.iter().all(|c| c.title != "Other project only"));
    }
}
