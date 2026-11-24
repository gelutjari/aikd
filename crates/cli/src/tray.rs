#![cfg_attr(windows, windows_subsystem = "windows")]
//! AIKD system tray companion (CONTRACT_V21 §6).
//!
//! Thin remote for the ONE daemon: a 3-color status light (green / yellow /
//! red), an App-mode launcher for AIKD Studio, the `Ctrl+Alt+N` quick-capture
//! hotkey, clipboard capture, the auto-watcher toggle and the daemon starter.
//! It never indexes anything itself — all work happens in `aikd` + the daemon
//! REST API.
//!
//! Zero extra crates: Win32 (mutex / hotkey / clipboard) is reached through
//! direct FFI and the daemon is polled with a tiny blocking HTTP/1.1 client.
//!
//! Build with: `cargo build --features tray --bin aikd-tray`
//! (default builds stay GUI-free).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(windows))]
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(not(windows))]
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
#[cfg(not(windows))]
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
#[cfg(not(windows))]
use winit::application::ApplicationHandler;
#[cfg(not(windows))]
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};

/// How often the daemon is re-probed (contract: ~8s).
const POLL_INTERVAL: Duration = Duration::from_secs(8);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(700);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// CREATE_NO_WINDOW — never flash a console for spawned helpers.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const COLOR_GREEN: [u8; 3] = [0x22, 0xc5, 0x5e];
const COLOR_YELLOW: [u8; 3] = [0xf5, 0x9e, 0x0b];
const COLOR_RED: [u8; 3] = [0xef, 0x44, 0x44];
/// Dark bolt drawn on top of the status disc (readable on all 3 colors).
const BOLT_RGB: [u8; 3] = [0x0f, 0x17, 0x2a];

static LAST_OPEN_MS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Runtime config (port / token / config path from the shared aikd config)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TrayConfig {
    config_path: String,
    port: u16,
    auth_token: Option<String>,
}

impl TrayConfig {
    fn load() -> Self {
        let config_path = std::env::var("AIKD_CONFIG")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "~/.aikd/config.yaml".to_string());
        let cfg = aikd_core::Config::load_validated(&config_path);
        Self {
            config_path,
            port: cfg.server.rest_port,
            auth_token: cfg.server.auth_token,
        }
    }
}

fn aikd_exe() -> std::path::PathBuf {
    let bin = if cfg!(windows) { "aikd.exe" } else { "aikd" };
    if let Ok(cur) = std::env::current_exe() {
        if let Some(dir) = cur.parent() {
            let sibling = dir.join(bin);
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    std::path::PathBuf::from("aikd")
}

// ---------------------------------------------------------------------------
// Minimal blocking HTTP/1.1 client (localhost only, no extra crates)
// ---------------------------------------------------------------------------

fn http_call(
    cfg: &TrayConfig,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{}", cfg.port);
    let sock: SocketAddr = addr.parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&sock, CONNECT_TIMEOUT).ok()?;
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));

    let payload = body.unwrap_or("");
    let mut req = String::with_capacity(256 + payload.len());
    req.push_str(&format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nAccept: application/json\r\n"
    ));
    if let Some(token) = cfg.auth_token.as_deref().filter(|t| !t.is_empty()) {
        req.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if method != "GET" {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    req.push_str("\r\n");

    stream.write_all(req.as_bytes()).ok()?;
    if !payload.is_empty() {
        stream.write_all(payload.as_bytes()).ok()?;
    }
    stream.flush().ok()?;

    // `Connection: close` makes the server end the stream; be forgiving about
    // resets after the body so a usable response is never thrown away.
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    }
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Option<(u16, String)> {
    let sep = find_subslice(raw, b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..sep]);
    let body = &raw[sep + 4..];

    let mut lines = head.lines();
    let status: u16 = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let mut chunked = false;
    let mut content_length: Option<usize> = None;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        } else if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().ok();
        }
    }
    let bytes = if chunked {
        decode_chunked(body)
    } else if let Some(n) = content_length {
        body[..n.min(body.len())].to_vec()
    } else {
        body.to_vec()
    };
    Some((status, String::from_utf8_lossy(&bytes).to_string()))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn decode_chunked(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(pos) = find_subslice(data, b"\r\n") else {
            break;
        };
        let size_line = String::from_utf8_lossy(&data[..pos]);
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size_hex, 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        let start = pos + 2;
        let end = (start + size).min(data.len());
        out.extend_from_slice(&data[start..end]);
        if end + 2 > data.len() {
            break;
        }
        data = &data[end + 2..];
    }
    out
}

// ---------------------------------------------------------------------------
// Daemon snapshot (drives the 3-color icon + status header)
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Snapshot {
    online: bool,
    files: i64,
    notes: i64,
    clients: i64,
    conflicted: i64,
    stale: i64,
    projects: usize,
    stale_projects: usize,
}

