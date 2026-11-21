use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use prometheus::{Encoder, Gauge, IntCounter, IntGauge, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
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
        let message = code
            .canonical_reason()
            .unwrap_or("error")
            .to_string();
        Self { code, message }
    }
}

fn error_response<T: Serialize>(msg: &str, code: StatusCode) -> Result<Json<ApiResponse<T>>, ApiError> {
    Err(ApiError::new(code, msg))
}

/// Validate authentication token from request headers.
/// Returns true only if the provided Bearer token matches the expected token.
/// Rejects empty tokens and missing tokens when auth is configured.
/// Supports both simple token auth and JWT tokens.
fn check_auth(headers: &HeaderMap, token: &Option<String>) -> bool {
    // Fail-closed: without a configured token every protected endpoint
    // rejects the request. Health/login stay public by design.
    let expected = match token {
        Some(t) if !t.is_empty() => t,
        _ => {
            log::warn!("No auth_token configured — rejecting request. Set server.auth_token in config.yaml");
            return false;
        }
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

/// Handle search query with caching and metrics support.
/// Cache key includes all query parameters to ensure correct cache hits.
async fn handle_query(
    State(state): State<RestState>,
    headers: HeaderMap,
    Query(params): Query<QueryParams>,
) -> Result<Json<ApiResponse<Vec<aikd_core::SearchResult>>>, ApiError> {
    let start = std::time::Instant::now();
    state.metrics.query_total.inc();

    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return error_response("Unauthorized", StatusCode::UNAUTHORIZED);
    }

    let limit = params.limit.unwrap_or(10);
    let hybrid = params.hybrid.unwrap_or(false);

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
        return Ok(Json(ApiResponse {
            success: true,
            data: Some(cached),
            error: None,
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
    let outcome = aikd_context::run_query(
        &database,
        state.tantivy.clone(),
        &cfg.model_path(),
        cfg.embedding.enabled,
        &req,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let results = outcome.results;

    // Store in cache
    state.query_cache.insert(cache_key, results.clone()).await;
    state
        .metrics
        .query_duration_seconds
        .observe(start.elapsed().as_secs_f64());
    log::debug!("Cached query result: {}", params.q);

    Ok(Json(ApiResponse {
        success: true,
        data: Some(results),
        error: None,
    }))
}

async fn handle_stats(
    State(state): State<RestState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(ApiError::from(StatusCode::UNAUTHORIZED));
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
        return Err(ApiError::from(StatusCode::UNAUTHORIZED));
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
) -> Result<Json<ApiResponse<String>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(ApiError::from(StatusCode::UNAUTHORIZED));
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
) -> Result<Json<ApiResponse<Vec<aikd_core::Conversation>>>, ApiError> {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return Err(ApiError::from(StatusCode::UNAUTHORIZED));
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

/// Prometheus metrics endpoint.
/// Returns metrics in Prometheus text format.
async fn handle_metrics(State(state): State<RestState>, headers: HeaderMap) -> impl IntoResponse {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return ApiError::new(StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
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
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
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
) -> Result<Json<ApiResponse<Vec<aikd_core::Session>>>, ApiError> {
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

pub async fn run_rest_server(config_path: &str, port: u16) -> anyhow::Result<()> {
    let cfg = Config::load(config_path).unwrap_or_default();
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
        database,
        tantivy,
        query_cache,
        metrics,
    };

    // Configure CORS with explicit origin whitelist
    // Default: allow only localhost origins for security
    let cors = if cors_origins.contains(&"*".to_string()) {
        log::warn!("CORS: Allowing all origins ('*'). Set specific origins in server.cors_origins for production.");
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
            .allow_headers(tower_http::cors::Any)
    } else if cors_origins.is_empty() {
        // No origins configured — allow localhost only
        let localhost_origins: Vec<axum::http::HeaderValue> = vec![
            "http://localhost:9090".parse().unwrap(),
            "http://127.0.0.1:9090".parse().unwrap(),
            "http://localhost:3000".parse().unwrap(),
            "http://127.0.0.1:3000".parse().unwrap(),
        ];
        CorsLayer::new()
            .allow_origin(localhost_origins)
            .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
            .allow_headers(tower_http::cors::Any)
    } else {
        // Use configured origins
        let origins: Vec<axum::http::HeaderValue> =
            cors_origins.iter().filter_map(|o| o.parse().ok()).collect();
        if origins.is_empty() {
            log::warn!("CORS: No valid origins in config, falling back to localhost only");
            CorsLayer::new()
                .allow_origin(["http://localhost:9090".parse().unwrap()])
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers(tower_http::cors::Any)
        } else {
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers(tower_http::cors::Any)
        }
    };

    // Configure rate limiting: 1 request per second per IP, burst 10.
    // PeerIpKeyExtractor always resolves on our loopback listener; the
    // default smart extractor fails here and rejects everything with 500.
    let governor_conf = tower_governor::governor::GovernorConfigBuilder::default()
        .per_second(1)
        .burst_size(10)
        .key_extractor(tower_governor::key_extractor::PeerIpKeyExtractor)
        .finish()
        .expect("Failed to create rate limiter config");

    let rate_limiter = tower_governor::GovernorLayer {
        config: std::sync::Arc::new(governor_conf),
    };

/// Local dashboard (attachment P7): read-only HTML rendered from live core
/// data. No separate data logic — every number below queries the same
/// storage the CLI/MCP paths use. Localhost only (server binds 127.0.0.1).
async fn handle_ui(State(state): State<RestState>, headers: HeaderMap) -> impl IntoResponse {
    let cfg = state.config.lock().await.clone();
    if !check_auth(&headers, &cfg.server.auth_token) {
        return ApiError::new(StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let database = state.database.lock().await;
    let conn = database.conn();
    let q = |sql: &str| -> String {
        conn.query_row(sql, [], |r| r.get::<_, String>(0))
            .map(|v| html_escape(&v))
            .unwrap_or_else(|_| "?".to_string())
    };
    let qi = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(-1) };
    let files = qi("SELECT COUNT(*) FROM files");
    let chunks = qi("SELECT COUNT(*) FROM chunks");
    let symbols = qi("SELECT COUNT(*) FROM code_symbols");
    let mem_active = qi("SELECT COUNT(*) FROM memory WHERE status='active'");
    let mem_super = qi("SELECT COUNT(*) FROM memory WHERE status='superseded'");
    let events = qi("SELECT COUNT(*) FROM events");
    let sessions = qi("SELECT COUNT(*) FROM sessions");
    let schema_v = q("SELECT version FROM schema_version LIMIT 1");
    let db_mb = std::fs::metadata(cfg.db_path())
        .map(|m| m.len() as f64 / (1024.0 * 1024.0))
        .unwrap_or(-1.0);
    let cover: Vec<(String, String, i64)> = aikd_storage::events::adapter_coverage(conn, "%", "default").unwrap_or_default();
    let mut cover_rows = String::new();
    for (agent, etype, n) in cover.iter().take(20) {
        cover_rows.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{n}</td></tr>", html_escape(agent), html_escape(etype)));
    }
    let mem_rows: String = conn
        .prepare("SELECT kind, title, source, status, updated_at FROM memory ORDER BY updated_at DESC LIMIT 15")
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
            })
            .and_then(|rows| rows.collect::<std::result::Result<Vec<_>, _>>())
        })
        .unwrap_or_default()
        .iter()
        .map(|(k, t, s, st, u)| format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>", html_escape(k), html_escape(t), html_escape(s), html_escape(st), html_escape(u)))
        .collect();
    let body = format!(
        "<!doctype html><html><head><meta charset=utf-8><title>AIKD Control Center</title></head>\
        <body><h1>AIKD Control Center</h1>\
        <p>DB schema v{schema_v} · {db_mb:.1} MB · files {files} · chunks {chunks} · symbols {symbols} · sessions {sessions}</p>\
        <h2>Memory</h2><p>active {mem_active} · superseded {mem_super}</p>\
        <table border=1><tr><th>kind</th><th>title</th><th>source</th><th>status</th><th>updated</th></tr>{mem_rows}</table>\
        <h2>Event journal</h2><p>total events {events}</p>\
        <table border=1><tr><th>agent</th><th>type</th><th>count</th></tr>{cover_rows}</table>\
        <h2>Compaction coverage</h2><p>Checkpoints and task state persist in <code>memory</code> per workstream; \
        resume any agent with <code>aikd handoff --workstream &lt;name&gt;</code> or the MCP <code>handoff</code> tool. \
        No compaction event from an adapter is claimed as protection without a verified lifecycle integration.</p>\
        <h2>Resources</h2><p>Semantic embedding default OFF · indexes incremental · queries served by one service (CLI/MCP/REST share <code>aikd-context</code>).</p>\
        </body></html>"
    );
    ([(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\"', "&quot;")
}

    let app = Router::new()
        .route("/api/health", get(handle_health))
        .route("/api/metrics", get(handle_metrics))
        .route("/metrics/prometheus", get(handle_metrics))
        .route("/api/auth/login", post(handle_login))
        .route("/api/query", get(handle_query))
        .route("/api/stats", get(handle_stats))
        .route("/api/status", get(handle_status))
        .route("/api/scan", post(handle_scan))
        .route("/api/embed", post(handle_embed))
        .route("/api/remember", post(handle_remember))
        .route("/api/recall", post(handle_recall))
        .route("/api/sessions", get(handle_sessions))
        .route("/ui", get(handle_ui))
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
