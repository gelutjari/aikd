use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post, put},
    Router,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use prometheus::{Encoder, Gauge, IntCounter, IntGauge, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

use aikd_core::{Config, SearchFilters};
use aikd_embedder as embedder;
use aikd_indexer::TantivyEngine;
use aikd_session as session;
use aikd_storage::Database;

/// JWT Claims structure
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Claims {
    sub: String,  // Subject (user identifier)
    role: String, // User role (admin, user, readonly)
    exp: usize,   // Expiration time
    iat: usize,   // Issued at
}

/// JWT token response
#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: usize,
}

/// Login request
#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

/// JWT configuration
struct JwtConfig {
    secret: String,
    expiration_hours: usize,
}

impl JwtConfig {
    fn from_config(config: &Config) -> Self {
        // Use auth_token as JWT secret if available, otherwise use a default
        // In production, users should set a proper JWT secret
        let secret = config
            .server
            .auth_token
            .clone()
            .unwrap_or_else(|| "aikd-default-jwt-secret-change-me".to_string());
        Self {
            secret,
            expiration_hours: 24,
        }
    }

    fn generate_token(&self, username: &str, role: &str) -> Result<String, StatusCode> {
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = Claims {
            sub: username.to_string(),
            role: role.to_string(),
            iat: now,
            exp: now + (self.expiration_hours * 3600),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    }

    fn validate_token(&self, token: &str) -> Result<Claims, StatusCode> {
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &Validation::default(),
        )
        .map(|data| data.claims)
        .map_err(|_| StatusCode::UNAUTHORIZED)
    }
}

/// Prometheus metrics for monitoring
struct Metrics {
    registry: Registry,
    query_total: IntCounter,
    query_cache_hits: IntCounter,
    query_cache_misses: IntCounter,
    query_duration_seconds: prometheus::Histogram,
    files_indexed: IntGauge,
    chunks_indexed: IntGauge,
    embeddings_count: IntGauge,
    active_sessions: IntGauge,
    db_size_bytes: Gauge,
}

impl Metrics {
    fn new() -> Self {
        let registry = Registry::new();

        let query_total =
            IntCounter::new("aikd_query_total", "Total number of search queries").unwrap();

        let query_cache_hits = IntCounter::new(
            "aikd_query_cache_hits_total",
            "Total number of query cache hits",
        )
        .unwrap();

        let query_cache_misses = IntCounter::new(
            "aikd_query_cache_misses_total",
            "Total number of query cache misses",
        )
        .unwrap();

        let query_duration_seconds = prometheus::Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "aikd_query_duration_seconds",
                "Query duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
        )
        .unwrap();

        let files_indexed = IntGauge::new("aikd_files_indexed", "Number of indexed files").unwrap();

        let chunks_indexed =
            IntGauge::new("aikd_chunks_indexed", "Number of indexed chunks").unwrap();

        let embeddings_count =
            IntGauge::new("aikd_embeddings_count", "Number of embeddings").unwrap();

        let active_sessions =
            IntGauge::new("aikd_active_sessions", "Number of active sessions").unwrap();

        let db_size_bytes =
            Gauge::new("aikd_db_size_bytes", "Database file size in bytes").unwrap();

        registry.register(Box::new(query_total.clone())).unwrap();
        registry
            .register(Box::new(query_cache_hits.clone()))
            .unwrap();
        registry
            .register(Box::new(query_cache_misses.clone()))
            .unwrap();
        registry
            .register(Box::new(query_duration_seconds.clone()))
            .unwrap();
        registry.register(Box::new(files_indexed.clone())).unwrap();
        registry.register(Box::new(chunks_indexed.clone())).unwrap();
        registry
            .register(Box::new(embeddings_count.clone()))
            .unwrap();
        registry
            .register(Box::new(active_sessions.clone()))
            .unwrap();
        registry.register(Box::new(db_size_bytes.clone())).unwrap();

        Self {
            registry,
            query_total,
            query_cache_hits,
            query_cache_misses,
            query_duration_seconds,
            files_indexed,
            chunks_indexed,
            embeddings_count,
            active_sessions,
            db_size_bytes,
        }
    }
}

/// Cache key for query results
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct QueryCacheKey {
    query: String,
    limit: usize,
    path: Option<String>,
    exclude: Option<String>,
    file_type: Option<String>,
    heading: Option<String>,
    hybrid: bool,
}

#[derive(Clone)]
struct RestState {
    config: Arc<Mutex<Config>>,
    config_path: String,
    database: Arc<Mutex<Database>>,
    tantivy: Arc<TantivyEngine>,
    /// Query cache: 1000 entries, 5 minute TTL
    query_cache: Arc<moka::future::Cache<QueryCacheKey, Vec<aikd_core::SearchResult>>>,
    /// Prometheus metrics
    metrics: Arc<Metrics>,
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
    project: Option<String>,
}

/// `GET /api/query` / `GET /api/symbols` payload: the historical array stays
/// in `data`, and `attached_constraints` is added as a sibling field so old
/// clients keep working while agents get their guardrails automatically.
#[derive(Serialize)]
struct QueryApiResponse {
    success: bool,
    data: Option<Vec<aikd_core::SearchResult>>,
    error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attached_constraints: Vec<aikd_storage::memory::ConstraintRef>,
}

#[derive(Serialize)]
struct SymbolApiResponse {
    success: bool,
    data: Option<Vec<aikd_core::CodeSymbol>>,
    error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attached_constraints: Vec<aikd_storage::memory::ConstraintRef>,
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

/// API error that carries both HTTP status and a JSON body.
/// Replaces the old pattern of returning HTTP 200 with an error payload.
struct ApiError {
    code: StatusCode,
    message: String,
}

impl ApiError {
    fn new(code: StatusCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = Json(ApiResponse::<serde_json::Value> {
            success: false,
            data: None,
            error: Some(self.message),
        });
        (self.code, body).into_response()
    }
}

impl From<StatusCode> for ApiError {
    fn from(code: StatusCode) -> Self {
        let message = code.canonical_reason().unwrap_or("error").to_string();
        Self { code, message }
    }
}

const UNAUTHORIZED_MSG: &str = "Unauthorized: missing/invalid Bearer token, or no server.auth_token configured. Set server.auth_token in config.yaml and send Authorization: Bearer <token>.";

fn unauthorized() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, UNAUTHORIZED_MSG)
}

/// Validate authentication token from request headers.
/// Since the REST server binds strictly to `127.0.0.1`, if `server.auth_token`
/// is not set (None or empty), local dashboard/API requests are permitted.
/// When `server.auth_token` is configured, a valid Bearer token or JWT is required.
fn check_auth(headers: &HeaderMap, token: &Option<String>) -> bool {
    let expected = match token {
        Some(t) if !t.is_empty() => t,
        _ => return true,
    };
    // Token is configured and non-empty — require valid Bearer token
    if let Some(auth) = headers.get("authorization") {
        if let Ok(auth_str) = auth.to_str() {
            let provided = auth_str.strip_prefix("Bearer ").unwrap_or(auth_str);
            // Try simple token match first
            if constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
                return true;
            }
            // Try JWT validation
            let jwt_config = JwtConfig {
                secret: expected.clone(),
                expiration_hours: 24,
            };
            if jwt_config.validate_token(provided).is_ok() {
                return true;
            }
            return false;
        }
    }
    false
}

