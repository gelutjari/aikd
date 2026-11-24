//! Lean mode for AIKD: zero content duplication in SQLite + FTS5 contentless lexical index.
//!
//! - `chunks.content` is kept empty on new scans (or cleared via `aikd gc --prune-content`).
//! - `chunks_fts(file_path, heading, tokens)` stores only tokenized terms (`content='', contentless_delete=1`).
//! - Snippets are read live from disk by `(file_path, line_start, line_end)`, falling back to `chunks.content`
//!   if non-empty (e.g. in-memory tests or pre-prune rows), and returning a clear message if the file was deleted.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

pub const MAX_SNIPPET_LINES: usize = 120;
pub const MAX_SNIPPET_BYTES: usize = 16 * 1024;

/// Ensure the contentless FTS5 virtual table exists.
pub fn ensure_fts(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(file_path, heading, tokens, content='', contentless_delete=1);",
    )?;
    Ok(())
}

/// Check if `chunks_fts` table exists.
pub fn fts_ready(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks_fts'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

/// Index one chunk into `chunks_fts` using its `chunks.rowid`.
pub fn index_chunk_fts(
    conn: &Connection,
    rowid: i64,
    file_path: &str,
    heading: &str,
    tokens: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM chunks_fts WHERE rowid = ?1",
        rusqlite::params![rowid],
    )?;
    conn.execute(
        "INSERT INTO chunks_fts(rowid, file_path, heading, tokens) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![rowid, file_path, heading, tokens],
    )?;
    Ok(())
}

/// Remove all FTS entries belonging to `file_id`.
pub fn remove_file_fts(conn: &Connection, file_id: i64) -> Result<()> {
    if !fts_ready(conn) {
        return Ok(());
    }
    conn.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks WHERE file_id = ?1)",
        rusqlite::params![file_id],
    )?;
    Ok(())
}

/// Delete specific chunk IDs from `chunks_fts`.
pub fn delete_chunks_fts(conn: &Connection, chunk_ids: &[String]) -> Result<()> {
    if chunk_ids.is_empty() || !fts_ready(conn) {
        return Ok(());
    }
    for id in chunk_ids {
        let _ = conn.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks WHERE id = ?1)",
            rusqlite::params![id],
        );
    }
    Ok(())
}

/// Clear all rows from `chunks_fts`.
pub fn clear_fts(conn: &Connection) -> Result<()> {
    if !fts_ready(conn) {
        return Ok(());
    }
    conn.execute("DELETE FROM chunks_fts", [])?;
    Ok(())
}

/// Backfill `chunks_fts` from existing `chunks` rows (using `chunks.content` or live disk snippet).
pub fn backfill_fts(conn: &Connection) -> Result<usize> {
    ensure_fts(conn)?;
    let rows: Vec<(i64, String, String, String, String, i64, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT c.rowid, f.path, c.heading_hierarchy, c.heading_text, c.content, c.line_start, c.line_end \
             FROM chunks c JOIN files f ON f.id = c.file_id \
             WHERE c.rowid NOT IN (SELECT rowid FROM chunks_fts)",
        )?;
        let mapped = stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?;
        mapped.filter_map(|r| r.ok()).collect()
    };

    let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut count = 0usize;
    let tx = conn.unchecked_transaction()?;
    for (rowid, fp, hh, ht, content, ls, le) in rows {
        let body = if !content.is_empty() {
            content
        } else {
            read_lines_cached(&mut file_cache, &fp, ls as usize, le as usize).unwrap_or_default()
        };
        let heading = format!("{hh} {ht}");
        let tokens = format!("{heading} {body}");
        index_chunk_fts(&tx, rowid, &fp, &heading, &tokens)?;
        count += 1;
    }
    tx.commit()?;
    Ok(count)
}

/// Rebuild `chunks_fts` from scratch (clears FTS, then backfills from `chunks.content` or live disk files).
pub fn rebuild_fts(conn: &Connection) -> Result<usize> {
    ensure_fts(conn)?;
    clear_fts(conn)?;
    backfill_fts(conn)
}

