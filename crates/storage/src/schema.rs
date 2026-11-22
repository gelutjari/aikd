use anyhow::Result;
use rusqlite::Connection;

const SCHEMA_VERSION: i32 = 14;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );
    ",
    )?;

    let current: i32 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);

    if current > SCHEMA_VERSION {
        anyhow::bail!(
            "Database schema version {current} is newer than supported {SCHEMA_VERSION}; refusing to open (upgrade AIKD first)"
        );
    }

    if current < 1 {
        migrate_v1(conn)?;
    }
    if current < 2 {
        migrate_v2(conn)?;
    }
    if current < 3 {
        migrate_v3(conn)?;
    }
    if current < 4 {
        migrate_v4(conn)?;
    }
    if current < 5 {
        migrate_v5(conn)?;
    }
    if current < 6 {
        migrate_v6(conn)?;
    }
    if current < 7 {
        migrate_v7(conn)?;
    }
    if current < 8 {
        migrate_v8(conn)?;
    }

    if current < 9 {
        migrate_v9(conn)?;
    }

    if current < 10 {
        migrate_v10(conn)?;
    }

    if current < 11 {
        migrate_v11(conn)?;
    }

    if current < 12 {
        migrate_v12(conn)?;
    }

    if current < 13 {
        migrate_v13(conn)?;
    }

    if current < 14 {
        migrate_v14(conn)?;
    }

    if current < SCHEMA_VERSION {
        conn.execute("DELETE FROM schema_version", [])?;
        conn.execute("INSERT INTO schema_version VALUES (?1)", [SCHEMA_VERSION])?;
    }

    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE,
            size INTEGER NOT NULL,
            modified_at TEXT NOT NULL,
            last_scanned TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            blake3_hash TEXT NOT NULL DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
        CREATE INDEX IF NOT EXISTS idx_files_status ON files(status);

        CREATE TABLE IF NOT EXISTS chunks (
            id TEXT PRIMARY KEY,
            file_id INTEGER NOT NULL REFERENCES files(id),
            chunk_index INTEGER NOT NULL,
            heading_hierarchy TEXT NOT NULL,
            heading_level INTEGER NOT NULL,
            heading_text TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            content TEXT NOT NULL,
            metadata_json TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks(file_id);
        CREATE INDEX IF NOT EXISTS idx_chunks_heading ON chunks(heading_text);
    ",
    )?;

    log::info!("Migration v1 applied");
    Ok(())
}

fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS embeddings (
            chunk_id TEXT PRIMARY KEY REFERENCES chunks(id),
            model TEXT NOT NULL,
            dimensions INTEGER NOT NULL,
            vector BLOB NOT NULL
        );
    ",
    )?;

    log::info!("Migration v2 applied — embeddings table created");
    Ok(())
}

fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            project_path TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_active TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES sessions(id),
            role TEXT NOT NULL CHECK(role IN ('user','assistant','system')),
            content TEXT NOT NULL,
            tokens INTEGER DEFAULT 0,
            chunk_refs TEXT,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_conv_session ON conversations(session_id, created_at);

        CREATE TABLE IF NOT EXISTS conversation_embeddings (
            conversation_id TEXT PRIMARY KEY REFERENCES conversations(id),
            model TEXT NOT NULL,
            dimensions INTEGER NOT NULL,
            vector BLOB NOT NULL
        );
    ",
    )?;

    // Add blake3_hash column if missing (for databases upgraded from v1)
    let has_hash: bool = conn
        .prepare("PRAGMA table_info(files)")
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(1)?;
                Ok(name)
            })?;
            for r in rows {
                if r? == "blake3_hash" {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .unwrap_or(false);

    if !has_hash {
        let _ = conn.execute(
            "ALTER TABLE files ADD COLUMN blake3_hash TEXT NOT NULL DEFAULT ''",
            [],
        );
    }

    log::info!("Migration v3 applied — sessions, conversations, conversation_embeddings created");
    Ok(())
}

fn migrate_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_chunks_heading_level ON chunks(heading_level);
        CREATE INDEX IF NOT EXISTS idx_chunks_line_range ON chunks(line_start, line_end);
        CREATE INDEX IF NOT EXISTS idx_files_modified ON files(modified_at);

        CREATE TABLE IF NOT EXISTS code_symbols (
            id TEXT PRIMARY KEY,
            chunk_id TEXT NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
            symbol_type TEXT NOT NULL,
            symbol_name TEXT NOT NULL,
            signature TEXT,
            docstring TEXT,
            visibility TEXT DEFAULT 'private',
            language TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_code_symbols_name ON code_symbols(symbol_name);
        CREATE INDEX IF NOT EXISTS idx_code_symbols_type ON code_symbols(symbol_type, language);

        CREATE TABLE IF NOT EXISTS audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_type TEXT NOT NULL,
            detail TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
    ",
    )?;

    log::info!("Migration v4 applied — code_symbols, audit_log, performance indexes");
    Ok(())
}

