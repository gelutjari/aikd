//! `aikd top` — live terminal Mission Control ("Siapa · Konek Ke Mana ·
//! Sedang Ngapain").
//!
//! Deliberately dependency-free: the frame comes from raw ANSI escapes and a
//! std-only HTTP/1.1 GET against the daemon's loopback REST API. When the
//! daemon is offline the dashboard degrades to a **read-only** SQLite snapshot
//! (no migrations, no writes) so it keeps working during a restart.

use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aikd_core::Config;

const CLEAR: &str = "\x1b[2J\x1b[H";
const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const RULE: &str = "════════════════════════════════════════════════════════════════════════════";

pub struct TopOptions {
    /// Refresh period in milliseconds.
    pub interval_ms: u64,
    /// Render a single frame and exit (scripts, CI, smoke tests).
    pub once: bool,
}

/// One dashboard frame worth of state, normalized from REST or SQLite.
#[derive(Default)]
struct Snapshot {
    online: bool,
    version: String,
    port: u16,
    pid: i64,
    uptime_secs: i64,
    db_mb: f64,
    files: i64,
    symbols: i64,
    notes: i64,
    events: i64,
    clients_count: i64,
    tokens_saved: i64,
    avg_ms: f64,
    memory_stale: i64,
    memory_conflicted: i64,
    clients: Vec<Value>,
    agents: Vec<Value>,
    terminals: Vec<Value>,
    recent_events: Vec<Value>,
    brief: Option<Value>,
    error: Option<String>,
}

