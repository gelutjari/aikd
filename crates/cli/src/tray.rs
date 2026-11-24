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
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
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
fn status_icon(rgb: [u8; 3]) -> Option<Icon> {
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
    Icon::from_rgba(rgba, SIZE, SIZE).ok()
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

/// Opens AIKD Studio in App mode. Throttled so a left click that is followed by
/// a double-click (or a held hotkey) does not open several windows.
fn open_ui(port: u16, capture: bool) {
    let now = now_ms();
    let last = LAST_OPEN_MS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < 800 {
        return;
    }
    LAST_OPEN_MS.store(now, Ordering::Relaxed);

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

#[cfg(windows)]
fn spawn_hotkey_thread(port: u16) {
    std::thread::spawn(move || unsafe {
        #[repr(C)]
        struct Point {
            x: i32,
            y: i32,
        }
        #[repr(C)]
        struct Msg {
            hwnd: *mut std::ffi::c_void,
            message: u32,
            w_param: usize,
            l_param: isize,
            time: u32,
            pt: Point,
            l_private: u32,
        }

        extern "system" {
            fn RegisterHotKey(hwnd: *mut std::ffi::c_void, id: i32, modifiers: u32, vk: u32)
                -> i32;
            fn GetMessageW(
                msg: *mut Msg,
                hwnd: *mut std::ffi::c_void,
                filter_min: u32,
                filter_max: u32,
            ) -> i32;
        }

        const MOD_ALT: u32 = 0x0001;
        const MOD_CONTROL: u32 = 0x0002;
        const MOD_NOREPEAT: u32 = 0x4000;
        const VK_N: u32 = 0x4E;
        const WM_HOTKEY: u32 = 0x0312;

        if RegisterHotKey(
            std::ptr::null_mut(),
            1,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            VK_N,
        ) == 0
        {
            // Combination already owned by another app: tray keeps working.
            return;
        }
        let mut msg: Msg = std::mem::zeroed();
        loop {
            if GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) <= 0 {
                break;
            }
            if msg.message == WM_HOTKEY {
                open_ui(port, true);
            }
        }
    });
}

#[cfg(not(windows))]
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
// Winit application shell
// ---------------------------------------------------------------------------

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

    /// Returns true while the auto-watch child is alive; reaps a dead one.
    fn watch_running(&mut self) -> bool {
        let exited = match self.watch_child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => false,
                _ => true,
            },
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
            // "status" is a disabled header; anything unknown is ignored.
            "quit" => {
                // Quit the tray only: the daemon keeps serving running agents.
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
        // The tray owns no windows; the event loop only routes user events.
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Status(snap) => self.apply_status(*snap),
            AppEvent::Menu(id) => self.handle_menu(event_loop, &id),
            AppEvent::OpenStudio => open_ui(self.cfg.port, false),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Everything is driven by events (poll thread / menu / hotkey).
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

fn build_menu() -> (Menu, MenuItem, MenuItem) {
    let menu = Menu::new();
    let status_item = MenuItem::with_id("status", "AIKD - starting…", false, None);
    let open_item = MenuItem::with_id("open", "Open AIKD Studio", true, None);
    let quick_item = MenuItem::with_id("quick", "Quick Send to AIKD (Ctrl+Alt+N)", true, None);
    let clip_item = MenuItem::with_id("clipboard", "Send Clipboard to AIKD", true, None);
    let watch_item = MenuItem::with_id("watch", "Toggle Auto-Watch (off)", true, None);
    let daemon_item = MenuItem::with_id("daemon", "Start / Restart Daemon", true, None);
    let sendto_item = MenuItem::with_id(
        "sendto",
        "Install Windows SendTo Shortcut",
        cfg!(windows),
        None,
    );
    let quit_item = MenuItem::with_id("quit", "Quit Tray (daemon keeps running)", true, None);

    let sep1 = PredefinedMenuItem::separator();
    let sep2 = PredefinedMenuItem::separator();
    let sep3 = PredefinedMenuItem::separator();

    let _ = menu.append_items(&[
        &status_item,
        &sep1,
        &open_item,
        &quick_item,
        &clip_item,
        &sep2,
        &watch_item,
        &daemon_item,
        &sendto_item,
        &sep3,
        &quit_item,
    ]);
    (menu, status_item, watch_item)
}

fn main() {
    if !acquire_single_instance() {
        return; // Another tray is already running: exit quietly.
    }

    let cfg = TrayConfig::load();
    let port = cfg.port;

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

    let (menu, status_item, watch_item) = build_menu();
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
    spawn_hotkey_thread(port);

    // Status poller: one HTTP round-trip every ~8s, delivered as a user event.
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