fn migrate_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        -- Contentless FTS5 index over chunks. The chunk text itself stays in
        -- `chunks.content`; FTS only stores the inverted index + chunk_id key.
        CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            chunk_id UNINDEXED,
            content,
            heading,
            content=''
        );

        -- Keep FTS in sync with chunks inside the same transaction.
        CREATE TRIGGER IF NOT EXISTS chunks_fts_insert AFTER INSERT ON chunks BEGIN
            INSERT INTO chunks_fts(rowid, chunk_id, content, heading)
            VALUES (new.rowid, new.id, new.content, new.heading_text);
        END;
        CREATE TRIGGER IF NOT EXISTS chunks_fts_delete AFTER DELETE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, chunk_id, content, heading)
            VALUES ('delete', old.rowid, old.id, old.content, old.heading_text);
        END;
        CREATE TRIGGER IF NOT EXISTS chunks_fts_update AFTER UPDATE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, chunk_id, content, heading)
            VALUES ('delete', old.rowid, old.id, old.content, old.heading_text);
            INSERT INTO chunks_fts(rowid, chunk_id, content, heading)
            VALUES (new.rowid, new.id, new.content, new.heading_text);
        END;

        -- Backfill FTS from existing chunks.
        INSERT INTO chunks_fts(rowid, chunk_id, content, heading)
            SELECT rowid, id, content, heading_text FROM chunks;

        -- Session lookup is by project_path: keep latest row per project, then enforce uniqueness.
        DELETE FROM sessions WHERE id NOT IN (
            SELECT id FROM sessions GROUP BY project_path HAVING MAX(last_active)
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_project_path ON sessions(project_path);

        -- Embedding lookups filter by model.
        CREATE INDEX IF NOT EXISTS idx_embeddings_model ON embeddings(model);
        ",
    )?;

    log::info!("Migration v5 applied — chunks_fts, session uniqueness, embeddings model index");
    Ok(())
}

fn migrate_v6(conn: &Connection) -> Result<()> {
    // Symbol line ranges: v5 indexed symbols without per-declaration lines.
    // Defaults backfill from the owning chunk; fresh scans write exact lines.
    let has_col: bool = conn
        .prepare("PRAGMA table_info(code_symbols)")
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(1)?;
                Ok(name)
            })?;
            let mut found = false;
            for r in rows {
                if r? == "line_start" {
                    found = true;
                    break;
                }
            }
            Ok(found)
        })
        .unwrap_or(false);
    if !has_col {
        conn.execute(
            "ALTER TABLE code_symbols ADD COLUMN line_start INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        conn.execute(
            "ALTER TABLE code_symbols ADD COLUMN line_end INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        conn.execute(
            "UPDATE code_symbols SET line_start = (SELECT line_start FROM chunks WHERE chunks.id = code_symbols.chunk_id), line_end = (SELECT line_end FROM chunks WHERE chunks.id = code_symbols.chunk_id)",
            [],
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_code_symbols_name ON code_symbols(symbol_name);",
    )?;

    log::info!("Migration v6 applied — code_symbols line ranges");
    Ok(())
}

fn migrate_v7(conn: &Connection) -> Result<()> {
    // Structured memory (P5): typed knowledge with provenance, scope,
    // confidence and lifecycle. Source records stay in `conversations`;
    // this table holds compiled durable memory.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS memory (
            id TEXT PRIMARY KEY,
            project_path TEXT NOT NULL,
            workstream TEXT NOT NULL DEFAULT 'default',
            kind TEXT NOT NULL CHECK(kind IN ('fact','decision','constraint','discovery','task','checkpoint')),
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'explicit' CHECK(source IN ('explicit','agent-asserted','inferred')),
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','superseded','archived')),
            confidence REAL NOT NULL DEFAULT 1.0,
            supersedes TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_memory_project ON memory(project_path, workstream, kind, status);
        ",
    )?;

    log::info!("Migration v7 applied — structured memory table");
    Ok(())
}