pub fn run(config_path: &str, opts: &TopOptions) -> Result<()> {
    let cfg = Config::load_validated(config_path);
    let port = cfg.server.rest_port;
    let token = cfg.server.auth_token.clone();
    let db_path = cfg.db_path();
    let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);

    if opts.once {
        let snap = collect(port, token.as_deref(), &db_path, &project);
        println!("{}", render(&snap, opts.interval_ms));
        return Ok(());
    }

    let quit = Arc::new(AtomicBool::new(false));
    spawn_key_reader(Arc::clone(&quit));
    #[cfg(windows)]
    let _raw = RawStdin::enable();
    #[cfg(not(windows))]
    let _raw = ();

    while !quit.load(Ordering::Relaxed) {
        let snap = collect(port, token.as_deref(), &db_path, &project);
        print!("{CLEAR}{}", render(&snap, opts.interval_ms));
        use std::io::Write;
        let _ = std::io::stdout().flush();
        // Sleep in small slices so `q` is responsive even at long intervals.
        let deadline = std::time::Instant::now() + Duration::from_millis(opts.interval_ms);
        while std::time::Instant::now() < deadline {
            if quit.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    // Leave the terminal clean for the shell prompt.
    println!("{CLEAR}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Data collection
// ---------------------------------------------------------------------------

fn collect(port: u16, token: Option<&str>, db_path: &Path, project: &str) -> Snapshot {
    // `/api/health` is the cheapest liveness probe and needs no DB writes.
    match http_get_json(port, "/api/health", token) {
        Ok(_) => {
            let overview = http_get_json(port, "/api/overview", token).unwrap_or(Value::Null);
            let live = http_get_json(port, "/api/live", token).unwrap_or(Value::Null);
            let brief = http_get_json(port, "/api/brief?days=1", token).ok();
            snapshot_from_rest(port, &overview, &live, brief)
        }
        Err(e) => {
            let mut snap = snapshot_from_sqlite(port, db_path, project);
            snap.error = Some(e.to_string());
            snap
        }
    }
}

fn snapshot_from_rest(port: u16, overview: &Value, live: &Value, brief: Option<Value>) -> Snapshot {
    let daemon = live.get("daemon").cloned().unwrap_or(Value::Null);
    Snapshot {
        online: true,
        version: s(overview, "version"),
        port,
        pid: i(&daemon, "pid"),
        uptime_secs: i(&daemon, "uptime_secs"),
        db_mb: f(overview, "db_mb"),
        files: i(overview, "files"),
        symbols: i(overview, "symbols"),
        notes: i(overview, "notes_count"),
        events: i(overview, "events"),
        clients_count: i(overview, "active_clients_count"),
        tokens_saved: i(overview, "tokens_saved_est"),
        avg_ms: f(overview, "avg_query_ms"),
        memory_stale: i(overview, "memory_stale"),
        memory_conflicted: i(overview, "memory_conflicted"),
        clients: arr(live, "active_clients"),
        agents: arr(live, "detected_agents"),
        terminals: arr(live, "herdr_terminals"),
        recent_events: arr(live, "recent_events"),
        brief,
        error: None,
    }
}

/// Offline fallback: strictly read-only SQLite (no `run_migrations`, no WAL
/// writes) so `aikd top` never disturbs a daemon that is starting or stopping.
fn snapshot_from_sqlite(port: u16, db_path: &Path, project: &str) -> Snapshot {
    let mut snap = Snapshot {
        version: env!("CARGO_PKG_VERSION").to_string(),
        port,
        ..Default::default()
    };
    let conn = match rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            snap.error = Some(format!("SQLite fallback unavailable: {e}"));
            return snap;
        }
    };
    let qi = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
    snap.db_mb = std::fs::metadata(db_path)
        .map(|m| m.len() as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0);
    snap.files = qi("SELECT COUNT(*) FROM files WHERE status='active'");
    snap.symbols = qi("SELECT COUNT(*) FROM code_symbols");
    snap.notes = qi("SELECT COUNT(*) FROM notes");
    snap.events = qi("SELECT COUNT(*) FROM events");
    snap.memory_stale = qi("SELECT COUNT(*) FROM memory WHERE status='stale'");
    snap.memory_conflicted = qi("SELECT COUNT(*) FROM memory WHERE status='conflicted'");
    if let Ok((saved, _, avg)) = aikd_storage::telemetry::token_savings(&conn) {
        snap.tokens_saved = saved;
        snap.avg_ms = avg;
    }
    if let Ok(clients) = aikd_storage::telemetry::list_active_clients(&conn) {
        snap.clients_count = clients.len() as i64;
        snap.clients = clients
            .iter()
            .map(|c| serde_json::to_value(c).unwrap_or(Value::Null))
            .collect();
    }
    snap.terminals = aikd_storage::telemetry::herdr_terminals()
        .iter()
        .map(|t| serde_json::to_value(t).unwrap_or(Value::Null))
        .collect();
    snap.recent_events = aikd_storage::events::recent_events(&conn, 20)
        .unwrap_or_default()
        .iter()
        .map(|e| {
            let summary = serde_json::from_str::<Value>(&e.payload)
                .ok()
                .and_then(|v| {
                    v.get("summary")
                        .or_else(|| v.get("query"))
                        .or_else(|| v.get("tool"))
                        .and_then(|x| x.as_str())
                        .map(str::to_string)
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| e.event_type.clone());
            serde_json::json!({
                "agent": e.agent_id,
                "event_type": e.event_type,
                "summary": format!("{}: {}", e.event_type, summary),
                "created_at": e.timestamp,
            })
        })
        .collect();
    snap.agents = aikd_core::agents::detect_agent_statuses()
        .iter()
        .filter(|a| a.detected)
        .map(|a| {
            serde_json::json!({
                "name": a.name,
                "exe": a.kind,
                "pid": 0,
                "cwd": "",
                "registered": a.registered,
                "connected_to_aikd": false,
            })
        })
        .collect();
    let _ = project;
    snap
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(snap: &Snapshot, interval_ms: u64) -> String {
    let mut out = String::new();
    let (dot, status) = if snap.online {
        (format!("{GREEN}●{RESET}"), "healthy")
    } else {
        (format!("{RED}●{RESET}"), "offline")
    };
    let mut head = format!(
        " {BOLD}{CYAN}AIKD LIVE{RESET}  v{ver}   daemon {dot} {status} :{port}",
        ver = if snap.version.is_empty() {
            env!("CARGO_PKG_VERSION").to_string()
        } else {
            snap.version.clone()
        },
        port = snap.port,
    );
    if snap.online {
        head.push_str(&format!(
            "   pid {}  up {}",
            snap.pid,
            human_duration(snap.uptime_secs)
        ));
    }
    out.push_str(RULE);
    out.push('\n');
    out.push_str(&head);
    out.push('\n');
    out.push_str(RULE);
    out.push('\n');

    out.push_str(&format!(
        " {DIM}DB{RESET} {:.1} MB · {DIM}files{RESET} {} · {DIM}symbols{RESET} {} · {DIM}notes{RESET} {} · {DIM}events{RESET} {} · {DIM}clients{RESET} {}\n",
        snap.db_mb,
        n(snap.files),
        n(snap.symbols),
        n(snap.notes),
        n(snap.events),
        n(snap.clients_count)
    ));
    out.push_str(&format!(
        " {BOLD}Tokens saved est{RESET} {} · {DIM}avg query{RESET} {:.1} ms",
        n(snap.tokens_saved),
        snap.avg_ms
    ));
    if snap.memory_conflicted > 0 {
        out.push_str(&format!(
            " · {YELLOW}{} conflicted{RESET}",
            snap.memory_conflicted
        ));
    }
    if snap.memory_stale > 0 {
        out.push_str(&format!(" · {YELLOW}{} stale{RESET}", snap.memory_stale));
    }
    out.push('\n');
    if let Some(err) = &snap.error {
        out.push_str(&format!(
            " {DIM}daemon offline ({}) — read-only SQLite fallback; start with `aikd daemon`{RESET}\n",
            clip(err, 60)
        ));
    }
    out.push('\n');

    out.push_str(&format!(
        "{CYAN}── MISSION CONTROL · SIAPA · KONEK KE MANA · SEDANG NGAPAIN ──{RESET}\n"
    ));
    if snap.clients.is_empty() {
        out.push_str(&format!("  {DIM}(no MCP/REST clients connected){RESET}\n"));
    } else {
        out.push_str(&format!(
            "  {DIM}{:<24} {:<11} {:<20} {}{RESET}\n",
            "WHO", "TRANSPORT", "WHERE", "DOING"
        ));
        for c in snap.clients.iter().take(8) {
            let who = {
                let id = s(c, "client_id");
                let name = s(c, "mcp_client_name");
                if name.is_empty() {
                    id
                } else {
                    format!("{id} ({name})")
                }
            };
            let mut doing = String::new();
            let tool = s(c, "last_tool");
            let target = s(c, "last_target");
            if !tool.is_empty() {
                doing.push_str(&tool);
            }
            if !target.is_empty() {
                doing.push(':');
                doing.push_str(&target);
            }
            if doing.is_empty() {
                doing.push('-');
            }
            out.push_str(&format!(
                "  {:<24} {:<11} {:<20} {}\n",
                clip(&who, 24),
                clip(&s(c, "transport"), 11),
                clip(&basename(&s(c, "project")), 20),
                clip(&doing, 30)
            ));
        }
    }

    if !snap.agents.is_empty() || !snap.terminals.is_empty() {
        out.push('\n');
        out.push_str(&format!("{CYAN}── DETECTED AGENTS & TERMINALS ──{RESET}\n"));
        for a in snap.agents.iter().take(6) {
            let linked = a
                .get("connected_to_aikd")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let registered = a
                .get("registered")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mark = if linked {
                format!("{GREEN}linked{RESET}")
            } else if registered {
                format!("{DIM}registered{RESET}")
            } else {
                format!("{DIM}idle{RESET}")
            };
            let pid = i(a, "pid");
            out.push_str(&format!(
                "  {} {:<14} pid {:<7} {:<24} {}\n",
                if linked { "●" } else { "○" },
                clip(&s(a, "name"), 14),
                if pid > 0 { pid.to_string() } else { "-".into() },
                clip(&basename(&s(a, "cwd")), 24),
                mark
            ));
        }
        for t in snap.terminals.iter().take(6) {
            out.push_str(&format!(
                "  ▪ {:<10} {:<18} {:<10} {}\n",
                clip(&s(t, "id"), 10),
                clip(&s(t, "name"), 18),
                clip(&s(t, "app"), 10),
                clip(&s(t, "status"), 20)
            ));
        }
    }

    out.push('\n');
    out.push_str(&format!("{CYAN}── RECENT EVENTS ──{RESET}\n"));
    if snap.recent_events.is_empty() {
        out.push_str(&format!("  {DIM}(no events yet){RESET}\n"));
    } else {
        for e in snap.recent_events.iter().take(8) {
            out.push_str(&format!(
                "  {DIM}{}{RESET} {:<14} {}\n",
                time_part(&s(e, "created_at")),
                clip(&s(e, "agent"), 14),
                clip(&s(e, "summary"), 48)
            ));
        }
    }

    out.push('\n');
    out.push_str(&format!("{CYAN}── MORNING BRIEF (last 24h) ──{RESET}\n"));
    let bullets: Vec<Value> = snap
        .brief
        .as_ref()
        .and_then(|b| b.get("bullets"))
        .and_then(|b| b.as_array())
        .cloned()
        .unwrap_or_default();
    if snap.brief.is_none() {
        out.push_str(&format!(
            "  {DIM}(brief unavailable — daemon offline){RESET}\n"
        ));
    } else if bullets.is_empty() {
        out.push_str(&format!("  {DIM}(nothing recent — start a daily log with `aikd send --kind daily_log`){RESET}\n"));
    } else {
        for b in bullets.iter().take(6) {
            out.push_str(&format!("  • {}\n", clip(b.as_str().unwrap_or(""), 72)));
        }
    }

    out.push('\n');
    out.push_str(&format!(
        "{DIM}refresh {:.1}s · q / Ctrl+C to exit{RESET}\n",
        interval_ms as f64 / 1000.0
    ));
    out
}

fn human_duration(secs: i64) -> String {
    if secs <= 0 {
        "0m".to_string()
    } else if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d{}h", secs / 86_400, (secs % 86_400) / 3600)
    }
}

/// Thousands separators without pulling in a formatting crate.
fn n(v: i64) -> String {
    let digits = v.unsigned_abs().to_string();
    let mut out = String::new();
    for (idx, ch) in digits.chars().enumerate() {
        if idx > 0 && (digits.len() - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if v < 0 {
        format!("-{out}")
    } else {
        out
    }
}

fn clip(text: &str, max: usize) -> String {
    let flat = text.replace(['\n', '\r'], " ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

fn time_part(ts: &str) -> String {
    ts.get(11..16).unwrap_or("--:--").to_string()
}

fn s(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn i(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(0)
}

fn f(v: &Value, key: &str) -> f64 {
    v.get(key)
        .and_then(|x| x.as_f64())
        .or_else(|| v.get(key).and_then(|x| x.as_i64()).map(|n| n as f64))
        .unwrap_or(0.0)
}

fn arr(v: &Value, key: &str) -> Vec<Value> {
    v.get(key)
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Minimal loopback HTTP client (no TLS, no extra crates)
// ---------------------------------------------------------------------------

fn http_get_json(port: u16, path: &str, token: Option<&str>) -> Result<Value> {
    let body = http_get(port, path, token)?;
    let value: Value = serde_json::from_str(&body)?;
    if value.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("request failed");
        anyhow::bail!("{msg}");
    }
    Ok(value.get("data").cloned().unwrap_or(value))
}

fn http_get(port: u16, path: &str, token: Option<&str>) -> Result<String> {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(600))?;
    stream.set_read_timeout(Some(Duration::from_millis(1_500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(800)))?;

    let mut req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes())?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or(("", text.as_ref()));
    let status_line = head.lines().next().unwrap_or("");
    if !status_line.contains(" 200 ") {
        anyhow::bail!(
            "HTTP {}",
            if status_line.is_empty() {
                "no response"
            } else {
                status_line
            }
        );
    }
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    Ok(if chunked {
        dechunk(body)
    } else {
        body.to_string()
    })
}

/// Decode `Transfer-Encoding: chunked` bodies (hyper usually sends
/// `Content-Length` for JSON, but a proxy or streamed route may not).
fn dechunk(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let Some(rel) = body[pos..].find("\r\n") else {
            break;
        };
        let Ok(size) = usize::from_str_radix(body[pos..pos + rel].trim(), 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        let start = pos + rel + 2;
        let end = start.saturating_add(size).min(bytes.len());
        out.extend_from_slice(&bytes[start..end]);
        pos = end.saturating_add(2);
    }
    String::from_utf8_lossy(&out).to_string()
}

// ---------------------------------------------------------------------------
// Keyboard input
// ---------------------------------------------------------------------------

/// Treat a key press as a quit signal. On Windows the console is switched out
/// of line-buffered mode so a single `q` works; elsewhere `q` + Enter does.
fn spawn_key_reader(quit: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = [0u8; 1];
        let mut stdin = std::io::stdin();
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    // 0x03 = Ctrl+C (delivered as input once processed input is off).
                    if matches!(buf[0], b'q' | b'Q' | 0x03) {
                        quit.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(windows)]
mod win32 {
    pub const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
    pub const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    pub const ENABLE_LINE_INPUT: u32 = 0x0002;
    pub const ENABLE_ECHO_INPUT: u32 = 0x0004;

    extern "system" {
        pub fn GetStdHandle(std_handle: u32) -> isize;
        pub fn GetConsoleMode(handle: isize, mode: *mut u32) -> i32;
        pub fn SetConsoleMode(handle: isize, mode: u32) -> i32;
    }
}

/// Keeps the console in single-key mode for the dashboard lifetime and always
/// restores the original mode on a clean exit.
#[cfg(windows)]
struct RawStdin {
    handle: isize,
    original: u32,
}

#[cfg(windows)]
impl RawStdin {
    fn enable() -> Option<Self> {
        use win32::*;
        // SAFETY: standard-console handle queries; no memory is retained and
        // the original mode is restored in Drop.
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            if handle == 0 || handle == -1 {
                return None;
            }
            let mut mode = 0u32;
            if GetConsoleMode(handle, &mut mode) == 0 {
                return None;
            }
            let raw = mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT);
            if SetConsoleMode(handle, raw) == 0 {
                return None;
            }
            Some(Self {
                handle,
                original: mode,
            })
        }
    }
}

#[cfg(windows)]
impl Drop for RawStdin {
    fn drop(&mut self) {
        // SAFETY: `handle` came from GetStdHandle and `original` from GetConsoleMode.
        unsafe {
            win32::SetConsoleMode(self.handle, self.original);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_get_grouped() {
        assert_eq!(n(0), "0");
        assert_eq!(n(999), "999");
        assert_eq!(n(1_234), "1,234");
        assert_eq!(n(12_345_678), "12,345,678");
        assert_eq!(n(-1_500), "-1,500");
    }

    #[test]
    fn clip_respects_char_boundaries() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("abcdefghij", 5), "abcd…");
        assert_eq!(clip("a\nb", 5), "a b");
        // Multi-byte input must not panic or split a char.
        assert_eq!(clip("日本語テスト", 4), "日本語…");
    }

    #[test]
    fn basename_handles_both_separators() {
        assert_eq!(basename("C:/Users/hp/aikd-v21"), "aikd-v21");
        assert_eq!(basename("C:\\Users\\hp\\aikd-v21\\"), "aikd-v21");
        assert_eq!(basename(""), "");
    }

    #[test]
    fn dechunk_decodes_multiple_frames() {
        assert_eq!(dechunk("5\r\nhello\r\n1\r\n!\r\n0\r\n\r\n"), "hello!");
        assert_eq!(dechunk("not chunked"), "");
    }

    #[test]
    fn rest_snapshot_reads_contract_fields() {
        let overview = serde_json::json!({
            "version": "2.1.0",
            "db_mb": 12.5,
            "files": 42,
            "symbols": 180,
            "notes_count": 3,
            "events": 7,
            "active_clients_count": 2,
            "tokens_saved_est": 48200,
            "avg_query_ms": 4.2,
            "memory_stale": 1,
            "memory_conflicted": 0
        });
        let live = serde_json::json!({
            "daemon": {"pid": 99, "uptime_secs": 125},
            "active_clients": [{"client_id": "mcp-1", "transport": "mcp-stdio", "project": "C:/p/aikd-v21", "last_tool": "query", "last_target": "impact"}],
            "detected_agents": [],
            "herdr_terminals": [],
            "recent_events": []
        });
        let brief = serde_json::json!({"bullets": ["did a thing"]});
        let snap = snapshot_from_rest(9091, &overview, &live, Some(brief));
        assert!(snap.online);
        assert_eq!(snap.version, "2.1.0");
        assert_eq!(snap.files, 42);
        assert_eq!(snap.tokens_saved, 48_200);
        assert_eq!(snap.pid, 99);
        assert_eq!(snap.clients.len(), 1);
        let frame = render(&snap, 2_000);
        assert!(frame.contains("MISSION CONTROL"));
        assert!(frame.contains("mcp-1"));
        assert!(frame.contains("48,200"));
        assert!(frame.contains("did a thing"));
    }

    #[test]
    fn offline_frame_is_honest() {
        let snap = Snapshot {
            version: "2.1.0".into(),
            port: 9091,
            error: Some("connection refused".into()),
            ..Default::default()
        };
        let frame = render(&snap, 2_000);
        assert!(frame.contains("offline"));
        assert!(frame.contains("read-only SQLite fallback"));
    }
}