/// Empty `chunks.content` across all rows to reclaim storage space.
pub fn prune_chunk_content(conn: &Connection) -> Result<usize> {
    let n = conn.execute("UPDATE chunks SET content = '' WHERE content != ''", [])?;
    Ok(n)
}

/// Delete all rows from `embeddings` and `conversation_embeddings`.
pub fn prune_semantic(conn: &Connection) -> Result<usize> {
    let n1 = conn.execute("DELETE FROM embeddings", []).unwrap_or(0);
    let n2 = conn
        .execute("DELETE FROM conversation_embeddings", [])
        .unwrap_or(0);
    Ok(n1 + n2)
}

/// Read lines `[line_start..=line_end]` (1-based, inclusive) from `file_path` on disk.
/// If `fallback_content` is non-empty, returns it directly (used for tests or unpruned rows).
pub fn read_snippet_or_fallback(
    file_path: &str,
    line_start: usize,
    line_end: usize,
    fallback_content: &str,
) -> String {
    if !fallback_content.is_empty() {
        return fallback_content.to_string();
    }
    match read_snippet_from_disk(file_path, line_start, line_end) {
        Ok(s) => s,
        Err(msg) => msg,
    }
}

/// Resolve a snippet for a search hit, auto-healing Lean Mode rows whose file vanished.
///
/// `chunks.content` is empty in Lean Mode, so snippets normally come from disk.
/// When the backing file is gone, there is nothing useful to show: mark its
/// `files` row `stale` (best effort, so later queries stop scanning it) and
/// return `None` so the caller can skip the hit instead of emitting an empty or
/// broken snippet. Stored content still wins when present (tests/unpruned rows).
pub fn read_search_snippet(
    conn: &Connection,
    file_path: &str,
    line_start: usize,
    line_end: usize,
    fallback_content: &str,
) -> Option<String> {
    if !fallback_content.is_empty() {
        return Some(fallback_content.to_string());
    }
    if file_path.is_empty() || !Path::new(file_path).exists() {
        mark_file_stale(conn, file_path);
        return None;
    }
    Some(read_snippet_or_fallback(
        file_path,
        line_start,
        line_end,
        fallback_content,
    ))
}

/// Mark a missing file's row `stale` so future lexical queries stop touching it.
/// Existing non-active rows are left untouched; `aikd gc` still deletes stale rows.
fn mark_file_stale(conn: &Connection, file_path: &str) {
    if file_path.is_empty() {
        return;
    }
    let _ = conn.execute(
        "UPDATE files SET status = 'stale' WHERE path = ?1 AND status = 'active'",
        rusqlite::params![file_path],
    );
}

pub fn read_snippet_from_disk(
    file_path: &str,
    line_start: usize,
    line_end: usize,
) -> std::result::Result<String, String> {
    if file_path.is_empty() {
        return Err("[snippet unavailable: empty file path]".to_string());
    }
    let p = Path::new(file_path);
    let raw = match std::fs::read_to_string(p) {
        Ok(s) => s,
        Err(e) => {
            return Err(format!(
                "[file missing or unreadable on disk: {file_path} ({e}) — run `aikd gc`]"
            ))
        }
    };
    Ok(slice_lines(&raw, line_start, line_end))
}

fn read_lines_cached(
    cache: &mut HashMap<String, Option<String>>,
    file_path: &str,
    line_start: usize,
    line_end: usize,
) -> Option<String> {
    let entry = cache
        .entry(file_path.to_string())
        .or_insert_with(|| std::fs::read_to_string(Path::new(file_path)).ok());
    entry
        .as_ref()
        .map(|raw| slice_lines(raw, line_start, line_end))
}

pub fn slice_lines(raw: &str, line_start: usize, line_end: usize) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let start_idx = line_start
        .saturating_sub(1)
        .min(lines.len().saturating_sub(1));
    let mut end_idx = if line_end == 0 || line_end < line_start {
        (start_idx + MAX_SNIPPET_LINES).min(lines.len())
    } else {
        line_end.min(lines.len())
    };
    if end_idx.saturating_sub(start_idx) > MAX_SNIPPET_LINES {
        end_idx = (start_idx + MAX_SNIPPET_LINES).min(lines.len());
    }
    let mut out = lines[start_idx..end_idx].join("\n");
    if out.len() > MAX_SNIPPET_BYTES {
        out = out.chars().take(MAX_SNIPPET_BYTES).collect();
        out.push_str("\n[…truncated]");
    }
    out
}

