use anyhow::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    schemars, tool, tool_handler, tool_router,
    ServerHandler, ServiceExt,
    transport::stdio,
};
use serde::Deserialize;

use aikd_core::{SearchFilters};
use aikd_storage::Database;
use aikd_indexer::TantivyEngine;
use aikd_embedder as embedder;
use rusqlite;
use aikd_session as session;
use aikd_chunker as chunker;

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
            Err(e) => return format!("Error: {}", e),
        };
        let tantivy = match TantivyEngine::open(&cfg.tantivy_path()) {
            Ok(t) => t,
            Err(e) => return format!("Error: {}", e),
        };

        let scan_paths: Vec<String> = params.path
            .map(|p| vec![p])
            .unwrap_or(cfg.scan.include_paths.clone());

        let mut files = Vec::new();
        for sp in &scan_paths {
            let expanded = shellexpand::tilde(sp);
            let root = std::path::Path::new(expanded.as_ref());
            if !root.exists() { continue; }
            for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
                if !entry.file_type().is_file() { continue; }
                let fname = entry.file_name().to_str().unwrap_or("");
                if cfg.should_exclude_file(fname) || !cfg.matches_filename_filter(fname) { continue; }
                if !cfg.scan.include_extensions.iter().any(|ext| entry.path().extension().and_then(|s| s.to_str()).map(|s| s == ext.as_str()).unwrap_or(false)) { continue; }
                files.push(entry.into_path());
            }
        }

        use rayon::prelude::*;
        let indexed: Vec<_> = files.par_iter().filter_map(|path| {
            let ps = path.to_string_lossy().to_string();
            let content = std::fs::read_to_string(path).ok()?;
            if !cfg.matches_content_filter(&content) { return None; }
            let chunks = chunker::chunk_file(&ps, &content, cfg.max_chunk_tokens(), cfg.min_chunk_tokens());
            Some((ps, chunks))
        }).collect();

        let tx = match database.begin_transaction() {
            Ok(tx) => tx,
            Err(e) => return format!("Error: {}", e),
        };

        for (ps, chunks) in &indexed {
            let size = std::fs::metadata(ps).map(|m| m.len()).unwrap_or(0);
            let now = chrono::Utc::now().to_rfc3339();
            if let Ok(old_fid) = tx.conn().query_row::<i64, _, _>("SELECT id FROM files WHERE path=?1", rusqlite::params![ps], |r| r.get(0)) {
                let _ = tx.conn().execute("DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)", rusqlite::params![old_fid]);
                let _ = tx.conn().execute("DELETE FROM chunks WHERE file_id=?1", rusqlite::params![old_fid]);
                let _ = tx.conn().execute("DELETE FROM files WHERE id=?1", rusqlite::params![old_fid]);
            }
            let hash = aikd_storage::compute_blake3(std::path::Path::new(ps)).unwrap_or_default();
            let _ = tx.conn().execute("INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1,?2,?3,?4,'active',?5)", rusqlite::params![ps, size as i64, now, now, hash]);
            if let Ok(fid) = tx.conn().query_row("SELECT id FROM files WHERE path=?1", rusqlite::params![ps], |r| r.get::<_, i64>(0)) {
                for c in chunks {
                    let hj = serde_json::to_string(&c.heading_hierarchy).unwrap_or_default();
                    let mj = serde_json::to_string(&c.metadata).unwrap_or_default();
                    let _ = tx.conn().execute(
                        "INSERT INTO chunks (id,file_id,chunk_index,heading_hierarchy,heading_level,heading_text,line_start,line_end,content,metadata_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                        rusqlite::params![c.id, fid, c.chunk_index as i64, hj, c.heading_level as i64, c.heading_text, c.line_start as i64, c.line_end as i64, c.content, mj, now, now],
                    );
                }
            }
        }

        if let Err(e) = tx.commit() {
            return format!("Error: {}", e);
        }

        tantivy.clear().ok();
        let tc: Vec<(String, String, String, String)> = indexed.iter()
            .flat_map(|(p, cs)| cs.iter().map(move |c| (c.id.clone(), p.clone(), c.heading_hierarchy_str(), c.content.clone())))
            .collect();
        tantivy.index_chunks(&tc).ok();

        let total: usize = indexed.iter().map(|(_, c)| c.len()).sum();
        format!("Indexed {} files, {} chunks", indexed.len(), total)
    }

    #[tool(description = "Search the knowledge base with BM25 or hybrid search")]
    async fn query(&self, Parameters(params): Parameters<QueryParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {}", e),
        };
        let tantivy = match TantivyEngine::open(&cfg.tantivy_path()) {
            Ok(t) => t,
            Err(e) => return format!("Error: {}", e),
        };

        let filters = SearchFilters {
            path_contains: params.path_filter,
            heading_contains: params.heading_filter,
            ..Default::default()
        };

        let limit = params.limit.unwrap_or(10);
        let hybrid = params.hybrid.unwrap_or(false);

        if hybrid {
            let kw_results = match tantivy.search(&params.query, limit * 2, &filters) {
                Ok(r) => r,
                Err(e) => return format!("Error: {}", e),
            };
            let kw_ids: Vec<String> = kw_results.iter().map(|r| r.chunk_id.clone()).collect();
            let all_embs = match embedder::load_all_embeddings(database.conn()) {
                Ok(e) => e,
                Err(e) => return format!("Error: {}", e),
            };
            if all_embs.is_empty() {
                return format_results(&kw_results);
            }
            let q_emb = kw_results.first()
                .and_then(|r| all_embs.iter().find(|(id, _)| id == &r.chunk_id).map(|(_, e)| e.clone()))
                .unwrap_or_else(|| vec![0.0; embedder::DIMENSIONS]);
            let vec_scored = embedder::vector_search(&q_emb, &all_embs, limit * 2);
            let vec_ids: Vec<String> = vec_scored.iter().map(|(id, _)| id.clone()).collect();
            let fused = embedder::reciprocal_rank_fusion(&kw_ids, &vec_ids, 60);
            let fused_ids: Vec<String> = fused.iter().take(limit).map(|(id, _)| id.clone()).collect();
            let results = match load_chunks_from_db(database.conn(), &fused_ids) {
                Ok(r) => r,
                Err(e) => return format!("Error: {}", e),
            };
            format_results(&results)
        } else {
            let results = match tantivy.search(&params.query, limit, &filters) {
                Ok(r) => r,
                Err(e) => return format!("Error: {}", e),
            };
            let enriched = match enrich_lines(database.conn(), &results) {
                Ok(r) => r,
                Err(e) => return format!("Error: {}", e),
            };
            format_results(&enriched)
        }
    }

    #[tool(description = "Get statistics about the knowledge base")]
    async fn stats(&self) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {}", e),
        };
        let conn = database.conn();
        let fc: i64 = conn.query_row("SELECT COUNT(*) FROM files WHERE status='active'", [], |r| r.get(0)).unwrap_or(0);
        let cc: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap_or(0);
        let ts: i64 = conn.query_row("SELECT COALESCE(SUM(size),0) FROM files WHERE status='active'", [], |r| r.get(0)).unwrap_or(0);
        let ec: i64 = conn.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0)).unwrap_or(0);
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
            Err(e) => return format!("Error: {}", e),
        };
        let count: i64 = database.conn().query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap_or(0);
        if count == 0 {
            return "No chunks to embed. Run scan first.".to_string();
        }
        let batch_size = params.batch_size.unwrap_or(32);
        let model_dir = cfg.model_path();
        match embedder::embed_and_store(database.conn(), &model_dir, batch_size) {
            Ok(n) => format!("Embedded {} chunks. Hybrid search now available.", n),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Save a conversation message to memory")]
    async fn remember(&self, Parameters(params): Parameters<RememberParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {}", e),
        };

        let session_id = match params.session_id {
            Some(id) => id,
            None => match session::get_or_create_session(database.conn(), &cfg.scan.include_paths.first().cloned().unwrap_or_default()) {
                Ok(s) => s.id,
                Err(e) => return format!("Error: {}", e),
            },
        };

        match session::remember(database.conn(), &session_id, &params.role, &params.content, &[]) {
            Ok(conv) => format!("Remembered [{}] {}: {}", conv.session_id, conv.role, &conv.content[..conv.content.len().min(100)]),
            Err(e) => format!("Error: {}", e),
        }
    }

    #[tool(description = "Search conversation history and file knowledge")]
    async fn recall(&self, Parameters(params): Parameters<RecallParams>) -> String {
        let cfg = self.state.config.lock().await.clone();
        let database = match Database::open(&cfg.db_path()) {
            Ok(db) => db,
            Err(e) => return format!("Error: {}", e),
        };

        let session_id = match params.session_id {
            Some(id) => id,
            None => match session::get_or_create_session(database.conn(), &cfg.scan.include_paths.first().cloned().unwrap_or_default()) {
                Ok(s) => s.id,
                Err(e) => return format!("Error: {}", e),
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
                    out.push_str(&format!("{}. [{}] {}: {}\n", i + 1, c.created_at, c.role, &c.content[..c.content.len().min(200)]));
                }
                out
            }
            Err(e) => format!("Error: {}", e),
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

fn load_chunks_from_db(conn: &rusqlite::Connection, ids: &[String]) -> Result<Vec<aikd_core::SearchResult>> {
    let mut results = Vec::new();
    for id in ids {
        let row = conn.query_row(
            "SELECT c.id,f.path,c.heading_hierarchy,c.heading_text,c.content,c.line_start,c.line_end FROM chunks c JOIN files f ON c.file_id=f.id WHERE c.id=?1",
            rusqlite::params![id],
            |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?, r.get::<_,String>(4)?, r.get::<_,i64>(5)? as usize, r.get::<_,i64>(6)? as usize)),
        );
        if let Ok((cid, fp, hj, ht, co, ls, le)) = row {
            let hier: Vec<String> = serde_json::from_str(&hj).unwrap_or_default();
            results.push(aikd_core::SearchResult { chunk_id: cid, file_path: fp, heading_hierarchy: hier.join(" > "), heading_text: ht, content: co, line_start: ls, line_end: le, score: 0.0 });
        }
    }
    Ok(results)
}

fn enrich_lines(conn: &rusqlite::Connection, results: &[aikd_core::SearchResult]) -> Result<Vec<aikd_core::SearchResult>> {
    let mut enriched = Vec::with_capacity(results.len());
    for r in results {
        let lines = conn.query_row(
            "SELECT line_start, line_end FROM chunks WHERE id=?1",
            rusqlite::params![r.chunk_id],
            |row| Ok((row.get::<_, i64>(0)? as usize, row.get::<_, i64>(1)? as usize)),
        ).unwrap_or((0, 0));
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
            let end = r.content.char_indices().nth(200).map(|(i, _)| i).unwrap_or(r.content.len());
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