/// Constant-time string comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// Estimated token savings for a REST query/symbol call.
fn record_query_metric(
    conn: &rusqlite::Connection,
    tool: &str,
    query: &str,
    result_paths: &[String],
    returned_bytes: usize,
    elapsed: std::time::Duration,
) {
    let seen: HashSet<&str> = result_paths.iter().map(|s| s.as_str()).collect();
    let mut full_bytes: i64 = 0;
    for path in seen {
        if let Ok(size) = conn.query_row(
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
        conn,
        tool,
        query,
        full_bytes,
        returned_bytes as i64,
        elapsed.as_millis() as i64,
    );
}

fn constraints_for(
    conn: &rusqlite::Connection,
    project: &str,
) -> Vec<aikd_storage::memory::ConstraintRef> {
    aikd_storage::memory::attach_constraints(conn, project, 5).unwrap_or_default()
}

/// Handle search query with caching and metrics support.
/// Cache key includes all query parameters to ensure correct cache hits.
async fn handle_query(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<QueryParams>,
) -> Result<Json<QueryApiResponse>, ApiError> {
    let start = std::time::Instant::now();
    state.metrics.query_total.inc();

    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }

    let limit = params.limit.unwrap_or(10);
    let hybrid = params.hybrid.unwrap_or(false);
    let project = params
        .project
        .clone()
        .map(|p| aikd_core::platform::normalize_project_key(&p))
        .unwrap_or_else(|| aikd_core::platform::canonical_project(&cfg.scan.include_paths));

    // Build cache key
    let cache_key = QueryCacheKey {
        query: params.q.clone(),
        limit,
        path: params.path.clone(),
        exclude: params.exclude.clone(),
        file_type: params.file_type.clone(),
        heading: params.heading.clone(),
        hybrid,
    };

    // Check cache first
    if let Some(cached) = state.query_cache.get(&cache_key).await {
        state.metrics.query_cache_hits.inc();
        state
            .metrics
            .query_duration_seconds
            .observe(start.elapsed().as_secs_f64());
        log::debug!("Cache hit for query: {}", params.q);
        let database = state.database.lock().await;
        let attached_constraints = constraints_for(database.conn(), &project);
        return Ok(Json(QueryApiResponse {
            success: true,
            data: Some(cached),
            error: None,
            attached_constraints,
        }));
    }

    state.metrics.query_cache_misses.inc();

    let database = state.database.lock().await;

    let filters = SearchFilters {
        path_contains: params.path.clone(),
        path_exclude: params.exclude.clone(),
        file_types: params.file_type.clone().map(|ft| vec![ft]),
        heading_contains: params.heading.clone(),
    };

    let req = aikd_context::QueryRequest {
        query: params.q.clone(),
        filters,
        limit,
        hybrid,
        budget: aikd_context::ContextBudget::default(),
    };
    let model_dir = cfg.model_path();
    let (source, enabled) = aikd_context::SemanticSource::from_config(&cfg, &model_dir);
    let outcome = aikd_context::run_query_with_source(
        &database,
        state.tantivy.clone(),
        source,
        enabled,
        &req,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let results = outcome.results;

    let returned_bytes: usize = results.iter().map(|r| r.content.len()).sum();
    let paths: Vec<String> = results.iter().map(|r| r.file_path.clone()).collect();
    record_query_metric(
        database.conn(),
        "query",
        &params.q,
        &paths,
        returned_bytes,
        start.elapsed(),
    );
    let attached_constraints = constraints_for(database.conn(), &project);

    // Store in cache
    state.query_cache.insert(cache_key, results.clone()).await;
    state
        .metrics
        .query_duration_seconds
        .observe(start.elapsed().as_secs_f64());
    log::debug!("Cached query result: {}", params.q);

    Ok(Json(QueryApiResponse {
        success: true,
        data: Some(results),
        error: None,
        attached_constraints,
    }))
}

#[derive(Deserialize)]
struct GraphParams {
    path: Option<String>,
    project: Option<String>,
    mode: Option<String>,
    focus: Option<String>,
    depth: Option<usize>,
    types: Option<String>,
    hide_orphans: Option<bool>,
    limit: Option<usize>,
}

async fn handle_graph(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<GraphParams>,
) -> Result<Json<ApiResponse<aikd_storage::graph::FileGraph>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    // Empty project/path → rich GLOBAL graph (never zero nodes).
    let project = params.project.or(params.path).unwrap_or_default();
    let types = params.types.map(|t| {
        t.split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect::<Vec<String>>()
    });
    let opts = aikd_storage::graph::GraphOptions {
        project,
        mode: params
            .mode
            .unwrap_or_else(|| "global".to_string())
            .to_lowercase(),
        focus: params.focus,
        depth: params.depth.unwrap_or(1),
        types,
        hide_orphans: params.hide_orphans.unwrap_or(false),
        limit: params.limit.unwrap_or(120),
    };
    let database = state.database.lock().await;
    match aikd_storage::graph::knowledge_graph(database.conn(), &opts) {
        Ok(g) => Ok(Json(ApiResponse {
            success: true,
            data: Some(g),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

#[derive(Deserialize)]
struct GraphNodeParams {
    id: String,
}

async fn handle_graph_node(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<GraphNodeParams>,
) -> Result<Json<ApiResponse<aikd_storage::graph::NodeDetail>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    match aikd_storage::graph::node_detail(database.conn(), &params.id) {
        Ok(Some(detail)) => Ok(Json(ApiResponse {
            success: true,
            data: Some(detail),
            error: None,
        })),
        Ok(None) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(format!("Node '{}' not found in the graph", params.id)),
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

async fn handle_stats(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
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
) -> Result<Json<ApiResponse<String>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }

    let database = state.database.lock().await;

    let override_path = match body.path {
        Some(p) => {
            let safe = aikd_core::security::sanitize_path_input(&p).map_err(|e| {
                ApiError::new(StatusCode::BAD_REQUEST, format!("Invalid scan path: {e}"))
            })?;
            Some(safe)
        }
        None => None,
    };
    let opts = aikd_scanner::ScanOptions {
        override_path,
        force: false,
    };

    match aikd_scanner::run_scan(&cfg, &database, &state.tantivy, &opts) {
        Ok(progress) => {
            state.query_cache.invalidate_all();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(format!(
                    "Indexed {} files, {} chunks in {:?}",
                    progress.files_indexed, progress.chunks_created, progress.elapsed
                )),
                error: None,
            }))
        }
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
) -> Result<Json<ApiResponse<String>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }

    let database = state.database.lock().await;
    let session_id = match body.session_id {
        Some(id) => id,
        None => match session::get_or_create_session(
            database.conn(),
            &aikd_core::platform::canonical_project(&cfg.scan.include_paths),
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
) -> Result<Json<ApiResponse<Vec<aikd_core::Conversation>>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }

    let database = state.database.lock().await;
    let session_id = match body.session_id {
        Some(id) => id,
        None => match session::get_or_create_session(
            database.conn(),
            &aikd_core::platform::canonical_project(&cfg.scan.include_paths),
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

/// Prometheus metrics endpoint.
/// Returns metrics in Prometheus text format.
async fn handle_metrics(State(state): State<RestState>, headers: HeaderMap) -> impl IntoResponse {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return unauthorized().into_response();
    }
    let db_size = std::fs::metadata(cfg.db_path())
        .map(|m| m.len())
        .unwrap_or(0);

    // Update gauge metrics
    state.metrics.db_size_bytes.set(db_size as f64);

    // Get database stats
    let database = state.database.lock().await;
    let conn = database.conn();

    let files_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM files WHERE status='active'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let chunks_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap_or(0);
    let embeddings_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
        .unwrap_or(0);
    let (sessions, _, _) = session::get_session_stats(conn).unwrap_or((0, 0, 0));

    state.metrics.files_indexed.set(files_count);
    state.metrics.chunks_indexed.set(chunks_count);
    state.metrics.embeddings_count.set(embeddings_count);
    state.metrics.active_sessions.set(sessions);

    // Encode metrics
    let encoder = TextEncoder::new();
    let metric_families = state.metrics.registry.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    // Use static content type string
    let content_type = match encoder.format_type() {
        "text/plain" => "text/plain; version=0.0.4; charset=utf-8",
        "application/vnd.google.protobuf" => "application/vnd.google.protobuf; proto=io.prometheus.client.MetricFamily; encoding=delimited",
        _ => "text/plain; version=0.0.4; charset=utf-8",
    };

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, content_type)],
        buffer,
    )
        .into_response()
}
async fn handle_status(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let profile = aikd_core::ResourceProfile::detect_with_mode(&cfg.resource.mode);
    let emb_active = crate::semantic_active(&cfg) && profile.embedding_enabled;
    Ok(Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "cpu_cores": profile.cpu_cores,
            "ram_gb": profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            "has_gpu": profile.has_gpu,
            "embedding_enabled": emb_active,
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
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    if !cfg.embedding.enabled {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some("Semantic embedding is disabled (embedding.enabled=false). Set embedding.enabled=true in config to opt in.".to_string()),
        }));
    }

    let database = state.database.lock().await;
    let model_dir = cfg.model_path();
    let start = std::time::Instant::now();
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
                Err(e) => {
                    return Ok(Json(ApiResponse {
                        success: false,
                        data: None,
                        error: Some(e.to_string()),
                    }))
                }
            };
            let batch = cfg.embedding.batch_size.parse().unwrap_or(32);
            return match embedder::embed_and_store_external(database.conn(), &provider, batch) {
                Ok(n) => Ok(Json(ApiResponse {
                    success: true,
                    data: Some(serde_json::json!({
                        "embedded": n,
                        "provider": provider.name(),
                        "elapsed_ms": start.elapsed().as_millis(),
                    })),
                    error: None,
                })),
                Err(e) => Ok(Json(ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                })),
            };
        }
        #[cfg(not(feature = "semantic-ext"))]
        {
            return Ok(Json(ApiResponse {
                success: false,
                data: None,
                error: Some("provider=external needs the semantic-ext build: cargo build --features semantic-ext".to_string()),
            }));
        }
    }
    let batch = aikd_core::ResourceProfile::detect_with_mode(&cfg.resource.mode).batch_size;
    match embedder::embed_and_store(database.conn(), &model_dir, batch) {
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
) -> Result<Json<ApiResponse<Vec<aikd_core::Session>>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
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

