pub mod schema;

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        let db = Self { conn };
        schema::run_migrations(&db.conn)?;
        Ok(db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn begin_transaction(&self) -> Result<Transaction<'_>> {
        let tx = self.conn.unchecked_transaction()?;
        Ok(Transaction { tx })
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
    pub fn load_chunks(
        &self,
        ids: &[String],
        filters: &aikd_core::SearchFilters,
    ) -> Result<Vec<aikd_core::SearchResult>> {
        let mut results = Vec::new();
        for id in ids {
            let row = self.conn.query_row(
                "SELECT c.id,f.path,c.heading_hierarchy,c.heading_text,c.content,c.line_start,c.line_end FROM chunks c JOIN files f ON c.file_id=f.id WHERE c.id=?1",
                rusqlite::params![id],
                |r| Ok((
                    r.get::<_,String>(0)?,
                    r.get::<_,String>(1)?,
                    r.get::<_,String>(2)?,
                    r.get::<_,String>(3)?,
                    r.get::<_,String>(4)?,
                    r.get::<_,i64>(5)? as usize,
                    r.get::<_,i64>(6)? as usize,
                )),
            );
            match row {
                Ok((cid, fp, hj, ht, co, ls, le)) => {
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
                        let has_ext = ft.iter().any(|ext| fp.ends_with(&format!(".{}", ext)));
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
                Err(_) => continue,
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