fn migrate_v8(conn: &Connection) -> Result<()> {
    // Persistent event journal (attachment P1/P3): append-only source
    // records. sequence is a global monotonic order; event_id PK gives
    // idempotent ingestion (duplicate delivery is ignored).
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS events (
            event_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            workstream TEXT NOT NULL DEFAULT 'default',
            agent_id TEXT NOT NULL,
            agent_session_id TEXT NOT NULL DEFAULT '',
            event_type TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            source_ref TEXT NOT NULL DEFAULT '',
            payload TEXT NOT NULL DEFAULT '{}',
            sequence INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_events_stream ON events(project_id, workstream, sequence);
        CREATE INDEX IF NOT EXISTS idx_events_type ON events(project_id, workstream, event_type, sequence);
        ",
    )?;

    log::info!("Migration v8 applied — event journal");
    Ok(())
}

fn migrate_v9(conn: &Connection) -> Result<()> {
    // Project Atlas (§5-8): one persistent snapshot row per project.
    // snapshot_json is generated FROM canonical tables (files, chunks,
    // code_symbols, memory, events) — never hand-written. fingerprint
    // covers (path, size, mtime) of indexed files; a mismatch marks the
    // snapshot STALE until rebuilt. No rescan of unchanged data.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS atlas (
            project_path TEXT PRIMARY KEY,
            snapshot_json TEXT NOT NULL DEFAULT '{}',
            fingerprint TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'stale' CHECK(status IN ('fresh','stale','empty')),
            updated_at TEXT NOT NULL DEFAULT '',
            build_ms INTEGER NOT NULL DEFAULT 0
        );
        ",
    )?;

    log::info!("Migration v9 applied — project atlas table");
    Ok(())
}

fn migrate_v10(conn: &Connection) -> Result<()> {
    // Memory governance (§11): STALE / CONFLICTED lifecycle + conflict notes.
    // SQLite CHECK constraints cannot be altered, so rebuild the table with
    // the wider status set and copy existing rows verbatim.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS memory_new (
            id TEXT PRIMARY KEY,
            project_path TEXT NOT NULL,
            workstream TEXT NOT NULL DEFAULT 'default',
            kind TEXT NOT NULL CHECK(kind IN ('fact','decision','constraint','discovery','task','checkpoint')),
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'explicit' CHECK(source IN ('explicit','agent-asserted','inferred')),
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','superseded','stale','archived','conflicted')),
            confidence REAL NOT NULL DEFAULT 1.0,
            supersedes TEXT,
            conflict_note TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        INSERT INTO memory_new (id, project_path, workstream, kind, title, body, source, status, confidence, supersedes, conflict_note, created_at, updated_at)
            SELECT id, project_path, workstream, kind, title, body, source, status, confidence, supersedes, NULL, created_at, updated_at FROM memory;
        DROP TABLE memory;
        ALTER TABLE memory_new RENAME TO memory;
        CREATE INDEX IF NOT EXISTS idx_memory_project ON memory(project_path, workstream, kind, status);
        ",
    )?;

    log::info!("Migration v10 applied — memory governance (stale/archived/conflicted)");
    Ok(())
}