fn fetch_snapshot(cfg: &TrayConfig) -> Snapshot {
    let Some((status, body)) = http_call(cfg, "GET", "/api/overview", None) else {
        return Snapshot::default();
    };
    if status != 200 {
        return Snapshot::default();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Snapshot::default();
    };
    let data = value.get("data").cloned().unwrap_or(value);
    let num = |key: &str| data.get(key).and_then(|v| v.as_i64()).unwrap_or(0);
    let projects = data
        .get("projects")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let stale_projects = projects
        .iter()
        .filter(|p| p.get("stale").and_then(|s| s.as_bool()).unwrap_or(false))
        .count();

    Snapshot {
        online: true,
        files: num("files"),
        notes: num("notes_count"),
        clients: num("active_clients_count"),
        conflicted: num("memory_conflicted"),
        stale: num("memory_stale"),
        projects: projects.len(),
        stale_projects,
    }
}

fn status_color(snap: &Snapshot) -> [u8; 3] {
    if !snap.online {
        COLOR_RED
    } else if snap.conflicted > 0 || snap.stale > 0 || snap.stale_projects > 0 {
        COLOR_YELLOW
    } else {
        COLOR_GREEN
    }
}

fn status_text(snap: &Snapshot) -> String {
    if !snap.online {
        return "AIKD - daemon offline".to_string();
    }
    let mut text = format!(
        "AIKD - {} files - {} notes - {} clients",
        snap.files, snap.notes, snap.clients
    );
    if snap.conflicted > 0 || snap.stale > 0 {
        text.push_str(&format!(
            " - {} conflicted / {} stale",
            snap.conflicted, snap.stale
        ));
    }
    if snap.stale_projects > 0 {
        text.push_str(&format!(" - {} stale projects", snap.stale_projects));
    }
    text
}

fn tooltip_text(snap: &Snapshot) -> String {
    if !snap.online {
        return "AIKD - daemon offline (click to open AIKD Studio)".to_string();
    }
    format!(
        "AIKD - online | {} files | {} notes | {} clients | {} projects",
        snap.files, snap.notes, snap.clients, snap.projects
    )
}

/// 32x32 RGBA disc in `rgb` with a dark lightning bolt, anti-aliased edge.
fn status_icon_rgba(rgb: [u8; 3]) -> Vec<u8> {
    const SIZE: u32 = 32;
    const RADIUS: f32 = 14.6;
    const CENTER: f32 = 16.0;

    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            // 3x3 supersampling keeps the 32px circle smooth at every DPI.
            let mut covered = 0u32;
            for sy in 0..3 {
                for sx in 0..3 {
                    let fx = x as f32 + (sx as f32 + 0.5) / 3.0;
                    let fy = y as f32 + (sy as f32 + 0.5) / 3.0;
                    let d = ((fx - CENTER).powi(2) + (fy - CENTER).powi(2)).sqrt();
                    if d <= RADIUS {
                        covered += 1;
                    }
                }
            }
            if covered == 0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let alpha = (covered * 255 / 9) as u8;
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let in_bolt =
                (fy >= 6.0 && fy <= 17.0 && fx >= (18.0 - (fy - 6.0) * 0.6) && fx <= 18.5)
                    || (fy >= 15.0 && fy <= 26.0 && fx >= 13.5 && fx <= (14.0 + (26.0 - fy) * 0.6));
            if in_bolt {
                rgba.extend_from_slice(&[BOLT_RGB[0], BOLT_RGB[1], BOLT_RGB[2], alpha]);
            } else {
                // Gentle top-to-bottom shade for a slightly glossy look.
                let t = fy / SIZE as f32;
                let shade =
                    |c: u8| -> u8 { (c as f32 * (1.12 - 0.28 * t)).clamp(0.0, 255.0) as u8 };
                rgba.extend_from_slice(&[shade(rgb[0]), shade(rgb[1]), shade(rgb[2]), alpha]);
            }
        }
    }
    rgba
}

#[cfg(not(windows))]
fn status_icon(rgb: [u8; 3]) -> Option<Icon> {
    Icon::from_rgba(status_icon_rgba(rgb), 32, 32).ok()
}

// ---------------------------------------------------------------------------
// Process launching helpers
// ---------------------------------------------------------------------------

fn spawn_aikd(cfg: &TrayConfig, args: &[&str]) -> Option<std::process::Child> {
    let mut cmd = std::process::Command::new(aikd_exe());
    cmd.arg("--config")
        .arg(&cfg.config_path)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().ok()
}

fn daemon_healthy(cfg: &TrayConfig) -> bool {
    http_call(cfg, "GET", "/api/health", None)
        .map(|(status, _)| status == 200)
        .unwrap_or(false)
}

/// Auto-start on tray startup: only when the daemon does not answer yet.
fn ensure_daemon(cfg: TrayConfig) {
    std::thread::spawn(move || {
        if daemon_healthy(&cfg) {
            return;
        }
        // `aikd daemon` itself waits for the REST port and detaches; no need to wait.
        let _ = spawn_aikd(&cfg, &["daemon"]);
    });
}

