//! SQLite FTS5 lexical search backend (contentless index over `chunks`).
//!
//! `chunks_fts` stores only the inverted index + `chunk_id` key; the chunk
//! text itself stays in `chunks.content`. Synchronization is handled by
//! triggers created in migration v5 (`schema.rs`), so inserts/deletes that
//! go through SQLite stay consistent inside the same transaction.

use aikd_core::{SearchFilters, SearchResult};
use anyhow::Result;
use rusqlite::Connection;

/// Escape free-text user input into a safe FTS5 MATCH expression.
///
/// Each whitespace-separated token is double-quoted (embedded quotes are
/// doubled) and tokens are AND-ed. This guarantees the expression never
/// throws a syntax error for inputs like `C++`, `foo:bar`, or unbalanced
/// quotes — at the cost of treating special operators as literals.
pub fn escape_match_query(query: &str) -> String {
    let mut out = Vec::new();
    for tok in query.split_whitespace() {
        let t = tok.replace('"', "\"\"");
        out.push(format!("\"{t}\""));
    }
    if out.is_empty() {
        // Match nothing on empty input rather than erroring.
        return "\"\"".to_string();
    }
    out.join(" AND ")
}

/// Lexical search over the FTS5 index, joined back to `chunks`/`files` for
/// full provenance (path, line range, heading) in a single query.
pub fn fts_search(
    conn: &Connection,
    query: &str,
    limit: usize,
    filters: &SearchFilters,
) -> Result<Vec<SearchResult>> {
    let matched = escape_match_query(query);
    let mut sql = String::from(
        "SELECT c.id, f.path, c.heading_hierarchy, c.heading_text, \
         c.line_start, c.line_end, c.content, rank \
         FROM chunks_fts \
         JOIN chunks c ON c.rowid = chunks_fts.rowid \
         JOIN files f ON f.id = c.file_id \
         WHERE chunks_fts MATCH ?1",
    );
    if filters.path_contains.is_some() {
        sql.push_str(" AND f.path LIKE '%' || ?2 || '%'");
    }
    if filters.path_exclude.is_some() {
        sql.push_str(" AND f.path NOT LIKE '%' || ?3 || '%'");
    }
    if filters.file_types.is_some() {
        sql.push_str(" AND (0 = 1");
        // placeholder expanded below
        sql.push_str(")");
    }
    if filters.heading_contains.is_some() {
        sql.push_str(" AND c.heading_text LIKE '%' || ?5 || '%'");
    }
    sql.push_str(" ORDER BY rank LIMIT ?6");

    // Bind positional params in fixed slots; unused slots get NULL/0.
    // file_types is expanded as a dynamic OR-list instead (see below).
    let mut final_sql = sql.clone();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params.push(Box::new(matched));
    params.push(Box::new(
        filters.path_contains.clone().unwrap_or_default(),
    ));
    params.push(Box::new(
        filters.path_exclude.clone().unwrap_or_default(),
    ));
    // slot ?4 reserved for file_types expansion below
    params.push(Box::new(String::new()));
    params.push(Box::new(
        filters.heading_contains.clone().unwrap_or_default(),
    ));
    params.push(Box::new(limit as i64));

    if let Some(ft) = &filters.file_types {
        let ors: Vec<String> = ft
            .iter()
            .enumerate()
            .map(|(i, _)| format!(" OR f.path LIKE '%' || ?{} || '%'", 7 + i))
            .collect();
        final_sql = final_sql.replace(" AND (0 = 1)", &format!(" AND (0 = 1{} )", ors.join("")));
        for ext in ft {
            params.push(Box::new(format!(".{ext}")));
        }
    } else {
        final_sql = final_sql.replace(" AND (0 = 1)", "");
    }

    let param_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&final_sql)?;
    let rows = stmt.query_map(&param_refs[..], |row| {
        Ok(SearchResult {
            chunk_id: row.get(0)?,
            file_path: row.get(1)?,
            heading_hierarchy: row.get(2)?,
            heading_text: row.get(3)?,
            line_start: row.get::<_, i64>(4)? as usize,
            line_end: row.get::<_, i64>(5)? as usize,
            content: row.get(6)?,
            // FTS rank is negative (lower = better); negate for score parity.
            score: -row.get::<_, f64>(7)? as f32,
        })
    })?;
    let mut results = Vec::new();
    for r in rows {
        results.push(r?);
    }
    Ok(results)
}

/// Rebuild the FTS index from `chunks` (full backfill). Prefer incremental
/// trigger sync in the normal path; use this after corruption or restore.
pub fn rebuild_fts(conn: &Connection) -> Result<usize> {
    // Contentless tables do not support DELETE FROM; use delete-all instead.
    conn.execute("INSERT INTO chunks_fts(chunks_fts) VALUES('delete-all')", [])?;
    let n = conn.execute(
        "INSERT INTO chunks_fts(rowid, chunk_id, content, heading) \
         SELECT rowid, id, content, heading_text FROM chunks",
        [],
    )?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/a.md', 10, 't', 't', 'active', 'h')",
            [],
        )
        .unwrap();
        let fid: i64 = conn
            .query_row("SELECT id FROM files WHERE path='/a.md'", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, created_at, updated_at) VALUES ('c1', ?1, 0, '[]', 1, 'Auth', 1, 5, 'authentication token flow', 't', 't')",
            rusqlite::params![fid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, created_at, updated_at) VALUES ('c2', ?1, 1, '[]', 1, 'DB', 6, 9, 'database connection pooling', 't', 't')",
            rusqlite::params![fid],
        )
        .unwrap();
        conn
    }

    #[test]
    fn fts_available_and_synced_by_trigger() {
        let conn = setup();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn fts_search_basic_and_escape() {
        let conn = setup();
        let f = SearchFilters::default();
        let r = fts_search(&conn, "authentication", 10, &f).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].chunk_id, "c1");
        assert_eq!(r[0].line_start, 1);
        assert_eq!(r[0].line_end, 5);
        // Special chars must not error.
        let r2 = fts_search(&conn, "C++ foo:bar \"unbalanced", 10, &f).unwrap();
        assert!(r2.is_empty());
    }

    #[test]
    fn fts_search_filters() {
        let conn = setup();
        let f = SearchFilters {
            path_contains: Some("nope".into()),
            ..Default::default()
        };
        assert!(fts_search(&conn, "authentication", 10, &f).unwrap().is_empty());
        let f2 = SearchFilters {
            file_types: Some(vec!["md".into()]),
            ..Default::default()
        };
        assert_eq!(fts_search(&conn, "database", 10, &f2).unwrap().len(), 1);
    }

    #[test]
    fn fts_stays_synced_on_delete() {
        let conn = setup();
        conn.execute("DELETE FROM chunks WHERE id='c1'", [])
            .unwrap();
        let f = SearchFilters::default();
        assert!(fts_search(&conn, "authentication", 10, &f).unwrap().is_empty());
        assert_eq!(fts_search(&conn, "database", 10, &f).unwrap().len(), 1);
    }

    #[test]
    fn fts_rebuild() {
        let conn = setup();
        assert_eq!(rebuild_fts(&conn).unwrap(), 2);
    }
}
