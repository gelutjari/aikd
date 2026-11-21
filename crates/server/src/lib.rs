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
        let cfg = Config::load_validated(&config_path);
        Self {
            config: Arc::new(Mutex::new(cfg)),
            config_path,
        }
    }
}

/// Singleton guard for the LONG-LIVED REST daemon only: one device runs ONE
/// daemon (one TCP port), while MANY agents connect to it via REST — and
/// every agent session additionally spawns its own short-lived MCP `serve`
/// on private stdio (no lock needed there: SQLite WAL + idempotent writes).
/// Uses an exclusive non-blocking file lock — released automatically by the
/// OS when the process dies, so there is never a stale lock to clean up.
fn acquire_singleton(who: &str) -> anyhow::Result<std::fs::File> {
    use fs2::FileExt;
    let dir = shellexpand::tilde("~/.aikd");
    std::fs::create_dir_all(dir.as_ref())?;
    let lock_path = format!("{dir}/aikd.lock");
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path)?;
    match f.try_lock_exclusive() {
        Ok(()) => {
            // Record owner for diagnostics (`aikd status` / doctor).
            let _ = f.set_len(0);
            use std::io::Write;
            let mut f2 = &f;
            let _ = writeln!(f2, "{} pid={} at={}", who, std::process::id(), chrono::Utc::now().to_rfc3339());
            Ok(f)
        }
        Err(_) => {
            let owner = std::fs::read_to_string(&lock_path).unwrap_or_default();
            anyhow::bail!(
                "another AIKD server already runs on this device ({owner_trim}). Multi agent share ONE instance — connect via MCP `aikd serve` config or REST :9090, do not start a second server.",
                owner_trim = owner.trim()
            )
        }
    }
}

pub async fn run_mcp_server(config_path: &str) -> anyhow::Result<()> {
    // NO file lock here by design: every agent session spawns its own short-
    // lived `serve` on private stdio. SQLite WAL + idempotent writes
    // (INSERT OR IGNORE, BEGIN IMMEDIATE) make concurrent readers/writers
    // safe. Only the long-lived REST daemon is singleton-guarded below —
    // one device, one TCP port, many agents.
    eprintln!("Starting AIKD MCP server (stdio transport)...");
    mcp::run_server(config_path).await
}

pub async fn run_rest_server(config_path: &str) -> anyhow::Result<()> {
    let _guard = acquire_singleton("rest")?;
    let cfg = Config::load_validated(config_path);
    let port = cfg.server.rest_port;
    rest::run_rest_server(config_path, port).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn singleton_second_daemon_fails() {
        let _first = super::acquire_singleton("rest").expect("first daemon wins");
        let second = super::acquire_singleton("test-b");
        assert!(second.is_err(), "second daemon on one device must refuse");
        let msg = second.unwrap_err().to_string();
        assert!(msg.contains("already runs"), "error must name the sharing rule: {msg}");
    }
}