fn restart_daemon(cfg: TrayConfig) {
    std::thread::spawn(move || {
        if let Some(mut child) = spawn_aikd(&cfg, &["daemon-stop"]) {
            let _ = child.wait();
        }
        // Wait for the old listener to release the port, otherwise `aikd daemon`
        // short-circuits with "already running" and the tray ends up with none.
        for _ in 0..50 {
            if !daemon_healthy(&cfg) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = spawn_aikd(&cfg, &["daemon"]);
    });
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(windows)]
fn focus_existing_studio_window() -> bool {
    use std::ffi::c_void;

    extern "system" {
        fn EnumWindows(
            cb: unsafe extern "system" fn(*mut c_void, isize) -> i32,
            lparam: isize,
        ) -> i32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn IsIconic(hwnd: *mut c_void) -> i32;
        fn GetWindowTextW(hwnd: *mut c_void, buf: *mut u16, max: i32) -> i32;
        fn ShowWindow(hwnd: *mut c_void, cmd: i32) -> i32;
        fn SetForegroundWindow(hwnd: *mut c_void) -> i32;
    }
    const SW_RESTORE: i32 = 9;
    const SW_SHOW: i32 = 5;

    unsafe extern "system" fn enum_cb(hwnd: *mut c_void, lparam: isize) -> i32 {
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            if title.contains("AIKD Studio") {
                let target = lparam as *mut *mut c_void;
                *target = hwnd;
                return 0; // found
            }
        }
        1
    }

    unsafe {
        let mut found: *mut c_void = std::ptr::null_mut();
        EnumWindows(enum_cb, (&mut found as *mut *mut c_void) as isize);
        if !found.is_null() {
            if IsIconic(found) != 0 {
                ShowWindow(found, SW_RESTORE);
            } else {
                ShowWindow(found, SW_SHOW);
            }
            SetForegroundWindow(found);
            return true;
        }
    }
    false
}

/// Opens AIKD Studio in App mode. If an AIKD Studio window is already open,
/// focuses it instead of spawning duplicates.
fn open_ui(port: u16, capture: bool) {
    let now = now_ms();
    let last = LAST_OPEN_MS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < 800 {
        return;
    }
    LAST_OPEN_MS.store(now, Ordering::Relaxed);

    #[cfg(windows)]
    if !capture && focus_existing_studio_window() {
        return;
    }

    let mut url = format!("http://127.0.0.1:{port}/ui");
    if capture {
        url.push_str("?capture=1");
    }

    #[cfg(windows)]
    {
        // App Paths is only consulted by ShellExecute, so probe install dirs too.
        for exe in browser_candidates() {
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg(format!("--app={url}"));
            if capture {
                cmd.arg("--window-size=680,780");
            }
            if cmd.spawn().is_ok() {
                return;
            }
        }
        // Last resort: hand the URL to the default browser.
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "start", "", &url]);
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
        let _ = cmd.spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }
}

#[cfg(windows)]
fn browser_candidates() -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    let mut push = |base: Option<std::ffi::OsString>, rest: &str| {
        if let Some(base) = base {
            let path = std::path::PathBuf::from(base).join(rest);
            if path.is_file() {
                out.push(path);
            }
        }
    };
    let pf = std::env::var_os("ProgramFiles");
    let pf86 = std::env::var_os("ProgramFiles(x86)");
    let local = std::env::var_os("LOCALAPPDATA");

    // Edge first (contract), then Chrome.
    push(pf86.clone(), r"Microsoft\Edge\Application\msedge.exe");
    push(pf.clone(), r"Microsoft\Edge\Application\msedge.exe");
    push(local.clone(), r"Microsoft\Edge\Application\msedge.exe");
    push(pf86, r"Google\Chrome\Application\chrome.exe");
    push(pf, r"Google\Chrome\Application\chrome.exe");
    push(local, r"Google\Chrome\Application\chrome.exe");

    // Bare names still work when the browser is on PATH.
    out.push(std::path::PathBuf::from("msedge"));
    out.push(std::path::PathBuf::from("chrome"));
    out
}

