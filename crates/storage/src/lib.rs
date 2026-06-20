pub mod schema;

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

/// Connection pool type for SQLite
pub type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;

/// Database with connection pooling support.
/// For single-threaded use, prefer `Database::open()`.
/// For multi-threaded use, prefer `Database::open_pooled()`.
pub struct Database {
    conn: Connection,
    pool: Option<Pool>,
}

impl Database {
    /// Open a single-connection database (for CLI, watcher, etc.)
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        Self::configure_connection(&conn)?;
        let db = Self { conn, pool: None };
        schema::run_migrations(&db.conn)?;
        Ok(db)
    }

    /// Open a pooled database (for server, concurrent access)
    pub fn open_pooled(db_path: &Path, max_size: u32) -> Result<(Self, Pool)> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = r2d2_sqlite::SqliteConnectionManager::file(db_path);
        let pool = r2d2::Pool::builder()
            .max_size(max_size)
            .connection_customizer(Box::new(SqliteConnectionCustomizer))
            .build(manager)?;

        // Run migrations on first connection
        let conn = pool.get()?;
        schema::run_migrations(&conn)?;
        drop(conn);

        // Also open a direct connection for compatibility
        let direct_conn = Connection::open(db_path)?;
        Self::configure_connection(&direct_conn)?;

        let db = Self {
            conn: direct_conn,
            pool: Some(pool.clone()),
        };

        Ok((db, pool))
    }

    /// Get a connection from the pool, or the direct connection.
    /// For pooled databases, this returns a pooled connection.
    /// For single databases, this returns a reference to the direct connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Get a connection from the pool (if pooled).
    /// Returns None if not using connection pooling.
    pub fn pooled_conn(&self) -> Option<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>> {
        self.pool.as_ref().and_then(|p| p.get().ok())
    }

    /// Get the pool reference (if pooled).
    pub fn pool(&self) -> Option<&Pool> {
        self.pool.as_ref()
    }

    /// Check if database is using connection pooling.
    pub fn is_pooled(&self) -> bool {
        self.pool.is_some()
    }

    fn configure_connection(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA synchronous=NORMAL; \
             PRAGMA foreign_keys=ON; \
             PRAGMA busy_timeout=5000; \
             PRAGMA cache_size=-64000; \
             PRAGMA mmap_size=268435456;",
        )?;
        Ok(())
    }

    pub fn begin_transaction(&self) -> Result<Transaction<'_>> {
        let tx = self.conn.unchecked_transaction()?;
        Ok(Transaction { tx })
    }
}

/// Custom connection initializer for pooled connections
#[derive(Debug)]
struct SqliteConnectionCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for SqliteConnectionCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA synchronous=NORMAL; \
             PRAGMA foreign_keys=ON; \
             PRAGMA busy_timeout=5000;",
        )?;
        Ok(())
    }
}

pub struct Transaction<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl<'a> Transaction<'a> {
    pub fn conn(&self) -> &Connection {
        &self.tx
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }
}

impl Database {
    /// Count active files in the database.
    pub fn file_count(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE status='active'",
            [],
            |r| r.get(0),
        )?)
    }

    /// Count chunks in the database.
    pub fn chunk_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?)
    }

    /// Count embeddings in the database.
    pub fn embedding_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?)
    }

    /// Load chunks by their IDs with optional filters.
    /// Uses batch query with WHERE IN (...) for better performance (avoids N+1 queries).
    pub fn load_chunks(
        &self,
        ids: &[String],
        filters: &aikd_core::SearchFilters,
    ) -> Result<Vec<aikd_core::SearchResult>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Build batch query with parameterized IN clause
        // SQLite has a limit on variables (SQLITE_LIMIT_VARIABLE_NUMBER, default 999)
        // Process in chunks of 900 to stay safe
        let mut results = Vec::with_capacity(ids.len());
        let chunk_size = 900;

        for id_chunk in ids.chunks(chunk_size) {
            // Build placeholders: "?1,?2,?3,..."
            let placeholders: Vec<String> = (0..id_chunk.len())
                .map(|i| format!("?{}", i + 1))
                .collect();
            let query = format!(
                "SELECT c.id, f.path, c.heading_hierarchy, c.heading_text, c.content, c.line_start, c.line_end \
                 FROM chunks c JOIN files f ON c.file_id=f.id \
                 WHERE c.id IN ({})",
                placeholders.join(",")
            );

            // Convert IDs to rusqlite params
            let params: Vec<Box<dyn rusqlite::types::ToSql>> = id_chunk
                .iter()
                .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
                .collect();
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = self.conn.prepare(&query)?;
            let rows = stmt.query_map(param_refs.as_slice(), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)? as usize,
                    r.get::<_, i64>(6)? as usize,
                ))
            })?;

            for row in rows {
                let (cid, fp, hj, ht, co, ls, le) = row?;

                // Apply filters in application layer
                if let Some(ref p) = filters.path_contains {
                    if !fp.contains(p.as_str()) {
                        continue;
                    }
                }
                if let Some(ref pe) = filters.path_exclude {
                    if fp.contains(pe.as_str()) {
                        continue;
                    }
                }
                if let Some(ref ft) = filters.file_types {
                    let has_ext = ft.iter().any(|ext| fp.ends_with(&format!(".{ext}")));
                    if !has_ext {
                        continue;
                    }
                }
                if let Some(ref h) = filters.heading_contains {
                    if !ht.contains(h.as_str()) {
                        continue;
                    }
                }

                let hier: Vec<String> = serde_json::from_str(&hj).unwrap_or_default();
                results.push(aikd_core::SearchResult {
                    chunk_id: cid,
                    file_path: fp,
                    heading_hierarchy: hier.join(" > "),
                    heading_text: ht,
                    content: co,
                    line_start: ls,
                    line_end: le,
                    score: 0.0,
                });
            }
        }

        Ok(results)
    }

    /// Enrich search results with line numbers from the database.
    pub fn enrich_line_numbers(
        &self,
        results: &[aikd_core::SearchResult],
    ) -> Result<Vec<aikd_core::SearchResult>> {
        let mut enriched = Vec::with_capacity(results.len());
        for r in results {
            let lines = self
                .conn
                .query_row(
                    "SELECT line_start, line_end FROM chunks WHERE id=?1",
                    rusqlite::params![r.chunk_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)? as usize,
                            row.get::<_, i64>(1)? as usize,
                        ))
                    },
                )
                .unwrap_or((0, 0));
            enriched.push(aikd_core::SearchResult {
                chunk_id: r.chunk_id.clone(),
                file_path: r.file_path.clone(),
                heading_hierarchy: r.heading_hierarchy.clone(),
                heading_text: r.heading_text.clone(),
                content: r.content.clone(),
                line_start: lines.0,
                line_end: lines.1,
                score: r.score,
            });
        }
        Ok(enriched)
    }

    /// Log an audit event to the audit_log table.
    pub fn log_audit(&self, event_type: &str, detail: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO audit_log (event_type, detail) VALUES (?1, ?2)",
            rusqlite::params![event_type, detail],
        )?;
        Ok(())
    }
}

pub fn compute_blake3(path: &Path) -> Result<String> {
    let content = std::fs::read(path)?;
    let hash = blake3::hash(&content);
    Ok(hash.to_hex().to_string())
}
