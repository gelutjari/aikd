use anyhow::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use std::collections::HashSet;

use aikd_core::SearchFilters;
use aikd_embedder as embedder;
use aikd_indexer::TantivyEngine;
use aikd_session as session;
use aikd_storage::Database;

use crate::AppState;

/// Stable client identity for this MCP process (one `aikd serve` per session).
fn client_id() -> String {
    format!("mcp-{}", std::process::id())
}

/// Parent process of this MCP server (the agent CLI/IDE that spawned it).
fn parent_process() -> (i64, String) {
    use sysinfo::{Pid, System};
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let me = Pid::from_u32(std::process::id());
    if let Some(proc_) = sys.process(me) {
        if let Some(parent) = proc_.parent() {
            let name = sys
                .process(parent)
                .map(|p| p.name().to_string_lossy().to_string())
                .unwrap_or_default();
            return (parent.as_u32() as i64, name);
        }
    }
    (0, String::new())
}

fn current_cwd() -> String {
    std::env::current_dir()
        .map(|p| aikd_core::platform::canonical_path(&p))
        .unwrap_or_default()
}

/// Best-effort heartbeat: never let telemetry break a tool call.
fn touch(db: &Database, tool: &str, target: &str) {
    let _ = aikd_storage::telemetry::touch_client(db.conn(), &client_id(), tool, target);
}

fn params_path_hint(opts: &aikd_scanner::ScanOptions) -> &str {
    opts.override_path
        .as_ref()
        .and_then(|p| p.to_str())
        .unwrap_or("")
}

/// Record token savings for a query/symbol call.
fn record_query_metrics(
    db: &Database,
    tool: &str,
    query: &str,
    result_paths: &[String],
    returned_bytes: usize,
    elapsed: std::time::Duration,
) {
    let mut full_bytes: i64 = 0;
    let seen: HashSet<&str> = result_paths.iter().map(|s| s.as_str()).collect();
    for path in seen {
        if let Ok(size) = db.conn().query_row(
            "SELECT size FROM files WHERE path=?1",
            rusqlite::params![path],
            |r| r.get::<_, i64>(0),
        ) {
            full_bytes += size;
        }
    }
    if full_bytes == 0 {
        full_bytes = returned_bytes as i64 * 4;
    }
    let _ = aikd_storage::telemetry::record_query_metric(
        db.conn(),
        tool,
        query,
        full_bytes,
        returned_bytes as i64,
        elapsed.as_millis() as i64,
    );
}

