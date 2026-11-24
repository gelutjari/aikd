//! Database maintenance: status, integrity check, backup/restore, compact.
//!
//! All operations work on a closed database where required (backup uses
//! SQLite's online `VACUUM INTO`, so readers can stay open).

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

/// Snapshot of database health and sizes.
#[derive(Debug, serde::Serialize)]
pub struct DbStatus {
    pub db_path: String,
    pub db_bytes: u64,
    pub schema_version: i32,
    pub files: i64,
    pub chunks: i64,
    pub embeddings: i64,
    pub sessions: i64,
    pub conversations: i64,
    pub freelist_pages: i64,
    pub page_size: i64,
    pub page_count: i64,
    pub journal_mode: String,
}

pub fn db_status(conn: &Connection, db_path: &Path) -> Result<DbStatus> {
    let q = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(-1) };
    let qs = |sql: &str| -> String {
        conn.query_row(sql, [], |r| r.get(0))
            .unwrap_or_else(|_| "?".to_string())
    };
    Ok(DbStatus {
        db_path: db_path.to_string_lossy().to_string(),
        db_bytes: std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0),
        schema_version: q("SELECT version FROM schema_version LIMIT 1") as i32,
        files: q("SELECT COUNT(*) FROM files"),
        chunks: q("SELECT COUNT(*) FROM chunks"),
        embeddings: q("SELECT COUNT(*) FROM embeddings"),
        sessions: q("SELECT COUNT(*) FROM sessions"),
        conversations: q("SELECT COUNT(*) FROM conversations"),
        freelist_pages: q("PRAGMA freelist_count"),
        page_size: q("PRAGMA page_size"),
        page_count: q("PRAGMA page_count"),
        journal_mode: qs("PRAGMA journal_mode"),
    })
}

/// Run `PRAGMA integrity_check` plus orphan detection. Tantivy (outside
/// SQLite) is verified by query smoke, not row counts. Empty = healthy.
pub fn db_check(conn: &Connection) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let ic: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap_or_else(|_| "failed to run".to_string());
    if ic.to_lowercase() != "ok" {
        problems.push(format!("integrity_check: {ic}"));
    }
    let orphan_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_id NOT IN (SELECT id FROM files)",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    if orphan_chunks > 0 {
        problems.push(format!("orphan chunks: {orphan_chunks} (run: aikd gc)"));
    }
    Ok(problems)
}

/// Online backup via `VACUUM INTO`. Safe to run while readers are open.
pub fn db_backup(conn: &Connection, dest: &Path) -> Result<u64> {
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p)?;
    }
    let dest_s = dest.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{dest_s}'"))?;
    Ok(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0))
}

/// Restore from a backup file: verify the backup, then atomically replace.
/// Caller must ensure no other process holds the database open.
pub fn db_restore(backup: &Path, db_path: &Path) -> Result<()> {
    let check = Connection::open(backup)?;
    let ic: String = check.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if ic.to_lowercase() != "ok" {
        anyhow::bail!("backup file failed integrity_check: {ic}");
    }
    if let Some(p) = db_path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = db_path.with_extension("restore.tmp");
    std::fs::copy(backup, &tmp)?;
    std::fs::rename(&tmp, db_path)?;
    // Remove WAL/SHM sidecars so the restored DB opens cleanly.
    let wal = std::path::PathBuf::from(format!("{}-wal", db_path.display()));
    let shm = std::path::PathBuf::from(format!("{}-shm", db_path.display()));
    let _ = std::fs::remove_file(wal);
    let _ = std::fs::remove_file(shm);
    Ok(())
}

/// Compact: VACUUM to reclaim space, optionally rebuilding FTS5 from chunks/disk.
pub fn db_compact(conn: &Connection, rebuild_fts: bool) -> Result<String> {
    if rebuild_fts {
        let n = crate::fts::rebuild_fts(conn)?;
        conn.execute_batch("VACUUM")?;
        return Ok(format!("rebuilt FTS ({n} chunks) and vacuumed"));
    }
    conn.execute_batch("VACUUM")?;
    Ok("vacuumed".to_string())
}