fn migrate_v11(conn: &Connection) -> Result<()> {
    // Canonical paths (§21): forward slashes everywhere. Older DBs may hold
    // the same file twice (`C:\\x` + `C:/x`) from mixed-separator scans.
    // Keep the oldest row per canonical path (cascade removes its chunks),
    // then rewrite remaining backslashes. Tantivy docs carry chunk IDs
    // only, so no search-index rewrite is needed.
    // NOTE: MIN(id) keeps the oldest; delete every OTHER row in those groups.
    let groups: Vec<String> = conn
        .prepare("SELECT replace(path, '\\', '/') FROM files GROUP BY replace(path, '\\', '/') HAVING COUNT(*) > 1")
        .and_then(|mut s| {
            s.query_map([], |r| r.get(0))
                .and_then(|rows| rows.collect::<std::result::Result<Vec<String>, _>>())
        })
        .unwrap_or_default();
    for g in &groups {
        let keep: i64 = conn.query_row(
            "SELECT MIN(id) FROM files WHERE replace(path, '\\', '/') = ?1",
            rusqlite::params![g],
            |r| r.get(0),
        )?;
        let victims: Vec<i64> = conn
            .prepare("SELECT id FROM files WHERE replace(path, '\\', '/') = ?1 AND id != ?2")
            .and_then(|mut s| {
                s.query_map(rusqlite::params![g, keep], |r| r.get(0))
                    .and_then(|rows| rows.collect::<std::result::Result<Vec<i64>, _>>())
            })
            .unwrap_or_default();
        for v in victims {
            crate::cascade::delete_file_id_cascade(conn, v)?;
        }
    }
    conn.execute("UPDATE files SET path = replace(path, '\\', '/')", [])?;
    log::info!(
        "Migration v11 applied — canonical slash paths ({} dup groups merged)",
        groups.len()
    );
    Ok(())
}

fn migrate_v12(conn: &Connection) -> Result<()> {
    // R1: Tantivy is the single lexical engine. The SQLite FTS5 mirror
    // (chunks_fts + 3 triggers) was write-amplification with zero readers —
    // no query path ever called fts_search. Drop it all.
    // DROP TABLE on FTS5 removes the data and shadow tables; triggers go
    // explicitly for older SQLite builds.
    conn.execute_batch(
        "
        DROP TRIGGER IF EXISTS chunks_fts_insert;
        DROP TRIGGER IF EXISTS chunks_fts_delete;
        DROP TRIGGER IF EXISTS chunks_fts_update;
        DROP TABLE IF EXISTS chunks_fts;
        ",
    )?;

    log::info!("Migration v12 applied — FTS5 mirror removed (Tantivy is the lexical engine)");
    Ok(())
}

fn migrate_v13(conn: &Connection) -> Result<()> {
    // Canonical project keys everywhere (follow-up of v11 which only fixed
    // files.path). Rows written with backslash keys (memory, events,
    // sessions, atlas) were invisible to slash-key readers — data looked
    // "lost" while present.
    // `sessions.project_path` has a UNIQUE index (idx_sessions_project_path),
    // so merge colliding groups first (re-pointing conversations to the
    // most recently active session) before rewriting separators.
    let colliding_sessions: Vec<String> = conn
        .prepare("SELECT replace(project_path, '\\', '/') FROM sessions GROUP BY replace(project_path, '\\', '/') HAVING COUNT(*) > 1")
        .and_then(|mut s| {
            s.query_map([], |r| r.get(0))
                .and_then(|rows| rows.collect::<std::result::Result<Vec<String>, _>>())
        })
        .unwrap_or_default();
    for canonical in &colliding_sessions {
        let keep: String = conn.query_row(
            "SELECT id FROM sessions WHERE replace(project_path, '\\', '/') = ?1 ORDER BY last_active DESC, rowid DESC LIMIT 1",
            rusqlite::params![canonical],
            |r| r.get(0),
        )?;
        let victims: Vec<String> = conn
            .prepare(
                "SELECT id FROM sessions WHERE replace(project_path, '\\', '/') = ?1 AND id != ?2",
            )
            .and_then(|mut s| {
                s.query_map(rusqlite::params![canonical, keep], |r| r.get(0))
                    .and_then(|rows| rows.collect::<std::result::Result<Vec<String>, _>>())
            })
            .unwrap_or_default();
        for victim in victims {
            conn.execute(
                "UPDATE conversations SET session_id = ?1 WHERE session_id = ?2",
                rusqlite::params![keep, victim],
            )?;
            conn.execute(
                "DELETE FROM sessions WHERE id = ?1",
                rusqlite::params![victim],
            )?;
        }
    }

    for (table, col) in [
        ("memory", "project_path"),
        ("events", "project_id"),
        ("sessions", "project_path"),
        ("atlas", "project_path"),
    ] {
        let sql = format!("UPDATE {table} SET {col} = replace({col}, '\\', '/')");
        let n = conn.execute(&sql, [])?;
        if n > 0 {
            log::info!("Migration v13 normalized {n} rows in {table}.{col}");
        }
    }
    log::info!("Migration v13 applied — canonical project keys");
    Ok(())
}

