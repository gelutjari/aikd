use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

use aikd_chunker as chunker;
use aikd_core::{Config, SearchFilters};
use aikd_embedder as embedder;
use aikd_indexer::TantivyEngine;
use aikd_session as session;
use aikd_storage::Database;
use rusqlite;

#[derive(Clone)]
struct RestState {
    config: Arc<Mutex<Config>>,
    #[allow(dead_code)]
    config_path: String,
}

#[derive(Deserialize)]
struct QueryParams {
    q: String,
    limit: Option<usize>,
    path: Option<String>,
    heading: Option<String>,
    hybrid: Option<bool>,
}

#[derive(Deserialize)]
struct RememberBody {
    session_id: Option<String>,
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct RecallBody {
    query: String,
    session_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ScanBody {
    path: Option<String>,
}

#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

fn check_auth(headers: &HeaderMap, token: &Option<String>) -> bool {
    if let Some(expected) = token {
        if expected.is_empty() {
            return true;
        }
        if let Some(auth) = headers.get("authorization") {
            if let Ok(auth_str) = auth.to_str() {
                return auth_str == format!("Bearer {}", expected) || auth_str == *expected;
            }
            return false;
        }
        return false;
    }
    true
}

async fn handle_query(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<QueryParams>,
) -> Result<Json<ApiResponse<Vec<aikd_core::SearchResult>>>, StatusCode> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let database = Database::open(&cfg.db_path()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let tantivy =
        TantivyEngine::open(&cfg.tantivy_path()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let filters = SearchFilters {
        path_contains: params.path,
        heading_contains: params.heading,
        ..Default::default()
    };
    let limit = params.limit.unwrap_or(10);
    let hybrid = params.hybrid.unwrap_or(false);

    if hybrid {
        let kw_results = tantivy
            .search(&params.q, limit * 2, &filters)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let kw_ids: Vec<String> = kw_results.iter().map(|r| r.chunk_id.clone()).collect();
        let all_embs = embedder::load_all_embeddings(database.conn())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if all_embs.is_empty() {
            return Ok(Json(ApiResponse {
                success: true,
                data: Some(kw_results),
                error: None,
            }));
        }
        let q_emb = kw_results
            .first()
            .and_then(|r| {
                all_embs
                    .iter()
                    .find(|(id, _)| id == &r.chunk_id)
                    .map(|(_, e)| e.clone())
            })
            .unwrap_or_else(|| vec![0.0; embedder::DIMENSIONS]);
        let vec_scored = embedder::vector_search(&q_emb, &all_embs, limit * 2);
        let vec_ids: Vec<String> = vec_scored.iter().map(|(id, _)| id.clone()).collect();
        let fused = embedder::reciprocal_rank_fusion(&kw_ids, &vec_ids, 60);
        let fused_ids: Vec<String> = fused.iter().take(limit).map(|(id, _)| id.clone()).collect();
        let results = load_chunks_rest(database.conn(), &fused_ids);
        Ok(Json(ApiResponse {
            success: true,
            data: Some(results),
            error: None,
        }))
    } else {
        let results = tantivy
            .search(&params.q, limit, &filters)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(ApiResponse {
            success: true,
            data: Some(results),
            error: None,
        }))
    }
}

async fn handle_stats(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let database = Database::open(&cfg.db_path()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    let ec: i64 = conn
        .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
        .unwrap_or(0);
    let (sc, convc, ce) = session::get_session_stats(conn).unwrap_or((0, 0, 0));

    let data = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "files": fc,
        "chunks": cc,
        "embeddings": ec,
        "dimensions": embedder::DIMENSIONS,
        "sessions": sc,
        "conversations": convc,
        "conversation_embeddings": ce,
    });
    Ok(Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
    }))
}

