//! Unified retrieval service: the single query path shared by CLI, MCP
//! and REST. Eliminates the three hand-rolled hybrid orchestrations.
//!
//! Retrieval order (cheap first): BM25 lexical by default, optional hybrid
//! (semantic). Semantic runs only when explicitly requested AND enabled AND
//! the model is present AND embeddings exist; otherwise lexical fallback.
//! Results are deduplicated and fitted to a [`ContextBudget`].

use aikd_core::{SearchFilters, SearchResult};
use anyhow::Result;
use std::path::Path;

/// How much context a caller wants back.
#[derive(Debug, Clone)]
pub struct ContextBudget {
    /// Max number of results.
    pub max_results: usize,
    /// Max total content characters across all results.
    pub max_chars: usize,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_results: 20,
            max_chars: 12_000,
        }
    }
}

/// One retrieval request.
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub query: String,
    pub filters: SearchFilters,
    pub limit: usize,
    pub hybrid: bool,
    pub budget: ContextBudget,
}

/// What was actually used to answer.
#[derive(Debug, Clone, PartialEq)]
pub enum RetrievalKind {
    /// Pure BM25 lexical search.
    Lexical,
    /// Hybrid BM25 + vector with RRF fusion.
    Hybrid,
    /// Hybrid requested but unavailable; fell back to lexical.
    LexicalFallback { reason: &'static str },
}

impl RetrievalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RetrievalKind::Lexical => "lexical",
            RetrievalKind::Hybrid => "hybrid",
            RetrievalKind::LexicalFallback { .. } => "lexical-fallback",
        }
    }

    pub fn semantic_used(&self) -> bool {
        matches!(self, RetrievalKind::Hybrid)
    }
}

/// The outcome of one retrieval: provenance-tagged results.
#[derive(Debug, Clone)]
pub struct QueryOutcome {
    pub results: Vec<SearchResult>,
    pub retrieval: RetrievalKind,
    /// How many chars were cut to fit the budget.
    pub truncated_chars: usize,
    pub elapsed_ms: u128,
}

fn apply_budget(mut results: Vec<SearchResult>, budget: &ContextBudget) -> (Vec<SearchResult>, usize) {
    results.truncate(budget.max_results);
    let mut used = 0usize;
    let mut cut = 0usize;
    for r in &mut results {
        if used >= budget.max_chars {
            cut += r.content.len();
            r.content.clear();
            continue;
        }
        let room = budget.max_chars - used;
        if r.content.len() > room {
            cut += r.content.len() - room;
            let mut s: String = r.content.chars().take(room).collect();
            s.push_str("\n[…truncated to fit context budget]");
            r.content = s;
        }
        used += r.content.len().min(room);
    }
    (results, cut)
}

/// Run retrieval. `embedding_enabled` gates semantic; `model_dir` is only
/// touched when hybrid is actually attempted (lazy, per-request).
pub fn run_query(
    db: &aikd_storage::Database,
    tantivy: std::sync::Arc<aikd_indexer::TantivyEngine>,
    model_dir: &Path,
    embedding_enabled: bool,
    req: &QueryRequest,
) -> Result<QueryOutcome> {
    let start = std::time::Instant::now();
    let limit = req.limit.min(req.budget.max_results).max(1) * 2;

    let (raw, retrieval) = if req.hybrid {
        match try_hybrid(db, tantivy.clone(), model_dir, embedding_enabled, req, limit) {
            Ok(r) => r,
            Err(reason) => {
                log::info!("hybrid unavailable ({reason}); lexical fallback");
                let results = tantivy.search(&req.query, limit, &req.filters)?;
                let results = enrich(db, &results)?;
                (results, RetrievalKind::LexicalFallback { reason })
            }
        }
    } else {
        let results = tantivy.search(&req.query, limit, &req.filters)?;
        let results = enrich(db, &results)?;
        (results, RetrievalKind::Lexical)
    };

    let (results, truncated_chars) = apply_budget(raw, &req.budget);
    Ok(QueryOutcome {
        results,
        retrieval,
        truncated_chars,
        elapsed_ms: start.elapsed().as_millis(),
    })
}

fn try_hybrid(
    db: &aikd_storage::Database,
    tantivy: std::sync::Arc<aikd_indexer::TantivyEngine>,
    model_dir: &Path,
    embedding_enabled: bool,
    req: &QueryRequest,
    limit: usize,
) -> Result<(Vec<SearchResult>, RetrievalKind), &'static str> {
    if !embedding_enabled {
        return Err("semantic is disabled (embedding.enabled=false)");
    }
    if !aikd_embedder::is_model_downloaded(model_dir) {
        return Err("model not downloaded");
    }
    let mut model =
        aikd_embedder::create_model(model_dir).map_err(|_| "model failed to load")?;
    let q_emb = model
        .embed(vec![req.query.as_str()], None)
        .map_err(|_| "query embedding failed")?
        .pop()
        .ok_or("empty embedding")?;
    let vector_index = aikd_indexer::VectorIndex::load_from_db(db.conn(), aikd_embedder::MODEL_NAME)
        .map_err(|_| "no embeddings")?;
    if vector_index.is_empty() {
        return Err("no embeddings stored");
    }
    let searcher =
        aikd_indexer::HybridSearcher::new(tantivy, std::sync::Arc::new(vector_index));
    let results = searcher
        .hybrid_search(&req.query, &q_emb, limit, &req.filters, 60)
        .map_err(|_| "hybrid search failed")?;
    let results = enrich(db, &results).map_err(|_| "enrich failed")?;
    Ok((results, RetrievalKind::Hybrid))
}

/// Fill line numbers from SQLite (Tantivy docs carry chunk IDs only).
fn enrich(
    db: &aikd_storage::Database,
    results: &[SearchResult],
) -> Result<Vec<SearchResult>> {
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        let (ls, le): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT line_start, line_end FROM chunks WHERE id=?1",
                rusqlite::params![r.chunk_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or((0, 0));
        out.push(SearchResult {
            chunk_id: r.chunk_id.clone(),
            file_path: r.file_path.clone(),
            heading_hierarchy: r.heading_hierarchy.clone(),
            heading_text: r.heading_text.clone(),
            content: r.content.clone(),
            line_start: ls as usize,
            line_end: le as usize,
            score: r.score,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_truncates_and_reports() {
        let mk = |n: usize| SearchResult {
            chunk_id: format!("c{n}"),
            file_path: "f".into(),
            heading_hierarchy: String::new(),
            heading_text: String::new(),
            content: "x".repeat(100),
            line_start: 1,
            line_end: 2,
            score: 1.0,
        };
        let (out, cut) = apply_budget(vec![mk(1), mk(2), mk(3)], &ContextBudget { max_results: 2, max_chars: 150 });
        assert_eq!(out.len(), 2);
        assert!(cut > 0);
        assert!(out[0].content.len() <= 160);
    }
}
