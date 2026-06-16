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

use aikd_core::{Config, SearchFilters};
use aikd_embedder as embedder;
use aikd_indexer::TantivyEngine;
use aikd_session as session;
use aikd_storage::Database;

#[derive(Clone)]
struct RestState {
    config: Arc<Mutex<Config>>,
    database: Arc<Mutex<Database>>,
    tantivy: Arc<TantivyEngine>,
}

#[derive(Deserialize)]
struct QueryParams {
    q: String,
    limit: Option<usize>,
    path: Option<String>,
    exclude: Option<String>,
    #[serde(rename = "type")]
    file_type: Option<String>,
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

fn error_response<T: Serialize>(
    msg: &str,
    _code: StatusCode,
) -> Result<Json<ApiResponse<T>>, StatusCode> {
    Ok(Json(ApiResponse {
        success: false,
        data: None,
        error: Some(msg.to_string()),
    }))
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
        return error_response("Unauthorized", StatusCode::UNAUTHORIZED);
    }

    let database = state.database.lock().await;
    let tantivy = &state.tantivy;

    let filters = SearchFilters {
        path_contains: params.path,
        path_exclude: params.exclude,
        file_types: params.file_type.map(|ft| vec![ft]),
        heading_contains: params.heading,
    };
    let limit = params.limit.unwrap_or(10);
    let hybrid = params.hybrid.unwrap_or(false);

    if hybrid {
        let model_dir = cfg.model_path();
        if !embedder::is_model_downloaded(&model_dir) {
            return Err(StatusCode::PRECONDITION_FAILED);
        }
        let mut model =
            embedder::create_model(&model_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let q_emb = model
            .embed(vec![params.q.as_str()], None)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .remove(0);
        let vector_index = std::sync::Arc::new(
            aikd_indexer::VectorIndex::load_from_db(database.conn(), embedder::MODEL_NAME)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
        if vector_index.is_empty() {
            let results = tantivy
                .search(&params.q, limit, &filters)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            return Ok(Json(ApiResponse {
                success: true,
                data: Some(results),
                error: None,
            }));
        }
        let searcher = aikd_indexer::HybridSearcher::new(state.tantivy.clone(), vector_index);
        let results = searcher
            .hybrid_search(&params.q, &q_emb, limit, &filters, 60)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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

    let database = state.database.lock().await;
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

    let database = state.database.lock().await;

    let opts = aikd_scanner::ScanOptions {
        override_path: body.path.map(std::path::PathBuf::from),
    };

    match aikd_scanner::run_scan(&cfg, &database, &state.tantivy, &opts) {
        Ok(progress) => Ok(Json(ApiResponse {
            success: true,
            data: Some(format!(
                "Indexed {} files, {} chunks in {:?}",
                progress.files_indexed, progress.chunks_created, progress.elapsed
            )),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
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

    let database = state.database.lock().await;
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

    let database = state.database.lock().await;
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

async fn handle_health(State(state): State<RestState>) -> Json<ApiResponse<serde_json::Value>> {
    let cfg = state.config.lock().await.clone();
    Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "rest_port": cfg.server.rest_port,
        })),
        error: None,
    })
}

async fn handle_metrics(State(state): State<RestState>) -> Json<ApiResponse<serde_json::Value>> {
    let cfg = state.config.lock().await.clone();
    let db_size = std::fs::metadata(cfg.db_path())
        .map(|m| m.len())
        .unwrap_or(0);
    Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "db_size_bytes": db_size,
            "rest_port": cfg.server.rest_port,
        })),
        error: None,
    })
}

async fn handle_status(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return error_response("Unauthorized", StatusCode::UNAUTHORIZED);
    }
    let profile = aikd_core::ResourceProfile::detect_with_mode(&cfg.resource.mode);
    Ok(Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "cpu_cores": profile.cpu_cores,
            "ram_gb": profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            "has_gpu": profile.has_gpu,
            "embedding_enabled": profile.embedding_enabled,
            "batch_size": profile.batch_size,
            "parallelism": profile.parallelism,
            "hnsw_m": profile.hnsw_m,
            "rest_port": cfg.server.rest_port,
        })),
        error: None,
    }))
}

async fn handle_embed(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return error_response("Unauthorized", StatusCode::UNAUTHORIZED);
    }

    let database = state.database.lock().await;
    let model_dir = cfg.model_path();
    let start = std::time::Instant::now();
    match embedder::embed_and_store(database.conn(), &model_dir, 32) {
        Ok(n) => Ok(Json(ApiResponse {
            success: true,
            data: Some(serde_json::json!({
                "embedded": n,
                "elapsed_ms": start.elapsed().as_millis(),
            })),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

async fn handle_sessions(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<aikd_core::Session>>>, StatusCode> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return error_response("Unauthorized", StatusCode::UNAUTHORIZED);
    }

    let database = state.database.lock().await;
    match session::list_sessions(database.conn()) {
        Ok(sessions) => Ok(Json(ApiResponse {
            success: true,
            data: Some(sessions),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn run_rest_server(config_path: &str, port: u16) -> anyhow::Result<()> {
    let cfg = Config::load(config_path).unwrap_or_default();
    let cors_origins = cfg.server.cors_origins.clone();

    let database = Arc::new(Mutex::new(Database::open(&cfg.db_path())?));
    let tantivy = Arc::new(TantivyEngine::open(&cfg.tantivy_path())?);

    let state = RestState {
        config: Arc::new(Mutex::new(cfg)),
        database,
        tantivy,
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
        .route("/api/health", get(handle_health))
        .route("/api/metrics", get(handle_metrics))
        .route("/api/query", get(handle_query))
        .route("/api/stats", get(handle_stats))
        .route("/api/status", get(handle_status))
        .route("/api/scan", post(handle_scan))
        .route("/api/embed", post(handle_embed))
        .route("/api/remember", post(handle_remember))
        .route("/api/recall", post(handle_recall))
        .route("/api/sessions", get(handle_sessions))
        .layer(cors)
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    eprintln!("AIKD REST API listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    log::info!("AIKD REST API listening on http://{}", addr);

    // Graceful shutdown on SIGTERM/SIGINT
    let shutdown_signal = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("Failed to install SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => {},
                _ = sigterm.recv() => {},
            }
        }
        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
        }
        log::info!("AIKD REST server shutting down...");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;
    Ok(())
}
