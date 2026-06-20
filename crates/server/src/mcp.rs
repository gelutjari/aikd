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

        let opts = aikd_scanner::ScanOptions {
            override_path: params.path.map(std::path::PathBuf::from),
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

        if hybrid {
            let model_dir = cfg.model_path();
            if !embedder::is_model_downloaded(&model_dir) {
                return "Model not downloaded. Run: aikd model download".to_string();
            }
            let mut model = match embedder::create_model(&model_dir) {
                Ok(m) => m,
                Err(e) => return format!("Error loading model: {e}"),
            };
            let q_emb = match model.embed(vec![params.query.as_str()], None) {
                Ok(mut e) => e.remove(0),
                Err(e) => return format!("Error embedding query: {e}"),
            };
            let vector_index = std::sync::Arc::new(
                match aikd_indexer::VectorIndex::load_from_db(database.conn(), embedder::MODEL_NAME)
                {
                    Ok(v) => v,
                    Err(e) => return format!("Error loading embeddings: {e}"),
                },
            );
            if vector_index.is_empty() {
                let results = match tantivy.search(&params.query, limit, &filters) {
                    Ok(r) => r,
                    Err(e) => return format!("Error: {e}"),
                };
                let enriched = match enrich_lines(database.conn(), &results) {
                    Ok(r) => r,
                    Err(e) => return format!("Error: {e}"),
                };
                return format_results(&enriched);
            }
            let searcher =
                aikd_indexer::HybridSearcher::new(std::sync::Arc::new(tantivy), vector_index);
            let results = match searcher.hybrid_search(&params.query, &q_emb, limit, &filters, 60) {
                Ok(r) => r,
                Err(e) => return format!("Error: {e}"),
            };
            let enriched = match enrich_lines(database.conn(), &results) {
                Ok(r) => r,
                Err(e) => return format!("Error: {e}"),
            };
            format_results(&enriched)
        } else {
            let results = match tantivy.search(&params.query, limit, &filters) {
                Ok(r) => r,
                Err(e) => return format!("Error: {e}"),
            };
            let enriched = match enrich_lines(database.conn(), &results) {
                Ok(r) => r,
                Err(e) => return format!("Error: {e}"),
            };
            format_results(&enriched)
        }
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
                &conv.content[..conv.content.len().min(100)]
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
                        &c.content[..c.content.len().min(200)]
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
}

fn enrich_lines(
    conn: &rusqlite::Connection,
    results: &[aikd_core::SearchResult],
) -> Result<Vec<aikd_core::SearchResult>> {
    let mut enriched = Vec::with_capacity(results.len());
    for r in results {
        let lines = conn
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