/// Sanitize an arbitrary user query string into a valid FTS5 MATCH expression.
/// Extracts alphanumeric/underscore tokens and joins them with OR (or prefix match),
/// preventing FTS5 syntax errors on code punctuation like `fn(`, `foo::bar`, `[aikd]`.
pub fn sanitize_fts_query(q: &str) -> String {
    let mut terms = Vec::new();
    for raw in q.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let t = raw.trim();
        if !t.is_empty() {
            terms.push(format!("\"{}\"", t.replace('"', "")));
        }
    }
    terms.join(" OR ")
}

/// Search `chunks_fts` with BM25 ranking, falling back to `LIKE` on `files.path` / `chunks.heading_text`
/// if FTS returns no hits or is empty. Reads snippets live from disk when `chunks.content` is empty.
pub fn search_lexical(
    conn: &Connection,
    query_str: &str,
    limit: usize,
    filters: &aikd_core::SearchFilters,
) -> Result<Vec<aikd_core::SearchResult>> {
    if query_str.trim().is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let fetch_limit = (limit * 4).max(40);
    let mut results = Vec::new();

    if fts_ready(conn) {
        let fts_q = sanitize_fts_query(query_str);
        if !fts_q.is_empty() {
            let sql = "SELECT c.id, f.path, c.heading_hierarchy, c.heading_text, c.content, \
                              c.line_start, c.line_end, (-bm25(chunks_fts)) AS score \
                       FROM chunks_fts \
                       JOIN chunks c ON c.rowid = chunks_fts.rowid \
                       JOIN files f ON f.id = c.file_id \
                       WHERE f.status = 'active' AND chunks_fts MATCH ?1 \
                       ORDER BY score DESC \
                       LIMIT ?2";
            if let Ok(mut stmt) = conn.prepare(sql) {
                if let Ok(rows) =
                    stmt.query_map(rusqlite::params![fts_q, fetch_limit as i64], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, i64>(5)? as usize,
                            r.get::<_, i64>(6)? as usize,
                            r.get::<_, f64>(7)? as f32,
                        ))
                    })
                {
                    let rows: Vec<_> = rows.flatten().collect();
                    for (cid, fp, hj, ht, co, ls, le, score) in rows {
                        let hier: Vec<String> = serde_json::from_str(&hj).unwrap_or_default();
                        let hier_str = if hier.is_empty() {
                            ht.clone()
                        } else {
                            hier.join(" > ")
                        };
                        if !filters.matches_path(&fp)
                            || (!filters.matches_heading(&hier_str)
                                && !filters.matches_heading(&ht))
                        {
                            continue;
                        }
                        let Some(snippet) = read_search_snippet(conn, &fp, ls, le, &co) else {
                            continue;
                        };
                        results.push(aikd_core::SearchResult {
                            chunk_id: cid,
                            file_path: fp,
                            heading_hierarchy: hier_str,
                            heading_text: ht,
                            content: snippet,
                            line_start: ls,
                            line_end: le,
                            score: if score > 0.0 { score } else { 0.001 },
                        });
                        if results.len() >= limit {
                            return Ok(results);
                        }
                    }
                }
            }
        }
    }

    // Fallback: substring LIKE on heading / path / content (when FTS is empty or had 0 matches)
    if results.is_empty() {
        let pat = format!("%{}%", query_str.trim());
        let sql = "SELECT c.id, f.path, c.heading_hierarchy, c.heading_text, c.content, \
                          c.line_start, c.line_end \
                   FROM chunks c JOIN files f ON f.id = c.file_id \
                   WHERE f.status = 'active' AND (c.content LIKE ?1 OR c.heading_text LIKE ?1 OR f.path LIKE ?1) \
                   LIMIT ?2";
        if let Ok(mut stmt) = conn.prepare(sql) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![pat, fetch_limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)? as usize,
                    r.get::<_, i64>(6)? as usize,
                ))
            }) {
                let rows: Vec<_> = rows.flatten().collect();
                for (cid, fp, hj, ht, co, ls, le) in rows {
                    let hier: Vec<String> = serde_json::from_str(&hj).unwrap_or_default();
                    let hier_str = if hier.is_empty() {
                        ht.clone()
                    } else {
                        hier.join(" > ")
                    };
                    if !filters.matches_path(&fp)
                        || (!filters.matches_heading(&hier_str) && !filters.matches_heading(&ht))
                    {
                        continue;
                    }
                    let Some(snippet) = read_search_snippet(conn, &fp, ls, le, &co) else {
                        continue;
                    };
                    results.push(aikd_core::SearchResult {
                        chunk_id: cid,
                        file_path: fp,
                        heading_hierarchy: hier_str,
                        heading_text: ht,
                        content: snippet,
                        line_start: ls,
                        line_end: le,
                        score: 1.0,
                    });
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;
    use aikd_core::SearchFilters;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    /// Insert an active file plus one Lean Mode chunk (`content` usually empty)
    /// and index the chunk's tokens in FTS so `search_lexical` can find it.
    fn seed_chunk(
        conn: &Connection,
        path: &str,
        content: &str,
        tokens: &str,
        line_start: i64,
        line_end: i64,
    ) {
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) \
             VALUES (?1, 1, 't', 't', 'active', 'h')",
            rusqlite::params![path],
        )
        .unwrap();
        let file_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, \
             heading_text, line_start, line_end, content, created_at, updated_at) \
             VALUES ('chunk-1', ?1, 0, '[]', 1, 'Heading', ?2, ?3, ?4, 't', 't')",
            rusqlite::params![file_id, line_start, line_end, content],
        )
        .unwrap();
        let rowid = conn.last_insert_rowid();
        index_chunk_fts(conn, rowid, path, "Heading", tokens).unwrap();
    }

    fn file_status(conn: &Connection, path: &str) -> String {
        conn.query_row(
            "SELECT status FROM files WHERE path = ?1",
            rusqlite::params![path],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn lean_mode_skips_missing_file_and_marks_it_stale() {
        let dir = tempfile::tempdir().unwrap();
        let missing_path = dir.path().join("deleted.md");
        assert!(!missing_path.exists());
        let path = missing_path.to_string_lossy().to_string();

        let conn = setup();
        seed_chunk(&conn, &path, "", "Heading needle", 1, 2);

        let results = search_lexical(&conn, "needle", 10, &SearchFilters::default()).unwrap();

        assert!(
            results.is_empty(),
            "missing Lean Mode file must be skipped, got {results:?}"
        );
        assert_eq!(file_status(&conn, &path), "stale");
    }

    #[test]
    fn lean_mode_returns_live_disk_snippet_for_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let live_path = dir.path().join("live.md");
        std::fs::write(&live_path, "alpha\nbeta\ngamma\n").unwrap();
        let path = live_path.to_string_lossy().to_string();

        let conn = setup();
        seed_chunk(&conn, &path, "", "Heading beta", 2, 3);

        let results = search_lexical(&conn, "beta", 10, &SearchFilters::default()).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "beta\ngamma");
        assert_eq!(results[0].line_start, 2);
        assert_eq!(results[0].line_end, 3);
        assert_eq!(file_status(&conn, &path), "active");
    }

    #[test]
    fn stored_content_still_wins_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing_path = dir.path().join("gone.md");
        let path = missing_path.to_string_lossy().to_string();

        let conn = setup();
        seed_chunk(&conn, &path, "stored snippet", "Heading stored", 1, 1);

        let results = search_lexical(&conn, "stored", 10, &SearchFilters::default()).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "stored snippet");
        assert_eq!(
            file_status(&conn, &path),
            "active",
            "stored content rows must not be marked stale"
        );
    }
}
