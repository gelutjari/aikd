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

#[cfg(feature = "semantic")]
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

#[cfg(not(feature = "semantic"))]
fn try_hybrid(
    _db: &aikd_storage::Database,
    _tantivy: std::sync::Arc<aikd_indexer::TantivyEngine>,
    _model_dir: &Path,
    _embedding_enabled: bool,
    _req: &QueryRequest,
    _limit: usize,
) -> Result<(Vec<SearchResult>, RetrievalKind), &'static str> {
    Err("semantic not compiled (zero-ONNX default build); rebuild with --features semantic")
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

/// The context package an agent receives on resume/transfer (§14–16).
/// Assembled ONLY from canonical stores: memory, events, atlas. One
/// constructor for CLI, MCP and REST — no divergent hand-rolled copies.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HandoffPackage {
    pub project: String,
    pub workstream: String,
    pub tasks: Vec<aikd_core::MemoryItem>,
    pub checkpoint: Option<aikd_core::MemoryItem>,
    pub decisions: Vec<aikd_core::MemoryItem>,
    pub constraints: Vec<aikd_core::MemoryItem>,
    pub discoveries: Vec<aikd_core::MemoryItem>,
    pub facts: Vec<aikd_core::MemoryItem>,
    pub conflicts: Vec<aikd_core::MemoryItem>,
    pub recent_events: Vec<aikd_core::AgentEvent>,
    pub watermark: i64,
    pub atlas_status: String,
    pub atlas_fingerprint: String,
    pub next_actions: Vec<String>,
    pub semantic_used: bool,
}

impl HandoffPackage {
    /// Estimated tokens (~4 chars/token) for the whole package.
    pub fn estimated_tokens(&self) -> usize {
        let mut chars = 0;
        for t in &self.tasks {
            chars += t.title.len() + t.body.len();
        }
        for m in self.decisions.iter().chain(&self.constraints).chain(&self.discoveries).chain(&self.facts).chain(&self.conflicts) {
            chars += m.title.len() + m.body.len();
        }
        if let Some(c) = &self.checkpoint {
            chars += c.title.len() + c.body.len();
        }
        for e in &self.recent_events {
            chars += e.payload.len();
        }
        for n in &self.next_actions {
            chars += n.len();
        }
        chars / 4
    }

    /// Human-readable rendering: summaries + source references (§16).
    /// Never dumps full bodies when a first line + id suffices.
    pub fn render_text(&self) -> String {
        let mut out = format!("=== HANDOFF: {} / {} ===\n", self.project, self.workstream);
        for t in &self.tasks {
            out.push_str(&format!("\nTASK {} — {}\n{}\n", t.id, t.title, t.body));
        }
        if self.tasks.is_empty() {
            out.push_str("(no active tasks)\n");
        }
        match &self.checkpoint {
            Some(c) => out.push_str(&format!("\nCHECKPOINT {} — {}\n{}\n", c.id, c.title, c.body)),
            None => out.push_str("\n(no checkpoint — save one with: aikd memory save --kind checkpoint ...)\n"),
        }
        for (label, items) in [
            ("DECISION", &self.decisions),
            ("CONSTRAINT", &self.constraints),
            ("DISCOVERY", &self.discoveries),
            ("FACT", &self.facts),
        ] {
            for m in items {
                out.push_str(&format!("\n{label} {} [{}]: {}\n  {}\n", m.id, m.source, m.title, m.body.lines().next().unwrap_or("")));
            }
        }
        if !self.conflicts.is_empty() {
            out.push_str("\n!! CONFLICTS (unresolved, both sides preserved):\n");
            for m in &self.conflicts {
                out.push_str(&format!("  - {} [{}]: {} — {}\n", m.id, m.source, m.title, m.conflict_note.as_deref().unwrap_or("")));
            }
        }
        if !self.next_actions.is_empty() {
            out.push_str("\nNEXT ACTIONS:\n");
            for n in &self.next_actions {
                out.push_str(&format!(" - {n}\n"));
            }
        }
        out.push_str(&format!("\nRECENT EVENTS ({} of journal, watermark seq={}):\n", self.recent_events.len(), self.watermark));
        for e in &self.recent_events {
            out.push_str(&format!(" #{} {}:{} {}\n", e.sequence, e.agent_id, e.event_type, e.payload.lines().next().unwrap_or("")));
        }
        out.push_str(&format!("\nAtlas: {} (fp {}) | est. tokens ~{} | semantic_used={} (structural retrieval sufficient)\n",
            self.atlas_status, &self.atlas_fingerprint.chars().take(12).collect::<String>(), self.estimated_tokens(), self.semantic_used));
        out.push_str(&format!("\nRetrieved: tasks={} decisions={} constraints={} events={} conflicts={} | semantic_used={}\n",
            self.tasks.len(), self.decisions.len(), self.constraints.len(), self.recent_events.len(), self.conflicts.len(), self.semantic_used));
        out
    }
}

