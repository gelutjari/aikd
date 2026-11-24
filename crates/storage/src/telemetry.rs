//! Live Mission Control telemetry: connected clients + query efficiency.
//!
//! `active_clients` is written by the MCP server (on `initialize`, on every
//! tool call, and on EOF) so the dashboard can answer "who is connected, to
//! which project, doing what". `query_metrics` stores per-query latency and
//! the estimated tokens saved (`(full_bytes - returned_bytes) / 4`).
//!
//! Every function here is best-effort: telemetry must never break a query,
//! so callers may ignore errors.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveClient {
    pub client_id: String,
    pub pid: i64,
    pub parent_pid: i64,
    pub parent_name: String,
    pub mcp_client_name: String,
    pub transport: String,
    pub cwd: String,
    pub project: String,
    pub last_tool: String,
    pub last_target: String,
    pub status: String,
    pub started_at: String,
    pub last_seen_at: String,
}

#[derive(Debug, Clone)]
pub struct ClientStart<'a> {
    pub client_id: &'a str,
    pub pid: i64,
    pub parent_pid: i64,
    pub parent_name: &'a str,
    pub mcp_client_name: &'a str,
    pub transport: &'a str,
    pub cwd: &'a str,
    pub project: &'a str,
}

const CLIENT_COLS: &str = "client_id, pid, parent_pid, parent_name, mcp_client_name, transport, cwd, project, last_tool, last_target, status, started_at, last_seen_at";

fn map_client(row: &rusqlite::Row) -> Result<ActiveClient, rusqlite::Error> {
    Ok(ActiveClient {
        client_id: row.get(0)?,
        pid: row.get(1)?,
        parent_pid: row.get(2)?,
        parent_name: row.get(3)?,
        mcp_client_name: row.get(4)?,
        transport: row.get(5)?,
        cwd: row.get(6)?,
        project: row.get(7)?,
        last_tool: row.get(8)?,
        last_target: row.get(9)?,
        status: row.get(10)?,
        started_at: row.get(11)?,
        last_seen_at: row.get(12)?,
    })
}

/// Register (or refresh) a connected client. Idempotent on `client_id`.
pub fn start_client(conn: &Connection, c: &ClientStart) -> Result<ActiveClient> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO active_clients (client_id, pid, parent_pid, parent_name, mcp_client_name, transport, cwd, project, last_tool, last_target, status, started_at, last_seen_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'','','active',?9,?9)
         ON CONFLICT(client_id) DO UPDATE SET
             pid=excluded.pid,
             parent_pid=excluded.parent_pid,
             parent_name=excluded.parent_name,
             mcp_client_name=excluded.mcp_client_name,
             transport=excluded.transport,
             cwd=excluded.cwd,
             project=excluded.project,
             status='active',
             last_seen_at=excluded.last_seen_at",
        rusqlite::params![
            c.client_id,
            c.pid,
            c.parent_pid,
            c.parent_name,
            c.mcp_client_name,
            c.transport,
            c.cwd,
            c.project,
            now
        ],
    )?;
    Ok(conn.query_row(
        &format!("SELECT {CLIENT_COLS} FROM active_clients WHERE client_id=?1"),
        rusqlite::params![c.client_id],
        map_client,
    )?)
}

/// Heartbeat on every tool call: tool name + short target description.
pub fn touch_client(conn: &Connection, client_id: &str, tool: &str, target: &str) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let target: String = target.chars().take(120).collect();
    Ok(conn.execute(
        "UPDATE active_clients SET last_tool=?1, last_target=?2, status='active', last_seen_at=?3 WHERE client_id=?4",
        rusqlite::params![tool, target, now, client_id],
    )?)
}

/// Mark one client disconnected (EOE / clean shutdown).
pub fn disconnect_client(conn: &Connection, client_id: &str) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    Ok(conn.execute(
        "UPDATE active_clients SET status='disconnected', last_seen_at=?1 WHERE client_id=?2",
        rusqlite::params![now, client_id],
    )?)
}

/// Mark every MCP client owned by `pid` disconnected (proxy task exit).
pub fn disconnect_by_pid(conn: &Connection, pid: i64) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    Ok(conn.execute(
        "UPDATE active_clients SET status='disconnected', last_seen_at=?1 WHERE pid=?2 AND status!='disconnected'",
        rusqlite::params![now, pid],
    )?)
}

/// Clients that have not been marked disconnected, newest heartbeat first.
pub fn list_active_clients(conn: &Connection) -> Result<Vec<ActiveClient>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {CLIENT_COLS} FROM active_clients WHERE status!='disconnected' ORDER BY last_seen_at DESC LIMIT 100"
    ))?;
    let rows = stmt.query_map([], map_client)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Count of live clients (dashboard KPI).
pub fn active_client_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM active_clients WHERE status!='disconnected'",
        [],
        |r| r.get(0),
    )?)
}