/// Render the auto-attached active constraints block.
fn constraints_block(conn: &rusqlite::Connection, project: &str) -> String {
    let items = aikd_storage::memory::attach_constraints(conn, project, 5).unwrap_or_default();
    if items.is_empty() {
        return String::new();
    }
    let mut out = String::from("\nActive constraints (respect these):\n");
    for c in items {
        out.push_str(&format!(
            "- [{}] {}: {}\n",
            c.scope,
            c.title,
            c.body.lines().next().unwrap_or("")
        ));
    }
    out
}

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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryLifecycleParams {
    #[schemars(description = "Action: stale, archive")]
    pub action: String,
    #[schemars(description = "Memory item id")]
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryConflictParams {
    #[schemars(description = "First item id")]
    pub id_a: String,
    #[schemars(description = "Second item id")]
    pub id_b: String,
    #[schemars(description = "Why they conflict")]
    pub note: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EventAppendParams {
    #[schemars(
        description = "Event type: session_start, user_message, assistant_message, tool_call, tool_result, file_change, test_result, task_status, checkpoint"
    )]
    pub event_type: String,
    #[schemars(description = "Workstream (default)")]
    pub workstream: Option<String>,
    #[schemars(description = "Agent id, e.g. agy")]
    pub agent: Option<String>,
    #[schemars(description = "Agent session id")]
    pub session: Option<String>,
    #[schemars(description = "Source reference (file, message id)")]
    pub source_ref: Option<String>,
    #[schemars(description = "JSON payload string")]
    pub payload: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HandoffParams {
    #[schemars(description = "Workstream (default)")]
    pub workstream: Option<String>,
    #[schemars(description = "Recent events to include")]
    pub recent_events: Option<usize>,
}

#[derive(Clone)]
pub struct AikdServer {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AikdServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            capabilities: rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: rmcp::model::Implementation {
                name: "aikd".to_string(),
                title: Some("AIKD — Project Intelligence & Code Search".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "AIKD indexed codebase search, symbols, Atlas, structured memory, and event journal."
                    .to_string(),
            ),
            ..Default::default()
        }
    }

    /// Capture the MCP `clientInfo` at handshake time and register this
    /// session in `active_clients` (Live Mission Control).
    fn initialize(
        &self,
        request: rmcp::model::InitializeRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<
        Output = std::result::Result<rmcp::model::InitializeResult, rmcp::ErrorData>,
    > + Send
           + '_ {
        let client_name = request.client_info.name.clone();
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }
        async move {
            let cfg = self.state.config.lock().await.clone();
            if let Ok(db) = Database::open(&cfg.db_path()) {
                let (parent_pid, parent_name) = parent_process();
                let cwd = current_cwd();
                let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
                let id = client_id();
                let _ = aikd_storage::telemetry::start_client(
                    db.conn(),
                    &aikd_storage::telemetry::ClientStart {
                        client_id: &id,
                        pid: std::process::id() as i64,
                        parent_pid,
                        parent_name: &parent_name,
                        mcp_client_name: &client_name,
                        transport: "mcp-stdio",
                        cwd: &cwd,
                        project: &project,
                    },
                );
            }
            Ok(self.get_info())
        }
    }
}

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
            force: false,
        };

        touch(&database, "scan", params_path_hint(&opts));
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
        touch(&database, "query", &params.query);
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
        let model_dir = cfg.model_path();
        let (source, enabled) = aikd_context::SemanticSource::from_config(&cfg, &model_dir);
        let started = std::time::Instant::now();
        let outcome = match aikd_context::run_query_with_source(
            &database,
            std::sync::Arc::new(tantivy),
            source,
            enabled,
            &req,
        ) {
            Ok(o) => o,
            Err(e) => return format!("Error: {e}"),
        };
        let returned_bytes: usize = outcome.results.iter().map(|r| r.content.len()).sum();
        let paths: Vec<String> = outcome
            .results
            .iter()
            .map(|r| r.file_path.clone())
            .collect();
        record_query_metrics(
            &database,
            "query",
            &params.query,
            &paths,
            returned_bytes,
            started.elapsed(),
        );
        let mut head = String::new();
        if let aikd_context::RetrievalKind::LexicalFallback { reason } = &outcome.retrieval {
            head.push_str(&format!(
                "(Hybrid unavailable: {reason}; used lexical search.)\n"
            ));
        }
        if outcome.truncated_chars > 0 {
            head.push_str(&format!(
                "(Context budget cut {} chars.)\n",
                outcome.truncated_chars
            ));
        }
        let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
        let constraints = constraints_block(database.conn(), &project);
        format!("{head}{}{constraints}", format_results(&outcome.results))
    }

    #[tool(description = "Get statistics about the knowledge base")]
    async fn stats(&self) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        touch(&database, "stats", "");
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
        if !cfg.embedding.enabled {
            return "Error: semantic embedding is disabled (embedding.enabled=false). Set embedding.enabled=true in config to opt in.".to_string();
        }
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        touch(&database, "embed", "");
        let count: i64 = database
            .conn()
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap_or(0);
        if count == 0 {
            return "No chunks to embed. Run scan first.".to_string();
        }
        if cfg.embedding.provider == "external" {
            #[cfg(feature = "semantic-ext")]
            {
                use aikd_core::SemanticProvider;
                let provider = match embedder::ExternalProvider::from_env(
                    &cfg.embedding.endpoint,
                    &cfg.embedding.model,
                    &cfg.embedding.api_key_env,
                ) {
                    Ok(p) => p,
                    Err(e) => return format!("Error: {e}"),
                };
                if !provider.is_available() {
                    return "Error: external provider unavailable (endpoint/key)".to_string();
                }
                let batch = params
                    .batch_size
                    .unwrap_or_else(|| cfg.embedding.batch_size.parse().unwrap_or(32));
                return match embedder::embed_and_store_external(database.conn(), &provider, batch) {
                    Ok(n) => format!("Embedded {n} chunks via {}.", provider.model_id()),
                    Err(e) => format!("Error: {e}"),
                };
            }
            #[cfg(not(feature = "semantic-ext"))]
            {
                return "Error: provider=external needs the semantic-ext build: cargo build --features semantic-ext".to_string();
            }
        }
        let batch_size = params.batch_size.unwrap_or_else(|| {
            aikd_core::ResourceProfile::detect_with_mode(&cfg.resource.mode).batch_size
        });
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
                &aikd_core::platform::canonical_project(&cfg.scan.include_paths),
            ) {
                Ok(s) => s.id,
                Err(e) => return format!("Error: {e}"),
            },
        };

        touch(&database, "remember", &params.role);
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
                &aikd_core::platform::canonical_project(&cfg.scan.include_paths),
            ) {
                Ok(s) => s.id,
                Err(e) => return format!("Error: {e}"),
            },
        };

        let limit = params.limit.unwrap_or(10);
        touch(&database, "recall", &params.query);
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
        let emb_active = crate::semantic_active(&cfg) && profile.embedding_enabled;
        format!(
            "AIKD Daemon Status\nCPU Cores: {}\nRAM: {:.1} GB\nGPU: {}\nEmbedding: {}\nBatch Size: {}\nParallelism: {}\nHNSW M: {}\nREST Port: {}",
            profile.cpu_cores,
            profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            profile.has_gpu,
            emb_active,
            profile.batch_size,
            profile.parallelism,
            profile.hnsw_m,
            cfg.server.rest_port,
        )
    }

    #[tool(
        description = "Look up code symbols (functions, structs, classes) by name with file and line provenance"
    )]
    async fn symbol(&self, Parameters(params): Parameters<SymbolParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let limit = params.limit.unwrap_or(20);
        touch(&database, "symbol", &params.name);
        let started = std::time::Instant::now();
        match aikd_storage::symbols::find_symbols(database.conn(), &params.name, limit) {
            Ok(symbols) if symbols.is_empty() => {
                format!("No symbols matching '{}'. Run scan first.", params.name)
            }
            Ok(symbols) => {
                let returned_bytes: usize = symbols
                    .iter()
                    .map(|s| s.signature.len() + s.name.len())
                    .sum();
                let paths: Vec<String> = symbols.iter().map(|s| s.file_path.clone()).collect();
                record_query_metrics(
                    &database,
                    "symbol",
                    &params.name,
                    &paths,
                    returned_bytes,
                    started.elapsed(),
                );
                let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
                let constraints = constraints_block(database.conn(), &project);
                let mut out = format!(
                    "{} symbol(s) for '{}' (EXTRACTED, heuristic):\n",
                    symbols.len(),
                    params.name
                );
                for s in &symbols {
                    out.push_str(&format!(
                        "{} {} — {}:{}-{} [{}]\n  {}\n",
                        s.kind,
                        s.name,
                        s.file_path,
                        s.line_start,
                        s.line_end,
                        s.language,
                        s.signature
                    ));
                }
                out.push_str(&constraints);
                out
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        description = "Save structured memory: fact, decision, constraint, discovery, task, or checkpoint with provenance"
    )]
    async fn memory_save(&self, Parameters(params): Parameters<MemorySaveParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
        let ws = params.workstream.unwrap_or_else(|| "default".to_string());
        let source = params
            .source
            .unwrap_or_else(|| "agent-asserted".to_string());
        const KINDS: [&str; 6] = [
            "fact",
            "decision",
            "constraint",
            "discovery",
            "task",
            "checkpoint",
        ];
        if !KINDS.contains(&params.kind.as_str()) {
            return format!("Error: invalid kind '{}'", params.kind);
        }
        if !["explicit", "agent-asserted", "inferred"].contains(&source.as_str()) {
            return format!(
                "Error: invalid source '{source}' (use explicit|agent-asserted|inferred)"
            );
        }
        touch(&database, "memory_save", &params.title);
        match aikd_storage::memory::save_memory(
            database.conn(),
            &project,
            &ws,
            &params.kind,
            &params.title,
            &params.body,
            &source,
            0.9,
        ) {
            Ok(m) => format!(
                "Saved {} '{}' [{}] (id {})",
                m.kind, m.title, m.source, m.id
            ),
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
        let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
        let ws = params.workstream.unwrap_or_else(|| "default".to_string());
        touch(&database, "memory_list", &ws);
        match aikd_storage::memory::list_memory(
            database.conn(),
            &project,
            Some(&ws),
            params.kind.as_deref(),
            params.all.unwrap_or(false),
        ) {
            Ok(items) if items.is_empty() => "No memory items.".to_string(),
            Ok(items) => {
                let mut out = String::new();
                for m in &items {
                    out.push_str(&format!(
                        "[{}] {} — {} [{}|{}]\n  {}\n",
                        m.kind,
                        m.title,
                        m.id,
                        m.source,
                        m.status,
                        m.body.lines().next().unwrap_or("")
                    ));
                }
                out
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        description = "Get active tasks and latest checkpoint for a workstream (resume point after compaction)"
    )]
    async fn memory_task(&self, Parameters(params): Parameters<MemoryTaskParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
        let ws = params.workstream.unwrap_or_else(|| "default".to_string());
        touch(&database, "memory_task", &ws);
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
            Some(c) => out.push_str(&format!(
                "\nCHECKPOINT {} — {}\n{}\n",
                c.id, c.title, c.body
            )),
            None => out.push_str("\n(no checkpoint)\n"),
        }
        out
    }

    #[tool(
        description = "Mark a memory item STALE or ARCHIVED (evidence kept, hidden from active views)"
    )]
    async fn memory_lifecycle(
        &self,
        Parameters(params): Parameters<MemoryLifecycleParams>,
    ) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let n = match params.action.as_str() {
            "stale" => aikd_storage::memory::mark_stale(database.conn(), &params.id),
            "archive" => aikd_storage::memory::archive(database.conn(), &params.id),
            other => return format!("Error: unknown action '{other}' (use stale|archive)"),
        };
        touch(&database, "memory_lifecycle", &params.id);
        match n {
            Ok(1) => format!("{}: {}", params.action, params.id),
            Ok(_) => format!(
                "No change (id {} not found or not in a flaggable state)",
                params.id
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        description = "Flag two memory items as CONFLICTED with a note (both preserved, neither becomes fact)"
    )]
    async fn memory_conflict(
        &self,
        Parameters(params): Parameters<MemoryConflictParams>,
    ) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        match aikd_storage::memory::flag_conflict(
            database.conn(),
            &params.id_a,
            &params.id_b,
            &params.note,
        ) {
            Ok(()) => format!(
                "Flagged conflict: {} <-> {} ({})",
                params.id_a, params.id_b, params.note
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        description = "Append a canonical agent event to the journal (AGY-style live adapter). Idempotent on content."
    )]
    async fn event_append(&self, Parameters(params): Parameters<EventAppendParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
        let payload = params.payload.unwrap_or_else(|| "{}".to_string());
        if serde_json::from_str::<serde_json::Value>(&payload).is_err() {
            return "Error: payload must be JSON".to_string();
        }
        touch(&database, "event_append", &params.event_type);
        match aikd_storage::events::append_event(
            database.conn(),
            &project,
            &params.workstream.unwrap_or_else(|| "default".to_string()),
            &params.agent.unwrap_or_else(|| "agy".to_string()),
            &params.session.unwrap_or_default(),
            &params.event_type,
            &params.source_ref.unwrap_or_default(),
            &payload,
        ) {
            Ok(ev) => format!(
                "Event {} seq={} ({}:{})",
                ev.event_id, ev.sequence, ev.agent_id, ev.event_type
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        description = "Build the context package for a workstream: tasks, checkpoint, decisions, constraints, recent events, next actions"
    )]
    async fn handoff(&self, Parameters(params): Parameters<HandoffParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
        let ws = params.workstream.unwrap_or_else(|| "default".to_string());
        let recent = params.recent_events.unwrap_or(30);
        touch(&database, "handoff", &ws);
        match aikd_context::assemble_handoff(database.conn(), &project, &ws, recent) {
            Ok(pkg) => pkg.render_text(),
            Err(e) => format!("Error: {e}"),
        }
    }
}