/// Login endpoint for JWT token generation.
/// This is a simplified auth endpoint. In production, integrate with proper user management.
async fn handle_login(
    State(state): State<RestState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<ApiResponse<TokenResponse>>, StatusCode> {
    let cfg = state.config.lock().await.clone();

    // Simple auth check - in production, use proper user database
    // For now, use the auth_token as a shared secret
    let expected_token = cfg.server.auth_token.clone().unwrap_or_default();

    if expected_token.is_empty() {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(
                "Authentication not configured. Set server.auth_token in config.yaml".to_string(),
            ),
        }));
    }

    // Simple validation: username must match "admin" and password must match auth_token
    // In production, use bcrypt/argon2 for password hashing
    if body.username != "admin" || body.password != expected_token {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some("Invalid credentials".to_string()),
        }));
    }

    let jwt_config = JwtConfig::from_config(&cfg);
    match jwt_config.generate_token(&body.username, "admin") {
        Ok(token) => Ok(Json(ApiResponse {
            success: true,
            data: Some(TokenResponse {
                access_token: token,
                token_type: "Bearer".to_string(),
                expires_in: jwt_config.expiration_hours * 3600,
            }),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(format!("Failed to generate token: {e:?}")),
        })),
    }
}

#[derive(Deserialize)]
struct SymbolQueryParams {
    name: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct MemoryQueryParams {
    kind: Option<String>,
    workstream: Option<String>,
}

#[derive(Deserialize)]
struct CreateMemoryBody {
    kind: String,
    title: String,
    body: String,
    workstream: Option<String>,
    scope: Option<String>,
    project: Option<String>,
}

#[derive(Deserialize)]
struct MemoryLifecycleBody {
    id: String,
    action: String,
}

#[derive(Deserialize)]
struct MaintenanceBody {
    action: String,
}

#[derive(Deserialize)]
struct AddPathBody {
    path: String,
}

// ---------------------------------------------------------------------------
// Dashboard helpers (git, projects, processes)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn git_command() -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = std::process::Command::new("git");
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(not(windows))]
fn git_command() -> std::process::Command {
    std::process::Command::new("git")
}

fn run_git(path: &str, args: &[&str]) -> Option<String> {
    let out = git_command().args(["-C", path]).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_branch_and_dirty(path: &str) -> (String, i64) {
    let branch = run_git(path, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let dirty = run_git(path, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count() as i64)
        .unwrap_or(0);
    (branch, dirty)
}

fn process_snapshot() -> sysinfo::System {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys
}

/// Is a background `aikd watch` process running?
fn watcher_running(sys: &sysinfo::System) -> bool {
    sys.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_lowercase();
        if !name.contains("aikd") {
            return false;
        }
        p.cmd()
            .iter()
            .any(|a| a.to_string_lossy().eq_ignore_ascii_case("watch"))
    })
}

const AGENT_EXES: [&str; 12] = [
    "opencode",
    "claude",
    "codex",
    "gemini",
    "gemini-cli",
    "antigravity",
    "cursor",
    "windsurf",
    "qwen",
    "code",
    "hermes",
    "pi",
];

/// Detect local coding agents running on this machine (read-only).
fn detect_local_agents(sys: &sysinfo::System) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for p in sys.processes().values() {
        let name = p.name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        let stem = lower.trim_end_matches(".exe");
        if !AGENT_EXES
            .iter()
            .any(|a| stem == *a || stem.starts_with(&format!("{a}-")))
        {
            continue;
        }
        out.push(serde_json::json!({
            "name": stem,
            "exe": name,
            "pid": p.pid().as_u32(),
            "cwd": p.cwd().map(|c| aikd_core::platform::canonical_path(c)).unwrap_or_default(),
            "connected_to_aikd": false,
        }));
    }
    out.sort_by_key(|v| v["name"].as_str().unwrap_or("").to_string());
    out.dedup_by_key(|v| {
        (
            v["name"].as_str().unwrap_or("").to_string(),
            v["pid"].as_i64().unwrap_or(0),
        )
    });
    out
}

/// (files, symbols, notes, atlas_status) for one project root.
fn project_stats(conn: &rusqlite::Connection, path: &str) -> (i64, i64, i64, String) {
    let norm = aikd_core::platform::normalize_project_key(path);
    let escaped = norm.replace('%', "\\%").replace('_', "\\_");
    let like = format!("{escaped}/%");
    let files: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM files WHERE status='active' AND (replace(path,'\\','/') = ?1 OR replace(path,'\\','/') LIKE ?2 ESCAPE '\\')",
            rusqlite::params![norm, like],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let symbols: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files fl ON fl.id=c.file_id WHERE fl.status='active' AND (replace(fl.path,'\\','/') = ?1 OR replace(fl.path,'\\','/') LIKE ?2 ESCAPE '\\')",
            rusqlite::params![norm, like],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let notes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notes WHERE project=?1",
            rusqlite::params![norm],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let status: String = conn
        .query_row(
            "SELECT status FROM atlas WHERE project_path=?1",
            rusqlite::params![norm],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "stale".to_string());
    (files, symbols, notes, status)
}