fn migrate_v14(conn: &Connection) -> Result<()> {
    crate::fts::ensure_fts(conn)?;
    log::info!("Migration v14 applied — contentless FTS5 (chunks_fts)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v13_normalizes_backslash_project_keys() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // Simulate a legacy DB: rewind version, insert pre-fix rows, migrate.
        conn.execute("UPDATE schema_version SET version = 12", [])
            .unwrap();
        // Simulate a pre-fix row written with a Windows backslash key.
        conn.execute(
            "INSERT INTO memory (id, project_path, workstream, kind, title, body, source, status, confidence, supersedes, conflict_note, created_at, updated_at) VALUES ('m1', 'C:\\proj', 'ws', 'task', 'T', 'b', 'explicit', 'active', 1.0, NULL, NULL, 't', 't')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO events (event_id, project_id, workstream, agent_id, agent_session_id, event_type, timestamp, source_ref, payload, sequence) VALUES ('e1', 'C:\\proj', 'ws', 'a', 's', 't', 't', '', '{}', 1)",
            [],
        ).unwrap();
        run_migrations(&conn).unwrap();
        let mp: String = conn
            .query_row("SELECT project_path FROM memory WHERE id='m1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let ep: String = conn
            .query_row(
                "SELECT project_id FROM events WHERE event_id='e1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mp, "C:/proj");
        assert_eq!(ep, "C:/proj");
        // And the canonical reader finds it.
        let items =
            crate::memory::list_memory(&conn, "C:/proj", Some("ws"), Some("task"), false).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_v13_merges_colliding_sessions() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute("UPDATE schema_version SET version = 12", [])
            .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, name, project_path, created_at, last_active) VALUES ('s_bs', 'Old', 'C:\\proj', '2026-01-01', '2026-01-01')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, name, project_path, created_at, last_active) VALUES ('s_sl', 'New', 'C:/proj', '2026-01-02', '2026-01-02')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO conversations (id, session_id, role, content, tokens, chunk_refs, created_at) VALUES ('c_old', 's_bs', 'user', 'hello', 1, '[]', '2026-01-01')",
            [],
        ).unwrap();
        run_migrations(&conn).unwrap();
        let sess_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE project_path='C:/proj'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sess_count, 1);
        let moved: String = conn
            .query_row(
                "SELECT session_id FROM conversations WHERE id='c_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(moved, "s_sl");
    }

    #[test]
    fn test_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"conversations".to_string()));
        assert!(tables.contains(&"conversation_embeddings".to_string()));
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"chunks".to_string()));
        assert!(tables.contains(&"embeddings".to_string()));
        assert!(tables.contains(&"code_symbols".to_string()));
        assert!(tables.contains(&"audit_log".to_string()));
        assert!(tables.contains(&"memory".to_string()));
        assert!(tables.contains(&"events".to_string()));
        assert!(tables.contains(&"atlas".to_string()));
    }

    #[test]
    fn test_schema_version() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_migration_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_sessions_table() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (id, name, project_path, created_at, last_active) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params!["s1", "Test Session", "/project", "2026-01-01", "2026-01-01"],
        ).unwrap();

        let name: String = conn
            .query_row("SELECT name FROM sessions WHERE id='s1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(name, "Test Session");
    }

    #[test]
    fn test_conversations_table() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (id, name, project_path, created_at, last_active) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params!["s1", "Test", "/project", "2026-01-01", "2026-01-01"],
        ).unwrap();

        conn.execute(
            "INSERT INTO conversations (id, session_id, role, content, tokens, chunk_refs, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params!["c1", "s1", "user", "Hello", 5, "[]", "2026-01-01"],
        ).unwrap();

        let content: String = conn
            .query_row(
                "SELECT content FROM conversations WHERE id='c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(content, "Hello");
    }

    #[test]
    fn test_blake3_hash_column() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params!["/test.md", 100, "2026-01-01", "2026-01-01", "active", "abc123"],
        ).unwrap();

        let hash: String = conn
            .query_row(
                "SELECT blake3_hash FROM files WHERE path='/test.md'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hash, "abc123");
    }
}