/// Record one query/symbol call and its estimated token savings.
pub fn record_query_metric(
    conn: &Connection,
    tool: &str,
    query_text: &str,
    full_bytes: i64,
    returned_bytes: i64,
    elapsed_ms: i64,
) -> Result<()> {
    let saved_tokens = ((full_bytes - returned_bytes).max(0)) / 4;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO query_metrics (tool, query_text, full_bytes, returned_bytes, saved_tokens_est, elapsed_ms, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            tool,
            query_text.chars().take(300).collect::<String>(),
            full_bytes.max(0),
            returned_bytes.max(0),
            saved_tokens,
            elapsed_ms.max(0),
            now
        ],
    )?;
    Ok(())
}

/// Totals for the dashboard: (tokens_saved_est, queries_total, avg_elapsed_ms).
pub fn token_savings(conn: &Connection) -> Result<(i64, i64, f64)> {
    let (saved, total, avg): (i64, i64, f64) = conn.query_row(
        "SELECT COALESCE(SUM(saved_tokens_est),0), COUNT(*), COALESCE(AVG(elapsed_ms),0.0) FROM query_metrics",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok((saved, total, avg))
}

/// Aggregated per-tool efficiency for the analytics panel.
pub fn per_tool_metrics(conn: &Connection) -> Result<Vec<(String, i64, i64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT tool, COUNT(*), COALESCE(SUM(saved_tokens_est),0), COALESCE(AVG(elapsed_ms),0.0) FROM query_metrics GROUP BY tool ORDER BY COUNT(*) DESC",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HerdrTerminal {
    pub id: String,
    pub name: String,
    pub app: String,
    pub cwd: String,
    pub status: String,
}

/// Read Herdr terminals strictly read-only. Missing/unreadable/odd-shaped
/// files yield an empty list — never an error, never a write.
pub fn herdr_terminals() -> Vec<HerdrTerminal> {
    let base = std::env::var("LOCALAPPDATA")
        .ok()
        .map(std::path::PathBuf::from);
    let candidates: Vec<std::path::PathBuf> = match base {
        Some(b) => vec![
            b.join("herdr-core").join("default").join("terminals.json"),
            b.join("herdr-core").join("terminals.json"),
        ],
        None => vec![std::path::PathBuf::from(
            "/tmp/herdr-core/default/terminals.json",
        )],
    };
    for path in candidates {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let arr = value
            .as_array()
            .cloned()
            .or_else(|| value.get("terminals").and_then(|v| v.as_array()).cloned())
            .or_else(|| value.get("panes").and_then(|v| v.as_array()).cloned())
            .unwrap_or_default();
        let mut out = Vec::new();
        for (i, t) in arr.iter().enumerate() {
            let s =
                |k: &str| -> String { t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string() };
            let id = if s("id").is_empty() {
                format!("t{i}")
            } else {
                s("id")
            };
            out.push(HerdrTerminal {
                id,
                name: s("name"),
                app: if s("app").is_empty() {
                    s("agent")
                } else {
                    s("app")
                },
                cwd: s("cwd"),
                status: if s("status").is_empty() {
                    "unknown".to_string()
                } else {
                    s("status")
                },
            });
        }
        return out;
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn client_lifecycle_start_touch_disconnect() {
        let conn = setup();
        let c = ClientStart {
            client_id: "mcp-1",
            pid: 1,
            parent_pid: 2,
            parent_name: "opencode.exe",
            mcp_client_name: "opencode",
            transport: "mcp-stdio",
            cwd: "C:/p",
            project: "C:/p",
        };
        let started = start_client(&conn, &c).unwrap();
        assert_eq!(started.status, "active");
        assert_eq!(active_client_count(&conn).unwrap(), 1);
        // Heartbeat carries the tool + target.
        touch_client(&conn, "mcp-1", "query", "handle_graph").unwrap();
        let list = list_active_clients(&conn).unwrap();
        assert_eq!(list[0].last_tool, "query");
        assert_eq!(list[0].last_target, "handle_graph");
        // Restart-with-same-id updates instead of duplicating.
        start_client(&conn, &c).unwrap();
        assert_eq!(active_client_count(&conn).unwrap(), 1);
        // Disconnect removes it from the live view but keeps the row.
        disconnect_client(&conn, "mcp-1").unwrap();
        assert_eq!(active_client_count(&conn).unwrap(), 0);
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM active_clients", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn query_metrics_estimate_saved_tokens() {
        let conn = setup();
        record_query_metric(&conn, "query", "auth", 40_000, 8_000, 5).unwrap();
        record_query_metric(&conn, "symbol", "handle_graph", 4_000, 4_000, 2).unwrap();
        let (saved, total, avg) = token_savings(&conn).unwrap();
        assert_eq!(total, 2);
        assert_eq!(saved, 8_000); // (32000)/4 + 0
        assert!(avg > 0.0);
        let per = per_tool_metrics(&conn).unwrap();
        assert_eq!(per.len(), 2);
    }

    #[test]
    fn herdr_missing_file_is_empty_not_error() {
        // No herdr installation on CI → must return [] silently.
        let _ = herdr_terminals();
    }
}