/// Garbage collect: drop DB rows for files that no longer exist on disk,
/// drop orphan chunks/embeddings left by interrupted scans, then VACUUM.
/// Returns (files_removed, chunks_removed).
#[derive(Debug, Default)]
pub struct RetentionReport {
    pub events_archived: i64,
    pub memory_marked_stale: i64,
}

/// Enforce the retention policy (§A): archive oldest journal events beyond
/// `events_max_per_workstream` per (project, workstream), and mark active
/// memory STALE after `memory_stale_days` untouched. Archiving deletes rows
/// but only AFTER the Atlas snapshot for the project is refreshed, so the
/// counts survive in `atlas` and the resume watermark (`MAX(sequence)`)
/// stays valid — `list_events(since)` simply returns fewer old rows.
pub fn enforce_retention(
    conn: &Connection,
    policy: &aikd_core::RetentionConfig,
) -> Result<RetentionReport> {
    let mut report = RetentionReport::default();
    if policy.events_max_per_workstream > 0 {
        let streams: Vec<(String, String)> = conn
            .prepare("SELECT DISTINCT project_id, workstream FROM events")
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .and_then(|rows| rows.collect::<std::result::Result<Vec<_>, _>>())
            })
            .unwrap_or_default();
        for (project, ws) in streams {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM events WHERE project_id=?1 AND workstream=?2",
                rusqlite::params![project, ws],
                |r| r.get(0),
            )?;
            let over = count.saturating_sub(policy.events_max_per_workstream as i64);
            if over > 0 {
                // Refresh Atlas first so the archived knowledge is not lost.
                let _ = crate::atlas::refresh_atlas(conn, &project);
                report.events_archived += conn.execute(
                    "DELETE FROM events WHERE (project_id, workstream, sequence) IN (
                        SELECT project_id, workstream, sequence FROM events
                        WHERE project_id=?1 AND workstream=?2 ORDER BY sequence ASC LIMIT ?3)",
                    rusqlite::params![project, ws, over],
                )? as i64;
            }
        }
    }
    if policy.events_max_age_days > 0 {
        let cutoff = (chrono::Utc::now()
            - chrono::Duration::days(policy.events_max_age_days as i64))
        .to_rfc3339();
        let streams: Vec<String> = conn
            .prepare("SELECT DISTINCT project_id FROM events WHERE timestamp < ?1")
            .and_then(|mut s| {
                s.query_map(rusqlite::params![cutoff], |r| r.get(0))
                    .and_then(|rows| rows.collect::<std::result::Result<Vec<String>, _>>())
            })
            .unwrap_or_default();
        for p in &streams {
            let _ = crate::atlas::refresh_atlas(conn, p);
        }
        report.events_archived += conn.execute(
            "DELETE FROM events WHERE timestamp < ?1",
            rusqlite::params![cutoff],
        )? as i64;
    }
    if policy.memory_stale_days > 0 {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(policy.memory_stale_days as i64))
            .to_rfc3339();
        report.memory_marked_stale += conn.execute(
            "UPDATE memory SET status='stale', updated_at=?1 WHERE status='active' AND updated_at < ?2",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), cutoff],
        )? as i64;
    }
    Ok(report)
}

pub fn gc(conn: &Connection, disk_check: bool) -> Result<(i64, i64)> {
    gc_with_config(conn, disk_check, None)
}

