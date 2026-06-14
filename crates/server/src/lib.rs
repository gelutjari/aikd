pub mod mcp;
pub mod rest;

use aikd_core::Config;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub config_path: String,
}

impl AppState {
    pub fn new(config_path: String) -> Self {
        let cfg = Config::load(&config_path).unwrap_or_default();
        Self {
            config: Arc::new(Mutex::new(cfg)),
            config_path,
        }
    }
}

pub async fn run_mcp_server(config_path: &str) -> anyhow::Result<()> {
    eprintln!("Starting AIKD MCP server (stdio transport)...");
    mcp::run_server(config_path).await
}

pub async fn run_rest_server(config_path: &str) -> anyhow::Result<()> {
    let cfg = Config::load(config_path).unwrap_or_default();
    let port = cfg.server.rest_port;
    rest::run_rest_server(config_path, port).await
}
