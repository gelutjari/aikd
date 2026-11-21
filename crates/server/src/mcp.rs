use anyhow::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
    ServerHandler, ServiceExt,
};
use serde::Deserialize;

use aikd_core::SearchFilters;
use aikd_embedder as embedder;
use aikd_indexer::TantivyEngine;
use aikd_session as session;
use aikd_storage::Database;

use crate::AppState;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScanParams {
    #[schemars(description = "Optional path to scan")]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryParams {
    #[schemars(description = "Search query string")]
    pub query: String,
    #[schemars(description = "Maximum number of results")]
    pub limit: Option<usize>,
    #[schemars(description = "Filter by file path")]
    pub path_filter: Option<String>,
    #[schemars(description = "Exclude paths matching pattern")]
    pub path_exclude: Option<String>,
    #[schemars(description = "Filter by file extension (e.g. 'rs', 'ts')")]
    pub file_types: Option<Vec<String>>,
    #[schemars(description = "Filter by heading")]
    pub heading_filter: Option<String>,
    #[schemars(description = "Use hybrid search")]
    pub hybrid: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EmbedParams {
    #[schemars(description = "Batch size")]
    pub batch_size: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RememberParams {
    #[schemars(description = "Session ID")]
    pub session_id: Option<String>,
    #[schemars(description = "Role: user, assistant, or system")]
    pub role: String,
    #[schemars(description = "Message content")]
    pub content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecallParams {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Session ID filter")]
    pub session_id: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SymbolParams {
    #[schemars(description = "Symbol name (exact or prefix)")]
    pub name: String,
    #[schemars(description = "Max results")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemorySaveParams {
    #[schemars(description = "Kind: fact, decision, constraint, discovery, task, checkpoint")]
    pub kind: String,
    #[schemars(description = "Short title")]
    pub title: String,
    #[schemars(description = "Full body")]
    pub body: String,
    #[schemars(description = "Workstream (default)")]
    pub workstream: Option<String>,
    #[schemars(description = "Provenance: explicit, agent-asserted, inferred")]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryListParams {
    #[schemars(description = "Kind filter (optional)")]
    pub kind: Option<String>,
    #[schemars(description = "Workstream (default)")]
    pub workstream: Option<String>,
    #[schemars(description = "Include superseded")]
    pub all: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryTaskParams {
    #[schemars(description = "Workstream (default)")]
    pub workstream: Option<String>,
}

#[derive(Clone)]
pub struct AikdServer {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AikdServer {}

#[tool_router(router = tool_router)]
impl AikdServer {
    pub fn new(config_path: String) -> Self {
        let state = AppState::new(config_path);
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Scan and index files into the knowledge base")]
    async fn scan(&self, Parameters(params): Parameters<ScanParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let tantivy = match TantivyEngine::open(&cfg.tantivy_path()) {
            Ok(t) => t,
            Err(e) => return format!("Error: {e}"),
        };

        let override_path = match params.path {
            Some(p) => match aikd_core::security::sanitize_path_input(&p) {
                Ok(safe) => Some(safe),
                Err(e) => return format!("Error: invalid scan path: {e}"),
            },
            None => None,
        };
        let opts = aikd_scanner::ScanOptions {
            override_path,
        };

        match aikd_scanner::run_scan(&cfg, &database, &tantivy, &opts) {
            Ok(progress) => format!(
                "Indexed {} files, {} chunks in {:?}",
                progress.files_indexed, progress.chunks_created, progress.elapsed
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Search the knowledge base with BM25 or hybrid search")]
    async fn query(&self, Parameters(params): Parameters<QueryParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let tantivy = match TantivyEngine::open(&cfg.tantivy_path()) {
            Ok(t) => t,
            Err(e) => return format!("Error: {e}"),
        };

        let filters = SearchFilters {
            path_contains: params.path_filter,
            path_exclude: params.path_exclude,
            file_types: params.file_types,
            heading_contains: params.heading_filter,
        };

        let limit = params.limit.unwrap_or(10);
        let hybrid = params.hybrid.unwrap_or(false);

        let req = aikd_context::QueryRequest {
            query: params.query.clone(),
            filters,
            limit,
            hybrid,
            budget: aikd_context::ContextBudget::default(),
        };
        let outcome = match aikd_context::run_query(
            &database,
            std::sync::Arc::new(tantivy),
            &cfg.model_path(),
            cfg.embedding.enabled,
            &req,
        ) {
            Ok(o) => o,
            Err(e) => return format!("Error: {e}"),
        };
        let mut head = String::new();
        if let aikd_context::RetrievalKind::LexicalFallback { reason } = &outcome.retrieval {
            head.push_str(&format!("(Hybrid unavailable: {reason}; used lexical search.)\n"));
        }
        if outcome.truncated_chars > 0 {
            head.push_str(&format!(
                "(Context budget cut {} chars.)\n",
                outcome.truncated_chars
            ));
        }
        format!("{head}{}", format_results(&outcome.results))
    }

    #[tool(description = "Get statistics about the knowledge base")]
    async fn stats(&self) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let conn = database.conn();
        let fc: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE status='active'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let cc: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap_or(0);
        let ts: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(size),0) FROM files WHERE status='active'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let ec: i64 = conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
            .unwrap_or(0);
        let (sc, convc, ce) = session::get_session_stats(conn).unwrap_or((0, 0, 0));
        format!(
            "AIKD v{}\nFiles: {}\nChunks: {}\nEmbeddings: {} ({}d)\nSessions: {}\nConversations: {}\nConv Embeddings: {}\nSize: {:.1} KB\nDB: {}\nIndex: {}",
            env!("CARGO_PKG_VERSION"),
            fc, cc, ec, embedder::DIMENSIONS, sc, convc, ce, ts as f64 / 1024.0, cfg.index.db_path, cfg.index.tantivy_path
        )
    }

    #[tool(description = "Generate embeddings for all chunks")]
    async fn embed(&self, Parameters(params): Parameters<EmbedParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let count: i64 = database
            .conn()
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap_or(0);
        if count == 0 {
            return "No chunks to embed. Run scan first.".to_string();
        }
        let batch_size = params.batch_size.unwrap_or(32);
        let model_dir = cfg.model_path();
        match embedder::embed_and_store(database.conn(), &model_dir, batch_size) {
            Ok(n) => format!("Embedded {n} chunks. Hybrid search now available."),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Save a conversation message to memory")]
    async fn remember(&self, Parameters(params): Parameters<RememberParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };

        let session_id = match params.session_id {
            Some(id) => id,
            None => match session::get_or_create_session(
                database.conn(),
                &cfg.scan.include_paths.first().cloned().unwrap_or_default(),
            ) {
                Ok(s) => s.id,
                Err(e) => return format!("Error: {e}"),
            },
        };

        match session::remember(
            database.conn(),
            &session_id,
            &params.role,
            &params.content,
            &[],
        ) {
            Ok(conv) => format!(
                "Remembered [{}] {}: {}",
                conv.session_id,
                conv.role,
                conv.content.chars().take(100).collect::<String>()
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Search conversation history and file knowledge")]
    async fn recall(&self, Parameters(params): Parameters<RecallParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };

        let session_id = match params.session_id {
            Some(id) => id,
            None => match session::get_or_create_session(
                database.conn(),
                &cfg.scan.include_paths.first().cloned().unwrap_or_default(),
            ) {
                Ok(s) => s.id,
                Err(e) => return format!("Error: {e}"),
            },
        };

        let limit = params.limit.unwrap_or(10);
        match session::recall(database.conn(), &session_id, &params.query, limit) {
            Ok(convs) => {
                if convs.is_empty() {
                    return "No matching conversations found.".to_string();
                }
                let mut out = String::new();
                for (i, c) in convs.iter().enumerate() {
                    out.push_str(&format!(
                        "{}. [{}] {}: {}\n",
                        i + 1,
                        c.created_at,
                        c.role,
                        c.content.chars().take(200).collect::<String>()
                    ));
                }
                out
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get resource usage and daemon status")]
    async fn status(&self) -> String {
        let cfg = self.state.config.lock().await.clone();
        let profile = aikd_core::ResourceProfile::detect_with_mode(&cfg.resource.mode);
        format!(
            "AIKD Daemon Status\nCPU Cores: {}\nRAM: {:.1} GB\nGPU: {}\nEmbedding: {}\nBatch Size: {}\nParallelism: {}\nHNSW M: {}\nREST Port: {}",
            profile.cpu_cores,
            profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            profile.has_gpu,
            profile.embedding_enabled,
            profile.batch_size,
            profile.parallelism,
            profile.hnsw_m,
            cfg.server.rest_port,
        )
    }

    #[tool(description = "Look up code symbols (functions, structs, classes) by name with file and line provenance")]
    async fn symbol(&self, Parameters(params): Parameters<SymbolParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let limit = params.limit.unwrap_or(20);
        match aikd_storage::symbols::find_symbols(database.conn(), &params.name, limit) {
            Ok(symbols) if symbols.is_empty() => {
                format!("No symbols matching '{}'. Run scan first.", params.name)
            }
            Ok(symbols) => {
                let mut out = format!(
                    "{} symbol(s) for '{}' (EXTRACTED, heuristic):\n",
                    symbols.len(),
                    params.name
                );
                for s in &symbols {
                    out.push_str(&format!(
                        "{} {} — {}:{}-{} [{}]\n  {}\n",
                        s.kind, s.name, s.file_path, s.line_start, s.line_end, s.language, s.signature
                    ));
                }
                out
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Save structured memory: fact, decision, constraint, discovery, task, or checkpoint with provenance")]
    async fn memory_save(&self, Parameters(params): Parameters<MemorySaveParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let project = cfg.scan.include_paths.first().cloned().unwrap_or_default();
        let ws = params.workstream.unwrap_or_else(|| "default".to_string());
        let source = params.source.unwrap_or_else(|| "agent-asserted".to_string());
        const KINDS: [&str; 6] = ["fact", "decision", "constraint", "discovery", "task", "checkpoint"];
        if !KINDS.contains(&params.kind.as_str()) {
            return format!("Error: invalid kind '{}'", params.kind);
        }
        match aikd_storage::memory::save_memory(database.conn(), &project, &ws, &params.kind, &params.title, &params.body, &source, 0.9) {
            Ok(m) => format!("Saved {} '{}' [{}] (id {})", m.kind, m.title, m.source, m.id),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "List structured memory with provenance and lifecycle status")]
    async fn memory_list(&self, Parameters(params): Parameters<MemoryListParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let project = cfg.scan.include_paths.first().cloned().unwrap_or_default();
        let ws = params.workstream.unwrap_or_else(|| "default".to_string());
        match aikd_storage::memory::list_memory(database.conn(), &project, Some(&ws), params.kind.as_deref(), params.all.unwrap_or(false)) {
            Ok(items) if items.is_empty() => "No memory items.".to_string(),
            Ok(items) => {
                let mut out = String::new();
                for m in &items {
                    out.push_str(&format!("[{}] {} — {} [{}|{}]\n  {}\n", m.kind, m.title, m.id, m.source, m.status, m.body.lines().next().unwrap_or("")));
                }
                out
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get active tasks and latest checkpoint for a workstream (resume point after compaction)")]
    async fn memory_task(&self, Parameters(params): Parameters<MemoryTaskParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let project = cfg.scan.include_paths.first().cloned().unwrap_or_default();
        let ws = params.workstream.unwrap_or_else(|| "default".to_string());
        let tasks = match aikd_storage::memory::active_tasks(database.conn(), &project, &ws) {
            Ok(t) => t,
            Err(e) => return format!("Error: {e}"),
        };
        let cp = match aikd_storage::memory::latest_checkpoint(database.conn(), &project, &ws) {
            Ok(c) => c,
            Err(e) => return format!("Error: {e}"),
        };
        let mut out = format!("Workstream: {ws}\n");
        for t in &tasks {
            out.push_str(&format!("\nTASK {} — {}\n{}\n", t.id, t.title, t.body));
        }
        if tasks.is_empty() {
            out.push_str("(no active tasks)\n");
        }
        match &cp {
            Some(c) => out.push_str(&format!("\nCHECKPOINT {} — {}\n{}\n", c.id, c.title, c.body)),
            None => out.push_str("\n(no checkpoint)\n"),
        }
        out
    }
}

fn format_results(results: &[aikd_core::SearchResult]) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }
    let mut out = String::new();
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, r.file_path));
        if !r.heading_text.is_empty() {
            out.push_str(&format!("   Heading: {}\n", r.heading_hierarchy));
        }
        if r.line_start > 0 {
            out.push_str(&format!("   Lines: {}-{}\n", r.line_start, r.line_end));
        }
        if r.score > 0.0 {
            out.push_str(&format!("   Score: {:.3}\n", r.score));
        }
        let preview = if r.content.chars().count() > 200 {
            let end = r
                .content
                .char_indices()
                .nth(200)
                .map(|(i, _)| i)
                .unwrap_or(r.content.len());
            format!("{}...", &r.content[..end])
        } else {
            r.content.clone()
        };
        out.push_str(&format!("   {}\n\n", preview.replace('\n', "\n   ")));
    }
    out
}

pub async fn run_server(config_path: &str) -> Result<()> {
    let server = AikdServer::new(config_path.to_string());
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