/// Garbage collect with optional config filtering: drops DB rows for files
/// that no longer exist on disk OR that match newly added `exclude_paths` /
/// `exclude_files` rules in `Config`.
pub fn gc_with_config(
    conn: &Connection,
    disk_check: bool,
    cfg: Option<&aikd_core::Config>,
) -> Result<(i64, i64)> {
    let paths: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, path FROM files")?;
        let x: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        x
    };
    let mut files_removed = 0i64;
    for (fid, ps) in paths {
        let clean = ps.strip_prefix("//?/").unwrap_or(&ps);
        let p = std::path::Path::new(clean);
        let gone = if disk_check {
            !p.exists() && !std::path::Path::new(&ps).exists()
        } else {
            false
        };
        let excluded_by_cfg = if let Some(c) = cfg {
            let dir_excluded = p.ancestors().any(|a| {
                a.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| c.should_exclude_dir(n))
                    .unwrap_or(false)
            });
            let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let file_excluded = c.should_exclude_file(fname);
            dir_excluded || file_excluded
        } else {
            false
        };
        if gone || excluded_by_cfg {
            crate::cascade::delete_file_id_cascade(conn, fid)?;
            files_removed += 1;
        }
    }
    // Orphans from interrupted scans (no live file row).
    let orphan_chunks = conn.execute(
        "DELETE FROM chunks WHERE file_id NOT IN (SELECT id FROM files)",
        [],
    )? as i64;
    conn.execute(
        "DELETE FROM embeddings WHERE chunk_id NOT IN (SELECT id FROM chunks)",
        [],
    )?;
    let _ = conn.execute(
        "DELETE FROM code_symbols WHERE chunk_id NOT IN (SELECT id FROM chunks)",
        [],
    );
    if crate::fts::fts_ready(conn) {
        let _ = conn.execute(
            "DELETE FROM chunks_fts WHERE rowid NOT IN (SELECT rowid FROM chunks)",
            [],
        );
    }
    conn.execute_batch("VACUUM")?;
    Ok((files_removed, orphan_chunks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_check_backup_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        let db = crate::Database::open(&db_path).unwrap();
        let st = db_status(db.conn(), &db_path).unwrap();
        assert_eq!(st.schema_version, 15);
        assert_eq!(st.files, 0);
        assert!(db_check(db.conn()).unwrap().is_empty());
        let dest = dir.path().join("b.db");
        let n = db_backup(db.conn(), &dest).unwrap();
        assert!(n > 0 && dest.exists());
        drop(db);
        let r2 = dir.path().join("r.db");
        db_restore(&dest, &r2).unwrap();
        let db2 = crate::Database::open(&r2).unwrap();
        assert!(db_check(db2.conn()).unwrap().is_empty());
    }
    #[test]
    fn retention_archives_oldest_keeps_watermark_valid() {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::run_migrations(&conn).unwrap();
        for i in 0..10 {
            crate::events::append_event(
                &conn,
                "/p",
                "ws",
                "agy",
                "s",
                "tool_call",
                &format!("src{i}"),
                "{}",
            )
            .unwrap();
        }
        assert_eq!(
            crate::events::latest_sequence(&conn, "/p", "ws").unwrap(),
            10
        );
        let policy = aikd_core::RetentionConfig {
            events_max_per_workstream: 4,
            events_max_age_days: 0,
            memory_stale_days: 0,
        };
        let rep = enforce_retention(&conn, &policy).unwrap();
        assert_eq!(rep.events_archived, 6);
        // Newest 4 survive; watermark still the max sequence; resume works.
        let all = crate::events::list_events(&conn, "/p", "ws", None, 0, 100).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all.last().unwrap().sequence, 10);
        // Memory untouched by event-only policy.
        crate::memory::save_memory(&conn, "/p", "ws", "fact", "F", "b", "explicit", 1.0).unwrap();
        let rep2 = enforce_retention(&conn, &policy).unwrap();
        assert_eq!(rep2.memory_marked_stale, 0);
    }

    #[test]
    fn retention_marks_stale_memory_by_age() {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::run_migrations(&conn).unwrap();
        let m = crate::memory::save_memory(&conn, "/p", "ws", "fact", "F", "b", "explicit", 1.0)
            .unwrap();
        // Backdate below the threshold.
        conn.execute(
            "UPDATE memory SET updated_at='2000-01-01T00:00:00+00:00' WHERE id=?1",
            rusqlite::params![m.id],
        )
        .unwrap();
        let policy = aikd_core::RetentionConfig {
            events_max_per_workstream: 0,
            events_max_age_days: 0,
            memory_stale_days: 30,
        };
        let rep = enforce_retention(&conn, &policy).unwrap();
        assert_eq!(rep.memory_marked_stale, 1);
        let act = crate::memory::list_memory(&conn, "/p", Some("ws"), Some("fact"), false).unwrap();
        assert!(act.is_empty());
    }
}