async fn handle_scan(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<ScanBody>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let database = Database::open(&cfg.db_path()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let tantivy =
        TantivyEngine::open(&cfg.tantivy_path()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let scan_paths: Vec<String> = body
        .path
        .map(|p| vec![p])
        .unwrap_or(cfg.scan.include_paths.clone());
    let mut files = Vec::new();
    for sp in &scan_paths {
        let expanded = shellexpand::tilde(sp);
        let root = std::path::Path::new(expanded.as_ref());
        if !root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let fname = entry.file_name().to_str().unwrap_or("");
            if cfg.should_exclude_file(fname) {
                continue;
            }
            if !cfg.scan.include_extensions.iter().any(|ext| {
                entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s == ext.as_str())
                    .unwrap_or(false)
            }) {
                continue;
            }
            files.push(entry.into_path());
        }
    }

    use rayon::prelude::*;
    let indexed: Vec<_> = files
        .par_iter()
        .filter_map(|path| {
            let ps = path.to_string_lossy().to_string();
            let content = std::fs::read_to_string(path).ok()?;
            let chunks = chunker::chunk_file(
                &ps,
                &content,
                cfg.max_chunk_tokens(),
                cfg.min_chunk_tokens(),
            );
            Some((ps, chunks))
        })
        .collect();

    let tx = database
        .begin_transaction()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for (ps, chunks) in &indexed {
        let size = std::fs::metadata(ps).map(|m| m.len()).unwrap_or(0);
        let now = chrono::Utc::now().to_rfc3339();
        if let Ok(old_fid) = tx.conn().query_row::<i64, _, _>(
            "SELECT id FROM files WHERE path=?1",
            rusqlite::params![ps],
            |r| r.get(0),
        ) {
            let _ = tx.conn().execute(
                "DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)",
                rusqlite::params![old_fid],
            );
            let _ = tx.conn().execute(
                "DELETE FROM chunks WHERE file_id=?1",
                rusqlite::params![old_fid],
            );
            let _ = tx
                .conn()
                .execute("DELETE FROM files WHERE id=?1", rusqlite::params![old_fid]);
        }
        let hash = aikd_storage::compute_blake3(std::path::Path::new(ps)).unwrap_or_default();
        let _ = tx.conn().execute("INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1,?2,?3,?4,'active',?5)", rusqlite::params![ps, size as i64, now, now, hash]);
        if let Ok(fid) = tx.conn().query_row(
            "SELECT id FROM files WHERE path=?1",
            rusqlite::params![ps],
            |r| r.get::<_, i64>(0),
        ) {
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
    tx.commit().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tantivy.clear().ok();
    let tc: Vec<(String, String, String, String)> = indexed
        .iter()
        .flat_map(|(p, cs)| {
            cs.iter().map(move |c| {
                (
                    c.id.clone(),
                    p.clone(),
                    c.heading_hierarchy_str(),
                    c.content.clone(),
                )
            })
        })
        .collect();
    tantivy.index_chunks(&tc).ok();

    let total: usize = indexed.iter().map(|(_, c)| c.len()).sum();
    Ok(Json(ApiResponse {
        success: true,
        data: Some(format!("Indexed {} files, {} chunks", indexed.len(), total)),
        error: None,
    }))
}

async fn handle_remember(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<RememberBody>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let database = Database::open(&cfg.db_path()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let session_id = match body.session_id {
        Some(id) => id,
        None => match session::get_or_create_session(
            database.conn(),
            &cfg.scan.include_paths.first().cloned().unwrap_or_default(),
        ) {
            Ok(s) => s.id,
            Err(e) => {
                return Ok(Json(ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                }))
            }
        },
    };

    match session::remember(database.conn(), &session_id, &body.role, &body.content, &[]) {
        Ok(conv) => Ok(Json(ApiResponse {
            success: true,
            data: Some(conv.id),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

async fn handle_recall(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<RecallBody>,
) -> Result<Json<ApiResponse<Vec<aikd_core::Conversation>>>, StatusCode> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let database = Database::open(&cfg.db_path()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let session_id = match body.session_id {
        Some(id) => id,
        None => match session::get_or_create_session(
            database.conn(),
            &cfg.scan.include_paths.first().cloned().unwrap_or_default(),
        ) {
            Ok(s) => s.id,
            Err(e) => {
                return Ok(Json(ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                }))
            }
        },
    };

    let limit = body.limit.unwrap_or(10);
    match session::recall(database.conn(), &session_id, &body.query, limit) {
        Ok(convs) => Ok(Json(ApiResponse {
            success: true,
            data: Some(convs),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

fn load_chunks_rest(conn: &rusqlite::Connection, ids: &[String]) -> Vec<aikd_core::SearchResult> {
    let mut results = Vec::new();
    for id in ids {
        let row = conn.query_row(
            "SELECT c.id,f.path,c.heading_hierarchy,c.heading_text,c.content,c.line_start,c.line_end FROM chunks c JOIN files f ON c.file_id=f.id WHERE c.id=?1",
            rusqlite::params![id],
            |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?, r.get::<_,String>(4)?, r.get::<_,i64>(5)? as usize, r.get::<_,i64>(6)? as usize)),
        );
        if let Ok((cid, fp, hj, ht, co, ls, le)) = row {
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
    }
    results
}

pub async fn run_rest_server(config_path: &str, port: u16) -> anyhow::Result<()> {
    let cfg = Config::load(config_path).unwrap_or_default();
    let cors_origins = cfg.server.cors_origins.clone();

    let state = RestState {
        config: Arc::new(Mutex::new(cfg)),
        config_path: config_path.to_string(),
    };

    let cors = if cors_origins.contains(&"*".to_string()) {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        CorsLayer::permissive()
    };

    let app = Router::new()
        .route("/api/query", get(handle_query))
        .route("/api/stats", get(handle_stats))
        .route("/api/scan", post(handle_scan))
        .route("/api/remember", post(handle_remember))
        .route("/api/recall", post(handle_recall))
        .layer(cors)
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    eprintln!("AIKD REST API listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
