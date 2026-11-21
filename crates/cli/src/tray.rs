//! AIKD system tray companion (§C).
//!
//! Thin remote for the ONE daemon: shows status, starts/stops the daemon
//! via the existing CLI (`aikd daemon` / `aikd daemon-stop`), opens the
//! live `/ui` dashboard, runs `aikd doctor`. It never indexes anything
//! itself — all work happens in `aikd` + the daemon REST API.
//!
//! Build with: `cargo build --features tray --bin aikd-tray`
//! (default builds stay GUI-free).

use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use std::net::TcpStream;
use std::time::Duration;
use tray_icon::{TrayIconBuilder, TrayIconEvent};
use winit::event_loop::{ControlFlow, EventLoop};

const REST_HOST: &str = "127.0.0.1";
const REST_PORT: u16 = 9090;

fn aikd_exe() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("aikd"))
}

fn daemon_running() -> bool {
    // PID file liveness + TCP probe: both must agree.
    let pid_path = shellexpand::tilde("~/.aikd/aikd.pid").to_string();
    let pid_ok = std::fs::read_to_string(pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|pid| {
            #[cfg(windows)]
            {
                let _ = pid;
                true // TCP probe below is authoritative on Windows.
            }
            #[cfg(unix)]
            {
                std::process::Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }
        })
        .unwrap_or(false);
    let tcp_ok =
        TcpStream::connect_timeout(&format!("{REST_HOST}:{REST_PORT}").parse().unwrap(), Duration::from_millis(400)).is_ok();
    pid_ok && tcp_ok
}

fn run_aikd(args: &[&str]) -> String {
    std::process::Command::new(aikd_exe())
        .args(args)
        .output()
        .map(|o| {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s.trim().to_string()
        })
        .unwrap_or_else(|e| format!("failed to run aikd: {e}"))
}

fn open_ui() {
    let url = format!("http://{REST_HOST}:{REST_PORT}/ui");
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd").args(["/C", "start", "", &url]).status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).status();
    }
}

fn load_icon() -> Option<tray_icon::Icon> {
    // Prefer the repo logo next to the exe; fall back to a 1x1 icon.
    let candidates = [
        aikd_exe().parent().map(|p| p.join("logo.png")),
        Some(std::path::PathBuf::from("assets/logo.png")),
    ];
    for c in candidates.into_iter().flatten() {
        if let Ok(bytes) = std::fs::read(&c) {
            if let Ok(img) = decode_png_rgba(&bytes) {
                if let Ok(icon) = tray_icon::Icon::from_rgba(img.0, img.1, img.2) {
                    return Some(icon);
                }
            }
        }
    }
    tray_icon::Icon::from_rgba(vec![255, 255, 255, 255], 1, 1).ok()
}

/// Minimal PNG decoder for 8-bit RGBA logos (no new dependencies).
fn decode_png_rgba(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    // Tiny hand-rolled reader: signature + IHDR only; full IDAT inflate is
    // out of scope — return Err so callers use the fallback icon.
    if bytes.len() < 33 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("not png".into());
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().map_err(|_| "bad ihdr")?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().map_err(|_| "bad ihdr")?);
    if w == 0 || h == 0 || w > 256 || h > 256 {
        return Err("bad size".into());
    }
    Err("full decode skipped".into())
}

fn main() {
    let event_loop = EventLoop::new().expect("tray event loop");
    let menu = Menu::new();
    let status_item = MenuItem::with_id("status", "Status: …", false, None);
    let open_item = MenuItem::with_id("open", "Open dashboard (/ui)", true, None);
    let start_item = MenuItem::with_id("start", "Start daemon", true, None);
    let stop_item = MenuItem::with_id("stop", "Stop daemon", true, None);
    let doctor_item = MenuItem::with_id("doctor", "Run doctor (terminal)", true, None);
    let quit_item = MenuItem::with_id("quit", "Quit tray", true, None);
    let _ = menu.append_items(&[
        &status_item,
        &PredefinedMenuItem::separator(),
        &open_item,
        &start_item,
        &stop_item,
        &doctor_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ]);

    let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("AIKD");
    if let Some(icon) = load_icon() {
        builder = builder.with_icon(icon);
    }
    let _tray = builder.build().expect("tray icon");

    let menu_rx = MenuEvent::receiver();
    let tray_rx = TrayIconEvent::receiver();

    // Refresh status line every 5s without blocking menu events.
    status_item.set_text(format!("Status: {}", if daemon_running() { "running" } else { "stopped" }));
    let mut last = std::time::Instant::now();

    #[allow(deprecated)]
    let _ = event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::WaitUntil(last + Duration::from_secs(5)));
        if last.elapsed() >= Duration::from_secs(5) {
            last = std::time::Instant::now();
            status_item.set_text(format!("Status: {}", if daemon_running() { "running" } else { "stopped" }));
        }
        if let winit::event::Event::NewEvents(winit::event::StartCause::ResumeTimeReached { .. }) = event {
            elwt.set_control_flow(ControlFlow::WaitUntil(last + Duration::from_secs(5)));
        }
        while let Ok(ev) = menu_rx.try_recv() {
            match ev.id.0.as_str() {
                "open" => open_ui(),
                "start" => {
                    let out = run_aikd(&["daemon"]);
                    status_item.set_text(format!("Status: {} ({})", if daemon_running() { "running" } else { "start?" }, truncate(&out, 40)));
                }
                "stop" => {
                    let out = run_aikd(&["daemon-stop"]);
                    status_item.set_text(format!("Status: stopped ({})", truncate(&out, 40)));
                }
                "doctor" => {
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("cmd").args(["/C", "start", "", &aikd_exe().to_string_lossy().to_string(), "doctor"]).status();
                    }
                }
                "quit" => elwt.exit(),
                _ => {}
            }
        }
        while tray_rx.try_recv().is_ok() {}
    });
}

fn truncate(s: &str, n: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.len() <= n {
        flat
    } else {
        format!("{}…", &flat[..n])
    }
}