fn format_results(results: &[aikd_core::SearchResult]) -> String {
    if results.is_empty() {
        return "No results found. Hint: run `stats` (is this the right DB?), `project show`-equivalent memory check, or narrow filters. If nothing is indexed yet, run `scan` first.".to_string();
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
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let (mut client_read, mut client_write) = tokio::io::split(client_io);

    let proxy_task = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let mut stdin_reader = BufReader::new(tokio::io::stdin());
        let mut stdout = tokio::io::stdout();
        let mut initialized_seen = false;
        let mut line = String::new();

        // Forward server_io -> stdout concurrently
        let out_task = tokio::spawn(async move {
            let _ = tokio::io::copy(&mut client_read, &mut stdout).await;
        });

        while let Ok(n) = stdin_reader.read_line(&mut line).await {
            if n == 0 {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                line.clear();
                continue;
            }
            if !initialized_seen {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    let method = val.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    if method == "initialize" {
                        initialized_seen = true;
                    } else if let Some(id) = val.get("id") {
                        // Non-initialize request before handshake (e.g. OMP v19.1.0 "server/discover").
                        // Reply with -32601 Method not found so client falls back to standard "initialize".
                        let resp = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32601,
                                "message": format!("Method not found: {method}")
                            }
                        });
                        let mut out_line = resp.to_string();
                        out_line.push('\n');
                        let _ = tokio::io::stdout().write_all(out_line.as_bytes()).await;
                        let _ = tokio::io::stdout().flush().await;
                        line.clear();
                        continue;
                    } else {
                        // Pre-initialize notification: ignore so rmcp doesn't abort handshake
                        line.clear();
                        continue;
                    }
                }
            }
            if client_write.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = client_write.flush().await;
            line.clear();
        }
        // Signal EOF to the rmcp service and stop the stdout pump. Waiting on
        // `out_task` here would deadlock: it only ends when the service
        // closes, and the service only closes when this side shuts down.
        let _ = client_write.shutdown().await;
        drop(client_write);
        out_task.abort();
    });

    let service = server.serve(server_io).await?;
    // Exit when EITHER the rmcp service stops OR stdin reaches EOF. Without
    // the select a client closing its pipe left the process (and its live
    // row) hanging forever.
    let mut proxy_task = proxy_task;
    tokio::select! {
        res = service.waiting() => {
            if let Err(e) = res {
                log::warn!("mcp: service stopped: {e}");
            }
        }
        _ = &mut proxy_task => {}
    }
    proxy_task.abort();
    // EOF / clean exit: mark this session disconnected so Live Mission
    // Control never shows a dead agent as active.
    let cfg = aikd_core::Config::load_validated(config_path);
    if let Ok(db) = Database::open(&cfg.db_path()) {
        let _ = aikd_storage::telemetry::disconnect_by_pid(db.conn(), std::process::id() as i64);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::AikdServer;
    use rmcp::ServerHandler;

    #[test]
    fn server_info_advertises_tools_capability() {
        let srv = AikdServer::new("~/.aikd/config.yaml".to_string());
        let info = srv.get_info();
        assert!(
            info.capabilities.tools.is_some(),
            "MCP initialize must advertise capabilities.tools so spec clients call tools/list"
        );
        assert_eq!(info.server_info.name, "aikd");
    }
}
