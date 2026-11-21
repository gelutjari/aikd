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
    pub fts_docs: i64,
    pub freelist_pages: i64,
    pub page_size: i64,
    pub page_count: i64,
    pub journal_mode: String,
}

pub fn db_status(conn: &Connection, db_path: &Path) -> Result<DbStatus> {
    let q = |sql: &str| -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap_or(-1)
    };
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
        fts_docs: q("SELECT COUNT(*) FROM chunks_fts"),
        freelist_pages: q("PRAGMA freelist_count"),
        page_size: q("PRAGMA page_size"),
        page_count: q("PRAGMA page_count"),
        journal_mode: qs("PRAGMA journal_mode"),
    })
}

/// Run `PRAGMA integrity_check` plus a row-count cross-check between
/// `chunks` and the FTS index. Returns a list of problems (empty = healthy).
pub fn db_check(conn: &Connection) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let ic: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap_or_else(|_| "failed to run".to_string());
    if ic.to_lowercase() != "ok" {
        problems.push(format!("integrity_check: {ic}"));
    }
    let chunks: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap_or(-1);
    let fts: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
        .unwrap_or(-2);
    if chunks != fts {
        problems.push(format!(
            "fts drift: chunks={chunks} fts_docs={fts} (run: aikd db compact --rebuild-fts)"
        ));
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
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    Ok(())
}

/// Compact: remove stale index rows, VACUUM to reclaim space, optionally
/// rebuild the FTS index from scratch.
pub fn db_compact(conn: &Connection, rebuild_fts: bool) -> Result<String> {
    if rebuild_fts {
        let n = super::fts::rebuild_fts(conn)?;
        conn.execute_batch("VACUUM")?;
        return Ok(format!("fts rebuilt ({n} docs), vacuumed"));
    }
    conn.execute_batch("VACUUM")?;
    Ok("vacuumed".to_string())
}

/// Garbage collect: drop DB rows for files that no longer exist on disk,
/// drop orphan chunks/embeddings left by interrupted scans, then VACUUM.
/// Returns (files_removed, chunks_removed).
pub fn gc(conn: &Connection, disk_check: bool) -> Result<(i64, i64)> {
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
        let gone = if disk_check {
            !std::path::Path::new(&ps).exists()
        } else {
            false
        };
        if gone {
            conn.execute(
                "DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)",
                rusqlite::params![fid],
            )?;
            conn.execute("DELETE FROM chunks WHERE file_id=?1", rusqlite::params![fid])?;
            conn.execute("DELETE FROM files WHERE id=?1", rusqlite::params![fid])?;
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
    // FTS rows follow chunks via triggers on DELETE; verify below in db_check.
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
        assert_eq!(st.schema_version, 9);
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
}