fn project_roots(conn: &rusqlite::Connection, cfg: &Config) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |p: String| {
        let p = aikd_core::platform::normalize_project_key(&p);
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    };
    for p in &cfg.scan.include_paths {
        push(p.clone());
    }
    for sql in [
        "SELECT project_path FROM atlas LIMIT 20",
        "SELECT DISTINCT project_path FROM memory WHERE project_path != '' LIMIT 20",
        "SELECT DISTINCT project FROM notes WHERE project != '' LIMIT 20",
    ] {
        if let Ok(mut stmt) = conn.prepare(sql) {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for p in rows.flatten() {
                    push(p);
                }
            }
        }
    }
    out.truncate(12);
    out
}

async fn handle_overview(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    let conn = database.conn();
    let qi = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
    let files = qi("SELECT COUNT(*) FROM files");
    let chunks = qi("SELECT COUNT(*) FROM chunks");
    let symbols = qi("SELECT COUNT(*) FROM code_symbols");
    let mem_active = qi("SELECT COUNT(*) FROM memory WHERE status='active'");
    let mem_stale = qi("SELECT COUNT(*) FROM memory WHERE status='stale'");
    let mem_arch = qi("SELECT COUNT(*) FROM memory WHERE status='archived'");
    let mem_conf = qi("SELECT COUNT(*) FROM memory WHERE status='conflicted'");
    let events = qi("SELECT COUNT(*) FROM events");
    let sessions = qi("SELECT COUNT(*) FROM sessions");
    let notes_count = aikd_storage::notes::notes_count(conn).unwrap_or(0);
    let active_clients_count = aikd_storage::telemetry::active_client_count(conn).unwrap_or(0);
    let (tokens_saved_est, _, avg_query_ms) =
        aikd_storage::telemetry::token_savings(conn).unwrap_or((0, 0, 0.0));
    let schema_v: i64 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap_or(15);
    let db_mb = std::fs::metadata(cfg.db_path())
        .map(|m| m.len() as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0);
    let profile = aikd_core::ResourceProfile::detect_with_mode(&cfg.resource.mode);
    let cover: Vec<serde_json::Value> = aikd_storage::events::all_adapter_coverage(conn)
        .unwrap_or_default()
        .into_iter()
        .take(25)
        .map(|(agent, event_type, count)| {
            serde_json::json!({ "agent": agent, "event_type": event_type, "count": count })
        })
        .collect();

    let mut projects: Vec<serde_json::Value> = Vec::new();
    for root in project_roots(conn, &cfg) {
        let (files_n, symbols_n, notes_n, atlas_status) = project_stats(conn, &root);
        let (branch, dirty) = git_branch_and_dirty(&root);
        projects.push(serde_json::json!({
            "path": root,
            "name": root.rsplit('/').next().unwrap_or(&root),
            "files": files_n,
            "symbols": symbols_n,
            "notes": notes_n,
            "git_branch": branch,
            "git_dirty": dirty,
            "stale": atlas_status != "fresh",
        }));
    }

    let sys = process_snapshot();
    let watcher = watcher_running(&sys);

    Ok(Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "schema_version": schema_v,
            "db_mb": db_mb,
            "files": files,
            "chunks": chunks,
            "symbols": symbols,
            "memory_active": mem_active,
            "memory_stale": mem_stale,
            "memory_archived": mem_arch,
            "memory_conflicted": mem_conf,
            "events": events,
            "sessions": sessions,
            "cpu_cores": profile.cpu_cores,
            "ram_gb": profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            "coverage": cover,
            "notes_count": notes_count,
            "active_clients_count": active_clients_count,
            "tokens_saved_est": tokens_saved_est,
            "avg_query_ms": avg_query_ms,
            "watcher_running": watcher,
            "projects": projects,
        })),
        error: None,
    }))
}

async fn handle_symbols_api(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<SymbolQueryParams>,
) -> Result<Json<SymbolApiResponse>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let start = std::time::Instant::now();
    let database = state.database.lock().await;
    let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
    match aikd_storage::symbols::find_symbols(
        database.conn(),
        &params.name,
        params.limit.unwrap_or(25),
    ) {
        Ok(list) => {
            let returned_bytes: usize = list.iter().map(|s| s.signature.len() + s.name.len()).sum();
            let paths: Vec<String> = list.iter().map(|s| s.file_path.clone()).collect();
            record_query_metric(
                database.conn(),
                "symbol",
                &params.name,
                &paths,
                returned_bytes,
                start.elapsed(),
            );
            let attached_constraints = constraints_for(database.conn(), &project);
            Ok(Json(SymbolApiResponse {
                success: true,
                data: Some(list),
                error: None,
                attached_constraints,
            }))
        }
        Err(e) => Ok(Json(SymbolApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
            attached_constraints: Vec::new(),
        })),
    }
}

