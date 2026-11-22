//! Cascading deletes in dependency order.
//!
//! SQLite foreign keys in AIKD have no `ON DELETE CASCADE` (changing that
//! requires rebuilding tables), so every multi-table delete goes through
//! these helpers. Callers must not hand-roll their own delete order.

use anyhow::Result;
use rusqlite::Connection;

/// Delete one indexed file and everything derived from it
/// (embeddings → chunks → files). Returns chunks removed.
pub fn delete_file_cascade(conn: &Connection, path: &str) -> Result<usize> {
    let fid: Option<i64> = conn
        .query_row(
            "SELECT id FROM files WHERE path=?1",
            rusqlite::params![path],
            |r| r.get(0),
        )
        .ok();
    let Some(fid) = fid else { return Ok(0) };
    delete_file_id_cascade(conn, fid)
}

/// Delete by `files.id`. Returns chunks removed.
pub fn delete_file_id_cascade(conn: &Connection, fid: i64) -> Result<usize> {
    let _ = crate::fts::remove_file_fts(conn, fid);
    conn.execute(
        "DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)",
        rusqlite::params![fid],
    )?;
    let _ = conn.execute(
        "DELETE FROM code_symbols WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)",
        rusqlite::params![fid],
    );
    let chunks = conn.execute(
        "DELETE FROM chunks WHERE file_id=?1",
        rusqlite::params![fid],
    )?;
    conn.execute("DELETE FROM files WHERE id=?1", rusqlite::params![fid])?;
    Ok(chunks)
}

/// Delete one session and everything under it
/// (conversation_embeddings → conversations → sessions).
pub fn delete_session_cascade(conn: &Connection, session_id: &str) -> Result<usize> {
    conn.execute(
        "DELETE FROM conversation_embeddings WHERE conversation_id IN (SELECT id FROM conversations WHERE session_id=?1)",
        rusqlite::params![session_id],
    )?;
    let convs = conn.execute(
        "DELETE FROM conversations WHERE session_id=?1",
        rusqlite::params![session_id],
    )?;
    conn.execute(
        "DELETE FROM sessions WHERE id=?1",
        rusqlite::params![session_id],
    )?;
    Ok(convs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_cascade_removes_all_rows() {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/a.md', 1, 't', 't', 'active', 'h')",
            [],
        )
        .unwrap();
        let fid: i64 = conn
            .query_row("SELECT id FROM files WHERE path='/a.md'", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, created_at, updated_at) VALUES ('c1', ?1, 0, '[]', 1, 'H', 1, 1, 'x', 't', 't')",
            rusqlite::params![fid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO embeddings (chunk_id, model, dimensions, vector) VALUES ('c1', 'm', 1, X'0000')",
            [],
        )
        .unwrap();
        assert_eq!(delete_file_cascade(&conn, "/a.md").unwrap(), 1);
        let files: i64 = conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .unwrap();
        let chunks: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        let emb: i64 = conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
            .unwrap();
        assert_eq!((files, chunks, emb), (0, 0, 0));
    }

    #[test]
    fn session_cascade_removes_embeddings() {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, name, project_path, created_at, last_active) VALUES ('s1', 'n', '/p', 't', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversations (id, session_id, role, content, tokens, chunk_refs, created_at) VALUES ('c1', 's1', 'user', 'hi', 0, '[]', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_embeddings (conversation_id, model, dimensions, vector) VALUES ('c1', 'm', 1, X'0000')",
            [],
        )
        .unwrap();
        assert_eq!(delete_session_cascade(&conn, "s1").unwrap(), 1);
        let ce: i64 = conn
            .query_row("SELECT COUNT(*) FROM conversation_embeddings", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(ce, 0);
    }
}
