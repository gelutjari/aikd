use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginQuery {
    pub query: String,
    pub limit: Option<usize>,
    pub path_filter: Option<String>,
    pub heading_filter: Option<String>,
    pub hybrid: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginResult {
    pub chunk_id: String,
    pub file_path: String,
    pub heading_hierarchy: String,
    pub heading_text: String,
    pub content: String,
    pub line_start: usize,
    pub line_end: usize,
    pub score: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginRemember {
    pub session_id: Option<String>,
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginRecall {
    pub query: String,
    pub session_id: Option<String>,
    pub limit: Option<usize>,
}

pub const REST_API_BASE: &str = "http://127.0.0.1:9090";
pub const ENDPOINT_QUERY: &str = "/api/query";
pub const ENDPOINT_STATS: &str = "/api/stats";
pub const ENDPOINT_SCAN: &str = "/api/scan";
pub const ENDPOINT_REMEMBER: &str = "/api/remember";
pub const ENDPOINT_RECALL: &str = "/api/recall";
