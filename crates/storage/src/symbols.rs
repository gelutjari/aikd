//! Symbol lookup over the `code_symbols` index (populated at scan time).

use aikd_core::CodeSymbol;
use anyhow::Result;
use rusqlite::Connection;

/// Find symbols by exact name first, then prefix match. Results carry
/// full provenance (file path + line range via the owning chunk).
pub fn find_symbols(conn: &Connection, name: &str, limit: usize) -> Result<Vec<CodeSymbol>> {
    let sql = "SELECT s.symbol_name, s.symbol_type, s.signature, s.visibility, s.language, \
               f.path, s.line_start, s.line_end, s.chunk_id \
               FROM code_symbols s \
               JOIN chunks c ON c.id = s.chunk_id \
               JOIN files f ON f.id = c.file_id \
               WHERE s.symbol_name = ?1 OR s.symbol_name LIKE ?2 \
               ORDER BY CASE WHEN s.symbol_name = ?1 THEN 0 ELSE 1 END, s.symbol_name \
               LIMIT ?3";
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params![name, format!("{name}%"), limit as i64],
        |r| {
            Ok(CodeSymbol {
                name: r.get(0)?,
                kind: r.get(1)?,
                signature: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                visibility: r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "private".into()),
                language: r.get(4)?,
                file_path: r.get(5)?,
                line_start: r.get::<_, i64>(6)? as usize,
                line_end: r.get::<_, i64>(7)? as usize,
                chunk_id: r.get(8)?,
            })
        },
    )?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Total number of indexed symbols.
pub fn symbol_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM code_symbols", [], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_symbols_exact_and_prefix() {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/a.rs', 1, 't', 't', 'active', 'h')",
            [],
        )
        .unwrap();
        let fid: i64 = conn
            .query_row("SELECT id FROM files WHERE path='/a.rs'", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, created_at, updated_at) VALUES ('c1', ?1, 0, '[]', 0, 'fn calculate_tax', 10, 20, 'x', 't', 't')",
            rusqlite::params![fid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO code_symbols (id, chunk_id, symbol_type, symbol_name, signature, visibility, language, line_start, line_end, created_at) VALUES ('s1', 'c1', 'function', 'calculate_tax', 'fn calculate_tax(x: f64)', 'public', 'rust', 10, 20, 't')",
            [],
        )
        .unwrap();
        let exact = find_symbols(&conn, "calculate_tax", 10).unwrap();
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].file_path, "/a.rs");
        assert_eq!(exact[0].line_start, 10);
        assert_eq!(exact[0].line_end, 20);
        let prefix = find_symbols(&conn, "calculate", 10).unwrap();
        assert_eq!(prefix.len(), 1);
        assert!(find_symbols(&conn, "nope", 10).unwrap().is_empty());
        assert_eq!(symbol_count(&conn).unwrap(), 1);
    }
}