// ---------------------------------------------------------------------------
// Win32 single-instance guard
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn acquire_single_instance() -> bool {
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn CreateMutexW(
            attributes: *mut c_void,
            initial_owner: i32,
            name: *const u16,
        ) -> *mut c_void;
        fn GetLastError() -> u32;
    }
    const ERROR_ALREADY_EXISTS: u32 = 183;

    let name: Vec<u16> = OsStr::new(r"Local\AIKD_TRAY_SINGLE_INSTANCE")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // The returned handle is intentionally kept open for the process lifetime
    // (dropping it would release the mutex and allow a second tray instance).
    unsafe {
        let handle = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
        if handle.is_null() {
            return true; // Could not create the mutex: never block the user.
        }
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
fn acquire_single_instance() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Global hotkey Ctrl+Alt+N (Windows): dedicated message-loop thread
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn spawn_hotkey_thread(_port: u16) {}
fn spawn_hotkey_thread(_port: u16) {}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn read_clipboard() -> Option<String> {
    use std::ffi::c_void;

    extern "system" {
        fn OpenClipboard(owner: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn GlobalLock(mem: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(mem: *mut c_void) -> i32;
    }
    const CF_UNICODETEXT: u32 = 13;
    const MAX_CHARS: usize = 1_000_000;

    // Another app may hold the clipboard open for a few milliseconds.
    for _ in 0..5 {
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                std::thread::sleep(Duration::from_millis(30));
                continue;
            }
            let mut text = None;
            if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
                let handle = GetClipboardData(CF_UNICODETEXT);
                if !handle.is_null() {
                    let ptr = GlobalLock(handle) as *const u16;
                    if !ptr.is_null() {
                        let mut len = 0usize;
                        while len < MAX_CHARS && *ptr.add(len) != 0 {
                            len += 1;
                        }
                        let slice = std::slice::from_raw_parts(ptr, len);
                        text = Some(String::from_utf16_lossy(slice));
                        GlobalUnlock(handle);
                    }
                }
            }
            CloseClipboard();
            if text.is_some() {
                return text;
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    None
}

#[cfg(not(windows))]
fn read_clipboard() -> Option<String> {
    for (prog, args) in [
        ("wl-paste", &["-n"][..]),
        ("xclip", &["-selection", "clipboard", "-o"][..]),
        ("xsel", &["-b"][..]),
    ] {
        if let Ok(out) = std::process::Command::new(prog).args(args).output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).to_string();
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn truncate(s: &str, n: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= n {
        flat
    } else {
        let head: String = flat.chars().take(n).collect();
        format!("{head}…")
    }
}

/// Reads the clipboard and quick-captures it through `POST /api/send`
/// (falling back to `/api/notes`, the aliased quick-capture route).
fn send_clipboard(cfg: &TrayConfig) -> String {
    let Some(text) = read_clipboard() else {
        return "clipboard is empty".to_string();
    };
    let text: String = text.trim().chars().take(100_000).collect();
    if text.trim().is_empty() {
        return "clipboard is empty".to_string();
    }
    let title: String = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Clipboard capture")
        .chars()
        .take(80)
        .collect();
    let payload = serde_json::json!({
        "title": title,
        "body": text,
        "kind": "note",
        "scope": "project",
        "folder": "Inbox",
        "tags": ["clipboard"],
        "source": "clipboard",
        "source_url": "",
    })
    .to_string();

    let Some((status, body)) = http_call(cfg, "POST", "/api/send", Some(&payload)) else {
        return "daemon offline - clipboard not saved".to_string();
    };
    let (status, body) = if status == 404 || status == 405 {
        http_call(cfg, "POST", "/api/notes", Some(&payload)).unwrap_or((status, body))
    } else {
        (status, body)
    };

    if (200..300).contains(&status) {
        let redacted = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                let data = v.get("data").cloned().unwrap_or(v);
                data.get("redacted_count").and_then(|r| r.as_u64())
            })
            .unwrap_or(0);
        format!(
            "clipboard sent ({} chars, {} secrets redacted)",
            text.chars().count(),
            redacted
        )
    } else {
        format!("clipboard send failed (HTTP {status})")
    }
}

// ---------------------------------------------------------------------------
// Windows SendTo shortcut ("Send to AIKD.cmd")
// ---------------------------------------------------------------------------

fn install_sendto_shortcut() -> String {
    #[cfg(windows)]
    {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return "APPDATA is not set".to_string();
        };
        let dir = std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("SendTo");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return format!("SendTo install failed: {e}");
        }
        let path = dir.join("Send to AIKD.cmd");
        let content = format!(
            "@echo off\r\n\"{}\" send %*\r\n",
            aikd_exe().to_string_lossy()
        );
        match std::fs::write(&path, content) {
            Ok(_) => format!("SendTo shortcut installed ({})", path.display()),
            Err(e) => format!("SendTo install failed: {e}"),
        }
    }
    #[cfg(not(windows))]
    {
        "SendTo shortcut is Windows-only".to_string()
    }
}

