use rusqlite::Connection;
use anyhow::Result;

const SCHEMA_VERSION: i32 = 4;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );
    ")?;

    let current: i32 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
        .unwrap_or(0);

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

    if current < SCHEMA_VERSION {
        conn.execute("DELETE FROM schema_version", [])?;
        conn.execute("INSERT INTO schema_version VALUES (?1)", [SCHEMA_VERSION])?;
    }

    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch("
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
    ")?;

    log::info!("Migration v1 applied");
    Ok(())
}

fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS embeddings (
            chunk_id TEXT PRIMARY KEY REFERENCES chunks(id),
            model TEXT NOT NULL,
            dimensions INTEGER NOT NULL,
            vector BLOB NOT NULL
        );
    ")?;

    log::info!("Migration v2 applied — embeddings table created");
    Ok(())
}

fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch("
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
    ")?;

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
        let _ = conn.execute("ALTER TABLE files ADD COLUMN blake3_hash TEXT NOT NULL DEFAULT ''", []);
    }

    log::info!("Migration v3 applied — sessions, conversations, conversation_embeddings created");
    Ok(())
}

fn migrate_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch("
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
    ")?;

    log::info!("Migration v4 applied — code_symbols, audit_log, performance indexes");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn test_schema_version() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_migration_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
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
            .query_row("SELECT name FROM sessions WHERE id='s1'", [], |row| row.get(0))
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
            .query_row("SELECT content FROM conversations WHERE id='c1'", [], |row| row.get(0))
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
            .query_row("SELECT blake3_hash FROM files WHERE path='/test.md'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(hash, "abc123");
    }
}