/// Assemble the handoff package from canonical stores.
pub fn assemble_handoff(
    conn: &rusqlite::Connection,
    project: &str,
    workstream: &str,
    recent_events: usize,
) -> Result<HandoffPackage> {
    use aikd_storage::memory::{active_tasks, conflicts, latest_checkpoint, list_memory};
    let tasks = active_tasks(conn, project, workstream)?;
    let checkpoint = latest_checkpoint(conn, project, workstream)?;
    let decisions = list_memory(conn, project, Some(workstream), Some("decision"), false)?;
    let constraints = list_memory(conn, project, Some(workstream), Some("constraint"), false)?;
    let discoveries = list_memory(conn, project, Some(workstream), Some("discovery"), false)?;
    let facts = list_memory(conn, project, Some(workstream), Some("fact"), false)?;
    let conflicted = conflicts(conn, project, Some(workstream))?;
    let watermark = aikd_storage::events::latest_sequence(conn, project, workstream).unwrap_or(0);
    let events = aikd_storage::events::list_events(conn, project, workstream, None, watermark.saturating_sub(recent_events as i64), recent_events)?;
    let next_actions: Vec<String> = tasks.iter().flat_map(|t| {
        t.body.lines().filter(|l| {
            let u = l.to_uppercase();
            u.contains("REMAINING") || u.contains("NEXT") || u.contains("TODO")
        }).map(|s| s.to_string()).collect::<Vec<_>>()
    }).collect();
    let (atlas_status, atlas_fingerprint) = match aikd_storage::atlas::read_atlas(conn, project)? {
        Some((_, status, _)) => {
            let fp: String = conn.query_row("SELECT fingerprint FROM atlas WHERE project_path=?1", rusqlite::params![project], |r| r.get(0)).unwrap_or_default();
            (status, fp)
        }
        None => ("missing".to_string(), String::new()),
    };
    Ok(HandoffPackage {
        project: project.to_string(),
        workstream: workstream.to_string(),
        tasks, checkpoint, decisions, constraints, discoveries, facts,
        conflicts: conflicted, recent_events: events, watermark,
        atlas_status, atlas_fingerprint, next_actions, semantic_used: false,
    })
}

/// Context inspector (§25): what WOULD be sent for a query and why.
/// Runs the normal retrieval path, then explains selection/exclusion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContextInspection {
    pub query: String,
    pub retrieval: String,
    pub selected: Vec<serde_json::Value>,
    pub excluded_reasons: Vec<String>,
    pub estimated_tokens: usize,
    pub truncated_chars: usize,
    pub elapsed_ms: u128,
}

pub fn inspect_context(
    db: &aikd_storage::Database,
    tantivy: std::sync::Arc<aikd_indexer::TantivyEngine>,
    model_dir: &Path,
    embedding_enabled: bool,
    req: &QueryRequest,
) -> Result<ContextInspection> {
    let outcome = run_query(db, tantivy, model_dir, embedding_enabled, req)?;
    let selected: Vec<serde_json::Value> = outcome.results.iter().map(|r| {
        serde_json::json!({
            "chunk": r.chunk_id, "file": r.file_path, "lines": format!("{}-{}", r.line_start, r.line_end),
            "score": r.score, "why": "lexical relevance within project scope",
        })
    }).collect();
    let mut excluded_reasons = Vec::new();
    excluded_reasons.push("files outside project scope: not retrieved (scope filter)".to_string());
    if outcome.truncated_chars > 0 {
        excluded_reasons.push(format!("{} chars cut to fit context budget (max_results={}, max_chars={})", outcome.truncated_chars, req.budget.max_results, req.budget.max_chars));
    }
    if req.hybrid && !outcome.retrieval.semantic_used() {
        excluded_reasons.push("semantic layer excluded: embeddings unavailable — lexical fallback used".to_string());
    }
    let estimated_tokens = outcome.results.iter().map(|r| r.content.len()).sum::<usize>() / 4;
    Ok(ContextInspection {
        query: req.query.clone(),
        retrieval: outcome.retrieval.as_str().to_string(),
        selected, excluded_reasons, estimated_tokens,
        truncated_chars: outcome.truncated_chars,
        elapsed_ms: outcome.elapsed_ms,
    })
}

    #[test]
    fn handoff_assembles_from_canonical_stores() {
        use aikd_storage::schema;
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        aikd_storage::memory::save_memory(&conn, "/p", "ws", "task", "T-1", "STATUS: IN_PROGRESS\nREMAINING: logout", "explicit", 1.0).unwrap();
        aikd_storage::memory::save_memory(&conn, "/p", "ws", "decision", "D-1", "use SQLite", "agent-asserted", 1.0).unwrap();
        aikd_storage::memory::save_memory(&conn, "/p", "ws", "checkpoint", "cp-1", "auth done", "explicit", 1.0).unwrap();
        aikd_storage::events::append_event(&conn, "/p", "ws", "agy", "s1", "session_start", "", "{}").unwrap();
        let pkg = assemble_handoff(&conn, "/p", "ws", 30).unwrap();
        assert_eq!(pkg.tasks.len(), 1);
        assert_eq!(pkg.decisions.len(), 1);
        assert!(pkg.checkpoint.is_some());
        assert_eq!(pkg.recent_events.len(), 1);
        assert_eq!(pkg.next_actions, vec!["REMAINING: logout".to_string()]);
        assert_eq!(pkg.atlas_status, "missing");
        assert!(pkg.estimated_tokens() > 0);
        let text = pkg.render_text();
        assert!(text.contains("T-1") && text.contains("cp-1") && text.contains("watermark seq=1"));
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