// ---------------------------------------------------------------------------
// Native Win32 Tray (Windows) — sets cbSize properly on NOTIFYICONDATAW
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win32_tray {
    use super::*;
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::sync::{Arc, Mutex};

    const TRAY_UID: u32 = 1;
    const WM_TRAYICON: u32 = 0x8001; // WM_APP + 1
    const WM_STATUS_UPDATE: u32 = 0x8002; // WM_APP + 2
    const WM_DESTROY: u32 = 0x0002;
    const WM_COMMAND: u32 = 0x0111;
    const WM_HOTKEY: u32 = 0x0312;
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_LBUTTONDBLCLK: u32 = 0x0203;
    const WM_RBUTTONUP: u32 = 0x0205;

    const NIM_ADD: u32 = 0x0000_0000;
    const NIM_MODIFY: u32 = 0x0000_0001;
    const NIM_DELETE: u32 = 0x0000_0002;
    const NIF_MESSAGE: u32 = 0x0000_0001;
    const NIF_ICON: u32 = 0x0000_0002;
    const NIF_TIP: u32 = 0x0000_0004;

    const MF_STRING: u32 = 0x0000_0000;
    const MF_SEPARATOR: u32 = 0x0000_0800;
    const MF_GRAYED: u32 = 0x0000_0001;
    const TPM_BOTTOMALIGN: u32 = 0x0020;
    const TPM_LEFTALIGN: u32 = 0x0000;
    const TPM_RIGHTBUTTON: u32 = 0x0002;

    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_NOREPEAT: u32 = 0x4000;
    const VK_N: u32 = 0x4E;

    const IDM_STATUS: usize = 1000;
    const IDM_OPEN: usize = 1001;
    const IDM_QUICK: usize = 1002;
    const IDM_CLIPBOARD: usize = 1003;
    const IDM_WATCH: usize = 1004;
    const IDM_DAEMON: usize = 1005;
    const IDM_SENDTO: usize = 1006;
    const IDM_QUIT: usize = 1007;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct Msg {
        hwnd: *mut c_void,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt: Point,
        l_private: u32,
    }
    #[repr(C)]
    struct WndClassW {
        style: u32,
        lpfn_wnd_proc: Option<unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize>,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: *mut c_void,
        h_icon: *mut c_void,
        h_cursor: *mut c_void,
        hbr_background: *mut c_void,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
    }
    #[repr(C)]
    struct NotifyIconDataW {
        cb_size: u32,
        h_wnd: *mut c_void,
        u_id: u32,
        u_flags: u32,
        u_callback_message: u32,
        h_icon: *mut c_void,
        sz_tip: [u16; 128],
        dw_state: u32,
        dw_state_mask: u32,
        sz_info: [u16; 256],
        u_timeout_or_version: u32,
        sz_info_title: [u16; 64],
        dw_info_flags: u32,
        guid_item: [u8; 16],
        h_balloon_icon: *mut c_void,
    }
    #[repr(C)]
    struct IconInfo {
        f_icon: i32,
        x_hotspot: u32,
        y_hotspot: u32,
        hbm_mask: *mut c_void,
        hbm_color: *mut c_void,
    }

    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn RegisterClassW(wc: *const WndClassW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class_name: *const u16,
            window_name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: *mut c_void,
            menu: *mut c_void,
            instance: *mut c_void,
            param: *mut c_void,
        ) -> *mut c_void;
        fn DefWindowProcW(hwnd: *mut c_void, msg: u32, wp: usize, lp: isize) -> isize;
        fn PostQuitMessage(code: i32);
        fn PostMessageW(hwnd: *mut c_void, msg: u32, wp: usize, lp: isize) -> i32;
        fn GetMessageW(msg: *mut Msg, hwnd: *mut c_void, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn RegisterWindowMessageW(name: *const u16) -> u32;
        fn Shell_NotifyIconW(message: u32, data: *mut NotifyIconDataW) -> i32;
        fn CreatePopupMenu() -> *mut c_void;
        fn AppendMenuW(menu: *mut c_void, flags: u32, id: usize, text: *const u16) -> i32;
        fn TrackPopupMenu(
            menu: *mut c_void,
            flags: u32,
            x: i32,
            y: i32,
            reserved: i32,
            hwnd: *mut c_void,
            rect: *const c_void,
        ) -> i32;
        fn DestroyMenu(menu: *mut c_void) -> i32;
        fn GetCursorPos(pt: *mut Point) -> i32;
        fn SetForegroundWindow(hwnd: *mut c_void) -> i32;
        fn RegisterHotKey(hwnd: *mut c_void, id: i32, modifiers: u32, vk: u32) -> i32;
        fn UnregisterHotKey(hwnd: *mut c_void, id: i32) -> i32;
        fn GetLastError() -> u32;
        fn CreateBitmap(w: i32, h: i32, planes: u32, bpp: u32, bits: *const c_void) -> *mut c_void;
        fn CreateIconIndirect(info: *const IconInfo) -> *mut c_void;
        fn DestroyIcon(hicon: *mut c_void) -> i32;
        fn DeleteObject(obj: *mut c_void) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    unsafe fn make_hicon(rgb: [u8; 3]) -> *mut c_void {
        let rgba = status_icon_rgba(rgb);
        // Win32 32bpp HBITMAP expects BGRA byte order.
        let mut bgra = Vec::with_capacity(rgba.len());
        for px in rgba.chunks_exact(4) {
            bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        let hbm_color = CreateBitmap(32, 32, 1, 32, bgra.as_ptr() as *const c_void);
        let mask_bits = [0u8; 32 * 4];
        let hbm_mask = CreateBitmap(32, 32, 1, 1, mask_bits.as_ptr() as *const c_void);
        let info = IconInfo {
            f_icon: 1,
            x_hotspot: 0,
            y_hotspot: 0,
            hbm_mask,
            hbm_color,
        };
        let hicon = CreateIconIndirect(&info);
        if !hbm_color.is_null() {
            DeleteObject(hbm_color);
        }
        if !hbm_mask.is_null() {
            DeleteObject(hbm_mask);
        }
        hicon
    }

    struct State {
        cfg: TrayConfig,
        hwnd: *mut c_void,
        hicon: *mut c_void,
        applied_color: Option<[u8; 3]>,
        status_line: String,
        tooltip: String,
        watch_child: Option<std::process::Child>,
        taskbar_msg: u32,
        latest_snap: Arc<Mutex<Option<Snapshot>>>,
    }

    static mut STATE_PTR: *mut State = std::ptr::null_mut();

    unsafe fn register_or_modify(st: &State, cmd: u32) -> i32 {
        let mut nid: NotifyIconDataW = std::mem::zeroed();
        nid.cb_size = std::mem::size_of::<NotifyIconDataW>() as u32;
        nid.h_wnd = st.hwnd;
        nid.u_id = TRAY_UID;
        nid.u_flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.u_callback_message = WM_TRAYICON;
        nid.h_icon = st.hicon;
        let tip = wide(&st.tooltip);
        for i in 0..tip.len().min(127) {
            nid.sz_tip[i] = tip[i];
        }
        let _ = GetLastError();
        Shell_NotifyIconW(cmd, &mut nid)
    }

    unsafe fn show_menu(st: &mut State) {
        let watch_running = match st.watch_child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        };
        if !watch_running {
            st.watch_child = None;
        }
        let hmenu = CreatePopupMenu();
        if hmenu.is_null() {
            return;
        }
        let s_status = wide(&st.status_line);
        let s_open = wide("Open AIKD Studio");
        let s_quick = wide("Quick Send to AIKD (Ctrl+Alt+N)");
        let s_clip = wide("Send Clipboard to AIKD");
        let s_watch = wide(if watch_running {
            "Toggle Auto-Watch (ON)"
        } else {
            "Toggle Auto-Watch (off)"
        });
        let s_daemon = wide("Start / Restart Daemon");
        let s_sendto = wide("Install Windows SendTo Shortcut");
        let s_quit = wide("Quit Tray (daemon keeps running)");

        AppendMenuW(hmenu, MF_STRING | MF_GRAYED, IDM_STATUS, s_status.as_ptr());
        AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(hmenu, MF_STRING, IDM_OPEN, s_open.as_ptr());
        AppendMenuW(hmenu, MF_STRING, IDM_QUICK, s_quick.as_ptr());
        AppendMenuW(hmenu, MF_STRING, IDM_CLIPBOARD, s_clip.as_ptr());
        AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(hmenu, MF_STRING, IDM_WATCH, s_watch.as_ptr());
        AppendMenuW(hmenu, MF_STRING, IDM_DAEMON, s_daemon.as_ptr());
        AppendMenuW(hmenu, MF_STRING, IDM_SENDTO, s_sendto.as_ptr());
        AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(hmenu, MF_STRING, IDM_QUIT, s_quit.as_ptr());

        let mut pt = Point { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(st.hwnd);
        TrackPopupMenu(
            hmenu,
            TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            st.hwnd,
            std::ptr::null(),
        );
        DestroyMenu(hmenu);
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: *mut c_void,
        msg: u32,
        wp: usize,
        lp: isize,
    ) -> isize {
        let st_ptr = STATE_PTR;
        if !st_ptr.is_null() {
            let st = &mut *st_ptr;
            if msg == st.taskbar_msg && st.taskbar_msg != 0 {
                register_or_modify(st, NIM_ADD);
                return 0;
            }
            match msg {
                WM_TRAYICON => {
                    let ev = (lp as u32) & 0xFFFF;
                    if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                        open_ui(st.cfg.port, false);
                    } else if ev == WM_RBUTTONUP {
                        show_menu(st);
                    }
                    return 0;
                }
                WM_STATUS_UPDATE => {
                    let snap = {
                        st.latest_snap
                            .lock()
                            .ok()
                            .and_then(|mut g| g.take())
                    };
                    if let Some(snap) = snap {
                        let color = status_color(&snap);
                        if st.applied_color != Some(color) {
                            let new_icon = make_hicon(color);
                            if !new_icon.is_null() {
                                if !st.hicon.is_null() {
                                    DestroyIcon(st.hicon);
                                }
                                st.hicon = new_icon;
                                st.applied_color = Some(color);
                            }
                        }
                        st.status_line = status_text(&snap);
                        st.tooltip = tooltip_text(&snap);
                        register_or_modify(st, NIM_MODIFY);
                    }
                    return 0;
                }
                WM_HOTKEY => {
                    open_ui(st.cfg.port, true);
                    return 0;
                }
                WM_COMMAND => {
                    match wp & 0xFFFF {
                        IDM_OPEN => open_ui(st.cfg.port, false),
                        IDM_QUICK => open_ui(st.cfg.port, true),
                        IDM_CLIPBOARD => {
                            let m = send_clipboard(&st.cfg);
                            st.status_line = format!("AIKD - {}", truncate(&m, 70));
                        }
                        IDM_WATCH => {
                            let alive = st
                                .watch_child
                                .as_mut()
                                .map(|c| matches!(c.try_wait(), Ok(None)))
                                .unwrap_or(false);
                            if alive {
                                if let Some(mut c) = st.watch_child.take() {
                                    let _ = c.kill();
                                    let _ = c.wait();
                                }
                            } else if let Some(c) = spawn_aikd(&st.cfg, &["watch"]) {
                                st.watch_child = Some(c);
                            }
                        }
                        IDM_DAEMON => {
                            st.status_line = "AIKD - starting daemon…".to_string();
                            restart_daemon(st.cfg.clone());
                        }
                        IDM_SENDTO => {
                            let m = install_sendto_shortcut();
                            st.status_line = format!("AIKD - {}", truncate(&m, 70));
                        }
                        IDM_QUIT => {
                            if let Some(mut c) = st.watch_child.take() {
                                let _ = c.kill();
                                let _ = c.wait();
                            }
                            let mut nid: NotifyIconDataW = std::mem::zeroed();
                            nid.cb_size = std::mem::size_of::<NotifyIconDataW>() as u32;
                            nid.h_wnd = st.hwnd;
                            nid.u_id = TRAY_UID;
                            Shell_NotifyIconW(NIM_DELETE, &mut nid);
                            UnregisterHotKey(st.hwnd, 1);
                            PostQuitMessage(0);
                        }
                        _ => {}
                    }
                    return 0;
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    return 0;
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    pub fn run() {
        unsafe {
            let cfg = TrayConfig::load();
            let hinst = GetModuleHandleW(std::ptr::null());
            let cls = wide("AIKD_NATIVE_TRAY_WND");
            let wc = WndClassW {
                style: 0,
                lpfn_wnd_proc: Some(wnd_proc),
                cb_cls_extra: 0,
                cb_wnd_extra: 0,
                h_instance: hinst,
                h_icon: std::ptr::null_mut(),
                h_cursor: std::ptr::null_mut(),
                hbr_background: std::ptr::null_mut(),
                lpsz_menu_name: std::ptr::null(),
                lpsz_class_name: cls.as_ptr(),
            };
            RegisterClassW(&wc);

            let title = wide("AIKD Tray");
            let hwnd = CreateWindowExW(
                0,
                cls.as_ptr(),
                title.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null_mut(),
            );
            if hwnd.is_null() {
                return;
            }

            let latest_snap = Arc::new(Mutex::new(None));
            let initial_icon = make_hicon(COLOR_RED);
            let tb_msg_name = wide("TaskbarCreated");
            let mut st = Box::new(State {
                cfg: cfg.clone(),
                hwnd,
                hicon: initial_icon,
                applied_color: Some(COLOR_RED),
                status_line: "AIKD - starting…".to_string(),
                tooltip: "AIKD - starting…".to_string(),
                watch_child: None,
                taskbar_msg: RegisterWindowMessageW(tb_msg_name.as_ptr()),
                latest_snap: Arc::clone(&latest_snap),
            });
            STATE_PTR = &mut *st;
            register_or_modify(&st, NIM_ADD);
            RegisterHotKey(hwnd, 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_N);

            ensure_daemon(cfg.clone());

            let hwnd_val = hwnd as usize;
            std::thread::spawn(move || loop {
                let snap = fetch_snapshot(&cfg);
                if let Ok(mut g) = latest_snap.lock() {
                    *g = Some(snap);
                }
                PostMessageW(hwnd_val as *mut c_void, WM_STATUS_UPDATE, 0, 0);
                std::thread::sleep(POLL_INTERVAL);
            });

            let mut msg: Msg = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            STATE_PTR = std::ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// Non-Windows Winit/tray-icon shell
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
mod unix_tray {
    use super::*;

    enum AppEvent {
        Status(Box<Snapshot>),
        Menu(String),
        OpenStudio,
    }

    fn wake(bus: &Arc<Mutex<EventLoopProxy<AppEvent>>>, event: AppEvent) {
        let proxy = bus.lock().unwrap_or_else(|e| e.into_inner());
        let _ = proxy.send_event(event);
    }

    struct TrayApp {
        cfg: TrayConfig,
        tray: TrayIcon,
        status_item: MenuItem,
        watch_item: MenuItem,
        watch_child: Option<std::process::Child>,
        applied_color: Option<[u8; 3]>,
    }

    impl TrayApp {
        fn apply_status(&mut self, snap: Snapshot) {
            let color = status_color(&snap);
            if self.applied_color != Some(color) {
                if let Some(icon) = status_icon(color) {
                    let _ = self.tray.set_icon(Some(icon));
                    self.applied_color = Some(color);
                }
            }
            let _ = self.tray.set_tooltip(Some(tooltip_text(&snap)));
            self.status_item.set_text(status_text(&snap));
        }

        fn watch_running(&mut self) -> bool {
            let exited = match self.watch_child.as_mut() {
                Some(child) => !matches!(child.try_wait(), Ok(None)),
                None => true,
            };
            if exited {
                self.watch_child = None;
            }
            !exited
        }

        fn toggle_watch(&mut self) {
            if self.watch_running() {
                if let Some(mut child) = self.watch_child.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                self.watch_item.set_text("Toggle Auto-Watch (off)");
                return;
            }
            match spawn_aikd(&self.cfg, &["watch"]) {
                Some(child) => {
                    self.watch_child = Some(child);
                    self.watch_item.set_text("Toggle Auto-Watch (ON)");
                }
                None => self
                    .status_item
                    .set_text("AIKD - failed to start the auto-watcher"),
            }
        }

        fn handle_menu(&mut self, event_loop: &ActiveEventLoop, id: &str) {
            match id {
                "open" => open_ui(self.cfg.port, false),
                "quick" => open_ui(self.cfg.port, true),
                "clipboard" => {
                    let message = send_clipboard(&self.cfg);
                    self.status_item
                        .set_text(format!("AIKD - {}", truncate(&message, 70)));
                }
                "watch" => self.toggle_watch(),
                "daemon" => {
                    self.status_item.set_text("AIKD - starting daemon…");
                    restart_daemon(self.cfg.clone());
                }
                "sendto" => {
                    let message = install_sendto_shortcut();
                    self.status_item
                        .set_text(format!("AIKD - {}", truncate(&message, 70)));
                }
                "quit" => {
                    if let Some(mut child) = self.watch_child.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    event_loop.exit();
                }
                _ => {}
            }
        }
    }

    impl ApplicationHandler<AppEvent> for TrayApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window_id: winit::window::WindowId,
            _event: winit::event::WindowEvent,
        ) {
        }
        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
            match event {
                AppEvent::Status(snap) => self.apply_status(*snap),
                AppEvent::Menu(id) => self.handle_menu(event_loop, &id),
                AppEvent::OpenStudio => open_ui(self.cfg.port, false),
            }
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    pub fn run() {
        let cfg = TrayConfig::load();
        let event_loop = match EventLoop::<AppEvent>::with_user_event().build() {
            Ok(event_loop) => event_loop,
            Err(_) => return,
        };
        let proxy = event_loop.create_proxy();
        let bus = Arc::new(Mutex::new(proxy));

        MenuEvent::set_event_handler(Some({
            let bus = Arc::clone(&bus);
            move |event: MenuEvent| wake(&bus, AppEvent::Menu(event.id.0))
        }));
        TrayIconEvent::set_event_handler(Some({
            let bus = Arc::clone(&bus);
            move |event: TrayIconEvent| match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
                | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => wake(&bus, AppEvent::OpenStudio),
                _ => {}
            }
        }));

        let menu = Menu::new();
        let status_item = MenuItem::with_id("status", "AIKD - starting…", false, None);
        let open_item = MenuItem::with_id("open", "Open AIKD Studio", true, None);
        let quick_item = MenuItem::with_id("quick", "Quick Send to AIKD (Ctrl+Alt+N)", true, None);
        let clip_item = MenuItem::with_id("clipboard", "Send Clipboard to AIKD", true, None);
        let watch_item = MenuItem::with_id("watch", "Toggle Auto-Watch (off)", true, None);
        let daemon_item = MenuItem::with_id("daemon", "Start / Restart Daemon", true, None);
        let sendto_item = MenuItem::with_id("sendto", "Install Windows SendTo Shortcut", false, None);
        let quit_item = MenuItem::with_id("quit", "Quit Tray (daemon keeps running)", true, None);
        let sep1 = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();
        let sep3 = PredefinedMenuItem::separator();
        let _ = menu.append_items(&[
            &status_item, &sep1, &open_item, &quick_item, &clip_item, &sep2, &watch_item,
            &daemon_item, &sendto_item, &sep3, &quit_item,
        ]);

        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("AIKD - starting…");
        if let Some(icon) = status_icon(COLOR_RED) {
            builder = builder.with_icon(icon);
        }
        let tray = match builder.build() {
            Ok(tray) => tray,
            Err(_) => return,
        };

        ensure_daemon(cfg.clone());
        {
            let cfg = cfg.clone();
            let bus = Arc::clone(&bus);
            std::thread::spawn(move || loop {
                let snapshot = fetch_snapshot(&cfg);
                wake(&bus, AppEvent::Status(Box::new(snapshot)));
                std::thread::sleep(POLL_INTERVAL);
            });
        }

        let mut app = TrayApp {
            cfg,
            tray,
            status_item,
            watch_item,
            watch_child: None,
            applied_color: Some(COLOR_RED),
        };
        let _ = event_loop.run_app(&mut app);
    }
}

fn main() {
    if !acquire_single_instance() {
        return; // Another tray is already running: exit quietly.
    }
    #[cfg(windows)]
    win32_tray::run();
    #[cfg(not(windows))]
    unix_tray::run();
}