async fn handle_memory_list_api(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<MemoryQueryParams>,
) -> Result<Json<ApiResponse<Vec<aikd_core::MemoryItem>>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    let conn = database.conn();
    let mut sql = "SELECT id, project_path, workstream, kind, title, body, source, status, confidence, supersedes, conflict_note, created_at, updated_at FROM memory WHERE 1=1".to_string();
    let mut bind_vals: Vec<String> = Vec::new();
    if let Some(k) = params.kind.filter(|s| !s.is_empty()) {
        bind_vals.push(k);
        sql.push_str(&format!(" AND kind=?{}", bind_vals.len()));
    }
    if let Some(ws) = params.workstream.filter(|s| !s.is_empty()) {
        bind_vals.push(ws);
        sql.push_str(&format!(" AND workstream=?{}", bind_vals.len()));
    }
    sql.push_str(" ORDER BY updated_at DESC LIMIT 100");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let refs: Vec<&dyn rusqlite::ToSql> = bind_vals
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt
        .query_map(&refs[..], |row| {
            Ok(aikd_core::MemoryItem {
                id: row.get(0)?,
                project_path: row.get(1)?,
                workstream: row.get(2)?,
                kind: row.get(3)?,
                title: row.get(4)?,
                body: row.get(5)?,
                source: row.get(6)?,
                status: row.get(7)?,
                confidence: row.get(8)?,
                supersedes: row.get(9)?,
                conflict_note: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut list = Vec::new();
    for r in rows.flatten() {
        list.push(r);
    }
    Ok(Json(ApiResponse {
        success: true,
        data: Some(list),
        error: None,
    }))
}

async fn handle_memory_create_api(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<CreateMemoryBody>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    // Secret Redaction Shield also applies to memory writes.
    let title_red = aikd_core::security::redact_secrets(&body.title);
    let body_red = aikd_core::security::redact_secrets(&body.body);
    let redacted_count = title_red.redacted_count + body_red.redacted_count;
    let global = body.scope.as_deref() == Some("global");
    let project = match body.project {
        Some(p) if !p.trim().is_empty() => aikd_core::platform::normalize_project_key(&p),
        _ => {
            if global {
                String::new()
            } else {
                aikd_core::platform::canonical_project(&cfg.scan.include_paths)
            }
        }
    };
    let ws = body.workstream.unwrap_or_else(|| "default".to_string());
    match aikd_storage::memory::save_memory(
        database.conn(),
        &project,
        &ws,
        &body.kind,
        &title_red.text,
        &body_red.text,
        "explicit",
        1.0,
    ) {
        Ok(item) => {
            let mut value = serde_json::to_value(&item).unwrap_or_default();
            if let Some(obj) = value.as_object_mut() {
                obj.insert("redacted_count".into(), serde_json::json!(redacted_count));
                obj.insert(
                    "redacted_kinds".into(),
                    serde_json::json!(title_red
                        .kinds
                        .iter()
                        .chain(body_red.kinds.iter())
                        .cloned()
                        .collect::<Vec<_>>()),
                );
            }
            Ok(Json(ApiResponse {
                success: true,
                data: Some(value),
                error: None,
            }))
        }
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

async fn handle_memory_lifecycle_api(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<MemoryLifecycleBody>,
) -> Result<Json<ApiResponse<String>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    let res = match body.action.as_str() {
        "stale" => aikd_storage::memory::mark_stale(database.conn(), &body.id),
        "archive" => aikd_storage::memory::archive(database.conn(), &body.id),
        other => Err(anyhow::anyhow!("Unknown action: {other}")),
    };
    match res {
        Ok(n) => Ok(Json(ApiResponse {
            success: true,
            data: Some(format!("Updated {n} item(s)")),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

async fn handle_handoff_api(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<String>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
    match aikd_context::assemble_handoff(database.conn(), &project, "default", 15) {
        Ok(pkg) => Ok(Json(ApiResponse {
            success: true,
            data: Some(pkg.render_text()),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

async fn handle_config_get_api(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Config>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    Ok(Json(ApiResponse {
        success: true,
        data: Some(cfg),
        error: None,
    }))
}

async fn handle_config_save_api(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(new_cfg): Json<Config>,
) -> Result<Json<ApiResponse<String>>, ApiError> {
    {
        let cfg = state.config.lock().await;
        if !check_auth(&headers, &cfg.server.auth_token) {
            return Err(unauthorized());
        }
    }
    if let Err(e) = new_cfg.save(&state.config_path) {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(format!("Failed to write config.yaml: {e}")),
        }));
    }
    let mut cfg_guard = state.config.lock().await;
    *cfg_guard = new_cfg;
    state.query_cache.invalidate_all();
    Ok(Json(ApiResponse {
        success: true,
        data: Some("Saved to config.yaml".to_string()),
        error: None,
    }))
}

async fn handle_config_add_path_api(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<AddPathBody>,
) -> Result<Json<ApiResponse<String>>, ApiError> {
    let mut cfg = state.config.lock().await;
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let p = body.path.trim().to_string();
    if !p.is_empty() && !cfg.scan.include_paths.contains(&p) {
        cfg.scan.include_paths.insert(0, p.clone());
        let _ = cfg.save(&state.config_path);
    }
    Ok(Json(ApiResponse {
        success: true,
        data: Some(format!("Added {p} to include_paths")),
        error: None,
    }))
}

async fn handle_maintenance_api(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<MaintenanceBody>,
) -> Result<Json<ApiResponse<String>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    let conn = database.conn();
    let msg = match body.action.as_str() {
        "gc" => match aikd_storage::maintenance::gc_with_config(conn, true, Some(&cfg)) {
            Ok((files, chunks)) => {
                format!("GC completed: removed {files} missing/excluded files and {chunks} orphan chunks, then VACUUMed.")
            }
            Err(e) => {
                return Ok(Json(ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                }))
            }
        },
        "rebuild_fts" => match aikd_storage::maintenance::db_compact(conn, true) {
            Ok(s) => format!("FTS5 Rebuild & Compact: {s}"),
            Err(e) => {
                return Ok(Json(ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                }))
            }
        },
        "prune_content" => match aikd_storage::fts::prune_chunk_content(conn) {
            Ok(n) => {
                let _ = conn.execute_batch("VACUUM");
                format!("Pruned legacy chunk content from {n} rows and VACUUMed database.")
            }
            Err(e) => {
                return Ok(Json(ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                }))
            }
        },
        "sync_rules" => {
            let purged = aikd_storage::memory::purge_dummy_memories(conn).unwrap_or(0);
            let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
            match aikd_storage::memory::sync_rules_and_skills_from_files(conn, &project) {
                Ok((created, superseded, unchanged)) => {
                    format!("Synced Rules & Skills (.md): +{created} new, {superseded} updated (superseded), {unchanged} unchanged (purged {purged} dummy test rows).")
                }
                Err(e) => {
                    return Ok(Json(ApiResponse {
                        success: false,
                        data: None,
                        error: Some(e.to_string()),
                    }))
                }
            }
        }
        other => {
            return Ok(Json(ApiResponse {
                success: false,
                data: None,
                error: Some(format!("Unknown maintenance action: {other}")),
            }))
        }
    };
    state.query_cache.invalidate_all();
    Ok(Json(ApiResponse {
        success: true,
        data: Some(msg),
        error: None,
    }))
}

async fn handle_agents_list_api(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<aikd_core::agents::AgentStatus>>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    Ok(Json(ApiResponse {
        success: true,
        data: Some(aikd_core::agents::detect_agent_statuses()),
        error: None,
    }))
}

async fn handle_agents_register_api(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<aikd_core::agents::AgentStatus>>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let bin = if cfg!(windows) {
        shellexpand::tilde("~/.local/bin/aikd.exe").to_string()
    } else {
        shellexpand::tilde("~/.local/bin/aikd").to_string()
    };
    let exe = if std::path::Path::new(&bin).is_file() {
        bin
    } else {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "aikd".to_string())
    };
    let _ = aikd_core::agents::detect_and_register(&exe);
    Ok(Json(ApiResponse {
        success: true,
        data: Some(aikd_core::agents::detect_agent_statuses()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Human notes, "Send to AIKD", brief, export
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct NoteBody {
    title: Option<String>,
    #[serde(default)]
    body: String,
    kind: Option<String>,
    scope: Option<String>,
    folder: Option<String>,
    project: Option<String>,
    tags: Option<Vec<String>>,
    source: Option<String>,
    source_url: Option<String>,
    pinned: Option<bool>,
}

#[derive(Deserialize, Default)]
struct NoteUpdateBody {
    title: Option<String>,
    body: Option<String>,
    kind: Option<String>,
    scope: Option<String>,
    folder: Option<String>,
    project: Option<String>,
    tags: Option<Vec<String>>,
    source: Option<String>,
    source_url: Option<String>,
    pinned: Option<bool>,
}

#[derive(Deserialize)]
struct NotesQuery {
    q: Option<String>,
    project: Option<String>,
    scope: Option<String>,
    folder: Option<String>,
    kind: Option<String>,
    tag: Option<String>,
    pinned: Option<bool>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct BriefParams {
    days: Option<i64>,
    project: Option<String>,
}

#[derive(Deserialize)]
struct ExportParams {
    #[serde(rename = "format")]
    format: Option<String>,
}

fn notes_dir(state: &RestState, cfg: &Config) -> std::path::PathBuf {
    cfg.runtime_dir(&state.config_path).join("notes")
}

async fn create_note(
    state: &RestState,
    headers: &HeaderMap,
    body: NoteBody,
    default_source: &str,
) -> Result<Json<ApiResponse<aikd_storage::notes::NoteRecord>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let global = body.scope.as_deref() == Some("global");
    let project = match body.project.as_deref() {
        Some(p) if !p.trim().is_empty() => p.to_string(),
        _ => {
            if global {
                String::new()
            } else {
                aikd_core::platform::canonical_project(&cfg.scan.include_paths)
            }
        }
    };
    let input = aikd_storage::notes::NewNote {
        title: body.title,
        body: body.body,
        kind: body.kind,
        scope: body.scope,
        folder: body.folder,
        project: Some(project),
        tags: body.tags,
        source: Some(body.source.unwrap_or_else(|| default_source.to_string())),
        source_url: body.source_url,
        pinned: body.pinned,
    };
    let dir = notes_dir(state, &cfg);
    let database = state.database.lock().await;
    match aikd_storage::notes::create_note(database.conn(), &dir, input) {
        Ok(note) => Ok(Json(ApiResponse {
            success: true,
            data: Some(note),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

/// `POST /api/send` — quick capture (tray, clipboard, Chrome extension).
async fn handle_send(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<NoteBody>,
) -> Result<Json<ApiResponse<aikd_storage::notes::NoteRecord>>, ApiError> {
    create_note(&state, &headers, body, "ui").await
}

/// `POST /api/notes` — human vault create.
async fn handle_notes_create(
    State(state): State<RestState>,
    headers: HeaderMap,
    Json(body): Json<NoteBody>,
) -> Result<Json<ApiResponse<aikd_storage::notes::NoteRecord>>, ApiError> {
    create_note(&state, &headers, body, "human").await
}

/// `GET /api/notes` — notes + tag/folder facets.
async fn handle_notes_list(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<NotesQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let filter = aikd_storage::notes::NoteFilter {
        q: params.q,
        project: params.project,
        scope: params.scope,
        folder: params.folder,
        kind: params.kind,
        tag: params.tag,
        pinned: params.pinned,
        limit: params.limit,
    };
    let database = state.database.lock().await;
    match aikd_storage::notes::list_notes(database.conn(), &filter) {
        Ok((notes, tags, folders)) => Ok(Json(ApiResponse {
            success: true,
            data: Some(serde_json::json!({
                "notes": notes,
                "tags": tags,
                "folders": folders,
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

/// `PUT /api/notes/{id}` — update + re-redact + rewrite Markdown.
async fn handle_note_update(
    State(state): State<RestState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<NoteUpdateBody>,
) -> Result<Json<ApiResponse<aikd_storage::notes::NoteRecord>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let patch = aikd_storage::notes::NotePatch {
        title: body.title,
        body: body.body,
        kind: body.kind,
        scope: body.scope,
        folder: body.folder,
        project: body.project,
        tags: body.tags,
        source: body.source,
        source_url: body.source_url,
        pinned: body.pinned,
    };
    let dir = notes_dir(&state, &cfg);
    let database = state.database.lock().await;
    match aikd_storage::notes::update_note(database.conn(), &dir, &id, patch) {
        Ok(Some(note)) => Ok(Json(ApiResponse {
            success: true,
            data: Some(note),
            error: None,
        })),
        Ok(None) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(format!("Note '{id}' not found")),
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

/// `DELETE /api/notes/{id}` — remove row, edges and Markdown file.
async fn handle_note_delete(
    State(state): State<RestState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<String>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    match aikd_storage::notes::delete_note(database.conn(), &id) {
        Ok(true) => Ok(Json(ApiResponse {
            success: true,
            data: Some(format!("Deleted note {id}")),
            error: None,
        })),
        Ok(false) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(format!("Note '{id}' not found")),
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

/// `GET /api/brief?days=3&project=...` — Morning Brief / check-before-start.
async fn handle_brief(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<BriefParams>,
) -> Result<Json<ApiResponse<aikd_storage::notes::MorningBrief>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let project = params
        .project
        .unwrap_or_else(|| aikd_core::platform::canonical_project(&cfg.scan.include_paths));
    let database = state.database.lock().await;
    match aikd_storage::notes::morning_brief(database.conn(), params.days.unwrap_or(3), &project) {
        Ok(brief) => Ok(Json(ApiResponse {
            success: true,
            data: Some(brief),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

/// `GET /api/export` — 1-click backup (JSON bundle or Markdown archive).
async fn handle_export(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<ExportParams>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    let want_markdown = params
        .format
        .as_deref()
        .map(|f| f.eq_ignore_ascii_case("md") || f.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false);
    if want_markdown {
        match aikd_storage::notes::export_markdown(database.conn()) {
            Ok(md) => Ok(Json(ApiResponse {
                success: true,
                data: Some(serde_json::json!({
                    "format": "markdown",
                    "filename": format!("aikd-backup-{}.md", chrono::Utc::now().format("%Y%m%d")),
                    "content": md,
                })),
                error: None,
            })),
            Err(e) => Ok(Json(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
            })),
        }
    } else {
        match aikd_storage::notes::export_bundle(database.conn()) {
            Ok(bundle) => Ok(Json(ApiResponse {
                success: true,
                data: Some(bundle),
                error: None,
            })),
            Err(e) => Ok(Json(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// Live Mission Control
// ---------------------------------------------------------------------------

static DAEMON_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// `GET /api/live` — who is connected, where, and doing what.
async fn handle_live(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let started = DAEMON_START.get_or_init(std::time::Instant::now);
    let database = state.database.lock().await;
    let conn = database.conn();

    let sys = process_snapshot();

    // Reconcile rows whose process died without a clean EOF.
    if let Ok(clients) = aikd_storage::telemetry::list_active_clients(conn) {
        for c in &clients {
            if c.pid > 0 {
                let alive = sys.process(sysinfo::Pid::from_u32(c.pid as u32)).is_some();
                if !alive {
                    let _ = aikd_storage::telemetry::disconnect_client(conn, &c.client_id);
                }
            }
        }
    }
    let clients = aikd_storage::telemetry::list_active_clients(conn).unwrap_or_default();

    let mut agents = detect_local_agents(&sys);
    let client_pids: HashSet<i64> = clients.iter().map(|c| c.pid).collect();
    let parent_pids: HashSet<i64> = clients.iter().map(|c| c.parent_pid).collect();
    for a in agents.iter_mut() {
        let pid = a["pid"].as_i64().unwrap_or(0);
        let connected = client_pids.contains(&pid) || parent_pids.contains(&pid);
        a["connected_to_aikd"] = serde_json::json!(connected);
    }

    let events = aikd_storage::events::recent_events(conn, 20)
        .unwrap_or_default()
        .into_iter()
        .map(|e| {
            let summary = serde_json::from_str::<serde_json::Value>(&e.payload)
                .ok()
                .and_then(|v| {
                    v.get("summary")
                        .or_else(|| v.get("query"))
                        .or_else(|| v.get("tool"))
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| e.event_type.clone());
            serde_json::json!({
                "id": e.event_id,
                "agent": e.agent_id,
                "event_type": e.event_type,
                "summary": format!("{}: {}", e.event_type, summary),
                "project": e.project_id,
                "created_at": e.timestamp,
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "daemon": {
                "pid": std::process::id(),
                "port": cfg.server.rest_port,
                "uptime_secs": started.elapsed().as_secs(),
                "db_path": aikd_core::platform::canonical_path(&cfg.db_path()),
                "status": "healthy",
            },
            "active_clients": clients,
            "detected_agents": agents,
            "herdr_terminals": aikd_storage::telemetry::herdr_terminals(),
            "recent_events": events,
        })),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Analytics
// ---------------------------------------------------------------------------

/// `GET /api/analytics` — languages, memory mix, activity timeline, tokens.
async fn handle_analytics(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let database = state.database.lock().await;
    let conn = database.conn();

    // Language distribution.
    let mut lang: HashMap<String, (i64, i64)> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT f.path, (SELECT COUNT(*) FROM chunks c WHERE c.file_id=f.id) FROM files f WHERE f.status='active'",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        }) {
            for (path, chunks) in rows.flatten() {
                let name = path.rsplit('/').next().unwrap_or(&path);
                let ext = match name.rfind('.') {
                    Some(i) => name[i..].to_lowercase(),
                    None => "(none)".to_string(),
                };
                let entry = lang.entry(ext).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += chunks;
            }
        }
    }
    let mut languages: Vec<serde_json::Value> = lang
        .into_iter()
        .map(|(ext, (files, chunks))| {
            serde_json::json!({ "ext": ext, "files": files, "chunks": chunks })
        })
        .collect();
    languages.sort_by(|a, b| {
        b["files"]
            .as_i64()
            .cmp(&a["files"].as_i64())
            .then_with(|| a["ext"].as_str().cmp(&b["ext"].as_str()))
    });
    languages.truncate(20);

    // Memory breakdown (kinds + lifecycle).
    let mut breakdown = serde_json::Map::new();
    for kind in [
        "note",
        "decision",
        "constraint",
        "fact",
        "discovery",
        "task",
        "prompt",
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory WHERE kind=?1 AND status='active'",
                rusqlite::params![kind],
                |r| r.get(0),
            )
            .unwrap_or(0);
        breakdown.insert(kind.to_string(), serde_json::json!(n));
    }
    let note_kind: i64 = conn
        .query_row("SELECT COUNT(*) FROM notes WHERE kind='note'", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    breakdown.insert("note".to_string(), serde_json::json!(note_kind));
    for status in ["active", "stale", "conflicted", "archived"] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory WHERE status=?1",
                rusqlite::params![status],
                |r| r.get(0),
            )
            .unwrap_or(0);
        breakdown.insert(status.to_string(), serde_json::json!(n));
    }

    // Activity timeline: last 24 hours bucketed by hour + agent.
    let mut buckets: Vec<serde_json::Value> = Vec::new();
    let mut hour_agents: HashMap<String, HashMap<String, i64>> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT substr(timestamp,12,2), agent_id, COUNT(*) FROM events WHERE datetime(timestamp) >= datetime('now','-24 hours') GROUP BY 1,2 ORDER BY 1",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        }) {
            for (hh, agent, count) in rows.flatten() {
                hour_agents
                    .entry(hh)
                    .or_default()
                    .insert(agent, count);
            }
        }
    }
    for h in 0..24 {
        let key = format!("{h:02}");
        let agents = hour_agents.remove(&key).unwrap_or_default();
        let count: i64 = agents.values().sum();
        buckets.push(serde_json::json!({
            "bucket": format!("{key}:00"),
            "count": count,
            "agents": agents,
        }));
    }

    let (saved, total, avg) = aikd_storage::telemetry::token_savings(conn).unwrap_or((0, 0, 0.0));

    Ok(Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "languages": languages,
            "memory_breakdown": breakdown,
            "activity_timeline": buckets,
            "tokens": {
                "saved_est": saved,
                "queries_total": total,
                "avg_elapsed_ms": avg,
            },
        })),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Impact analysis + context pack
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ImpactParams {
    target: String,
    project: Option<String>,
}

/// `GET /api/impact?target=<symbol_or_file>` — blast radius before refactor.
async fn handle_impact(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<ImpactParams>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let project = params
        .project
        .unwrap_or_else(|| aikd_core::platform::canonical_project(&cfg.scan.include_paths));
    let database = state.database.lock().await;
    match aikd_storage::graph::impact(database.conn(), &params.target, &project) {
        Ok(value) => Ok(Json(ApiResponse {
            success: true,
            data: Some(value),
            error: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        })),
    }
}

#[derive(Deserialize)]
struct ContextPackParams {
    project: Option<String>,
    node_id: Option<String>,
}

/// `GET /api/context-pack` — dense Markdown ready to paste into a web chat.
async fn handle_context_pack(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<ContextPackParams>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(unauthorized());
    }
    let project = params
        .project
        .clone()
        .map(|p| aikd_core::platform::normalize_project_key(&p))
        .unwrap_or_else(|| aikd_core::platform::canonical_project(&cfg.scan.include_paths));
    let database = state.database.lock().await;
    let conn = database.conn();

    let mut items = 0usize;
    let mut md = String::new();
    md.push_str("# AIKD Context Pack\n\n");
    md.push_str(&format!(
        "Project: `{}`\nGenerated: {}\n\n",
        if project.is_empty() {
            "(global)"
        } else {
            &project
        },
        chrono::Utc::now().to_rfc3339()
    ));

    if let Some(node_id) = params.node_id.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(Some(detail)) = aikd_storage::graph::node_detail(conn, node_id) {
            md.push_str(&format!("## Focus: {}\n\n", detail.label));
            md.push_str(&format!(
                "_kind: {} · scope: {}_\n\n",
                detail.kind, detail.scope
            ));
            if !detail.preview.trim().is_empty() {
                md.push_str(&detail.preview);
                md.push_str("\n\n");
            }
            if !detail.outgoing.is_empty() {
                md.push_str("### Links\n");
                for l in detail.outgoing.iter().take(15) {
                    md.push_str(&format!("- {} ({})\n", l.label, l.edge_kind));
                }
                md.push('\n');
            }
            items += 1;
        }
    }

    let symbols = aikd_storage::symbols::find_symbols(conn, "", 15).unwrap_or_default();
    if !symbols.is_empty() {
        md.push_str("## Symbols\n");
        for s in symbols.iter().take(15) {
            md.push_str(&format!(
                "- `{}` ({}) — {}:{}\n",
                s.name, s.kind, s.file_path, s.line_start
            ));
        }
        md.push('\n');
        items += symbols.len();
    }

    let constraints = constraints_for(conn, &project);
    if !constraints.is_empty() {
        md.push_str("## Active Constraints\n");
        for c in &constraints {
            md.push_str(&format!(
                "- [{}] **{}** — {}\n",
                c.scope,
                c.title,
                c.body.lines().next().unwrap_or("")
            ));
        }
        md.push('\n');
        items += constraints.len();
    }

    if let Ok((notes, _, _)) = aikd_storage::notes::list_notes(
        conn,
        &aikd_storage::notes::NoteFilter {
            project: Some(project.clone()).filter(|p| !p.is_empty()),
            limit: Some(8),
            ..Default::default()
        },
    ) {
        if !notes.is_empty() {
            md.push_str("## Human Notes\n");
            for n in &notes {
                md.push_str(&format!(
                    "- **{}** ({}) — {}\n",
                    n.title,
                    n.kind,
                    n.body
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("")
                        .trim()
                ));
            }
            md.push('\n');
            items += notes.len();
        }
    }

    let tokens_est = aikd_core::estimate_tokens(&md);
    Ok(Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "markdown": md,
            "tokens_est": tokens_est,
            "items_count": items,
        })),
        error: None,
    }))
}

pub async fn run_rest_server(config_path: &str, port: u16) -> anyhow::Result<()> {
    let cfg = Config::load_validated(config_path);
    let cors_origins = cfg.server.cors_origins.clone();

    // Use connection pooling for better concurrent performance
    let (database, _pool) = Database::open_pooled(&cfg.db_path(), 10)?;
    let database = Arc::new(Mutex::new(database));
    let tantivy = Arc::new(TantivyEngine::open(&cfg.tantivy_path())?);

    log::info!("Database connection pool initialized (max: 10)");

    // Initialize query cache: 1000 entries, 5 minute TTL
    let query_cache = Arc::new(
        moka::future::Cache::builder()
            .max_capacity(1000)
            .time_to_live(std::time::Duration::from_secs(300))
            .build(),
    );

    // Initialize Prometheus metrics
    let metrics = Arc::new(Metrics::new());
    log::info!("Prometheus metrics initialized");

    let state = RestState {
        config: Arc::new(Mutex::new(cfg)),
        config_path: config_path.to_string(),
        database,
        tantivy,
        query_cache,
        metrics,
    };

    // Configure CORS. Default (empty `cors_origins`) is localhost + browser
    // extension origins only: the Chrome/Edge MV3 extension talks to both
    // :9090 and sandbox :9091 without opening CORS to arbitrary websites.
    let allow_methods = [
        axum::http::Method::GET,
        axum::http::Method::POST,
        axum::http::Method::PUT,
        axum::http::Method::DELETE,
        axum::http::Method::OPTIONS,
    ];
    let localhost_predicate = tower_http::cors::AllowOrigin::predicate(
        |origin: &axum::http::HeaderValue, _parts: &axum::http::request::Parts| {
            let Ok(o) = origin.to_str() else {
                return false;
            };
            let o = o.trim_end_matches('/').to_ascii_lowercase();
            o == "http://localhost"
                || o == "http://127.0.0.1"
                || o.starts_with("http://localhost:")
                || o.starts_with("http://127.0.0.1:")
                || o.starts_with("https://localhost:")
                || o.starts_with("https://127.0.0.1:")
                || o.starts_with("chrome-extension://")
                || o.starts_with("extension://")
                || o.starts_with("moz-extension://")
                || o.starts_with("safari-web-extension://")
        },
    );
    let cors = if cors_origins.contains(&"*".to_string()) {
        log::warn!("CORS: Allowing all origins ('*'). Set specific origins in server.cors_origins for production.");
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(allow_methods.clone())
            .allow_headers(tower_http::cors::Any)
    } else if cors_origins.is_empty() {
        CorsLayer::new()
            .allow_origin(localhost_predicate)
            .allow_methods(allow_methods.clone())
            .allow_headers(tower_http::cors::Any)
    } else {
        // Use configured origins (plus the always-safe local/extension set).
        let origins: Vec<axum::http::HeaderValue> =
            cors_origins.iter().filter_map(|o| o.parse().ok()).collect();
        if origins.is_empty() {
            log::warn!("CORS: No valid origins in config, falling back to localhost + extensions");
            CorsLayer::new()
                .allow_origin(localhost_predicate)
                .allow_methods(allow_methods.clone())
                .allow_headers(tower_http::cors::Any)
        } else {
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(allow_methods.clone())
                .allow_headers(tower_http::cors::Any)
        }
    };

    // Configure rate limiting: ~30 requests per second per IP, burst 100
    // so the interactive `/ui` dashboard can fetch multiple panels concurrently.
    // NOTE: tower_governor's `per_second(n)` means ONE token every n SECONDS,
    // not n tokens per second — use `per_millisecond(33)` for ~30 rps.
    let governor_conf = tower_governor::governor::GovernorConfigBuilder::default()
        .per_millisecond(33)
        .burst_size(100)
        .key_extractor(tower_governor::key_extractor::PeerIpKeyExtractor)
        .finish()
        .expect("Failed to create rate limiter config");

    let rate_limiter = tower_governor::GovernorLayer {
        config: std::sync::Arc::new(governor_conf),
    };

    /// Local dashboard (`/ui` and `/`): serves the embedded AIKD Studio SPA.
    async fn handle_ui(State(state): State<RestState>, headers: HeaderMap) -> impl IntoResponse {
        let cfg = state.config.lock().await.clone();
        if !check_auth(&headers, &cfg.server.auth_token) {
            return unauthorized().into_response();
        }
        const UI_HTML: &str = include_str!("ui.html");
        (
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            UI_HTML,
        )
            .into_response()
    }

    /// Modular knowledge-graph views (10-mode upgrade), served as one embedded
    /// bundle so AIKD Studio stays 100% offline with zero external requests.
    async fn handle_graph_modules(
        State(state): State<RestState>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        let cfg = state.config.lock().await.clone();
        if !check_auth(&headers, &cfg.server.auth_token) {
            return unauthorized().into_response();
        }
        // Concatenated in load order: shared adapter -> switcher -> view modules.
        // `include_str!` keeps every byte inside the compiled binary.
        const GRAPH_MODULES: &str = concat!(
            include_str!("ui/adapters/graphViewAdapter.js"),
            "\n;\n",
            include_str!("ui/components/ViewModeSwitcher.js"),
            "\n;\n",
            include_str!("ui/views/SpatialGraph3DView.js"),
            "\n;\n",
            include_str!("ui/views/CityTreemapView.js"),
            "\n;\n",
            include_str!("ui/views/CirclePackingView.js"),
            "\n;\n",
            include_str!("ui/views/SunburstView.js"),
            "\n;\n",
            include_str!("ui/views/SemanticIslandsView.js"),
            "\n;\n",
            include_str!("ui/views/MillerColumnsView.js"),
            "\n;\n",
            include_str!("ui/views/SwimlaneTimelineView.js"),
            "\n",
        );
        (
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "application/javascript; charset=utf-8",
                ),
                (axum::http::header::CACHE_CONTROL, "no-cache"),
            ],
            GRAPH_MODULES,
        )
            .into_response()
    }

    let app = Router::new()
        .route("/", get(handle_ui))
        .route("/ui", get(handle_ui))
        .route("/ui/graph-modules.js", get(handle_graph_modules))
        .route("/api/overview", get(handle_overview))
        .route("/api/health", get(handle_health))
        .route("/api/metrics", get(handle_metrics))
        .route("/metrics/prometheus", get(handle_metrics))
        .route("/api/auth/login", post(handle_login))
        .route("/api/query", get(handle_query))
        .route("/api/symbols", get(handle_symbols_api))
        .route("/api/stats", get(handle_stats))
        .route("/api/status", get(handle_status))
        .route("/api/scan", post(handle_scan))
        .route("/api/maintenance", post(handle_maintenance_api))
        .route(
            "/api/memory",
            get(handle_memory_list_api).post(handle_memory_create_api),
        )
        .route("/api/memory/lifecycle", post(handle_memory_lifecycle_api))
        .route("/api/handoff", get(handle_handoff_api))
        .route(
            "/api/config",
            get(handle_config_get_api).post(handle_config_save_api),
        )
        .route("/api/config/add-path", post(handle_config_add_path_api))
        .route("/api/agents", get(handle_agents_list_api))
        .route("/api/agents/register", post(handle_agents_register_api))
        .route("/api/embed", post(handle_embed))
        .route("/api/remember", post(handle_remember))
        .route("/api/recall", post(handle_recall))
        .route("/api/sessions", get(handle_sessions))
        .route("/api/graph", get(handle_graph))
        .route("/api/graph/node", get(handle_graph_node))
        .route("/api/send", post(handle_send))
        .route(
            "/api/notes",
            get(handle_notes_list).post(handle_notes_create),
        )
        .route(
            "/api/notes/{id}",
            put(handle_note_update).delete(handle_note_delete),
        )
        .route("/api/brief", get(handle_brief))
        .route("/api/export", get(handle_export))
        .route("/api/live", get(handle_live))
        .route("/api/analytics", get(handle_analytics))
        .route("/api/impact", get(handle_impact))
        .route("/api/context-pack", get(handle_context_pack))
        .layer(rate_limiter)
        .layer(cors)
        .with_state(state);

    let addr = format!("127.0.0.1:{port}");
    eprintln!("AIKD REST API listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    log::info!("AIKD REST API listening on http://{addr}");

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

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal)
    .await?;
    Ok(())
}
