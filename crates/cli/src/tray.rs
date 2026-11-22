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
    let tcp_ok = TcpStream::connect_timeout(
        &format!("{REST_HOST}:{REST_PORT}").parse().unwrap(),
        Duration::from_millis(400),
    )
    .is_ok();
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
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).status();
    }
}

fn load_icon() -> Option<tray_icon::Icon> {
    // Generate a crisp 32x32 emerald-cyan gradient icon programmatically
    // (zero external image crate dependency).
    let w = 32u32;
    let h = 32u32;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 - 15.5).abs();
            let dy = (y as f32 - 15.5).abs();
            if dx > 14.5
                || dy > 14.5
                || (dx > 12.0 && dy > 12.0 && (dx - 12.0).hypot(dy - 12.0) > 2.5)
            {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                // Lightning bolt center shape in dark navy, emerald-teal background
                let fx = x as f32;
                let fy = y as f32;
                let in_bolt = (fy >= 6.0
                    && fy <= 17.0
                    && fx >= (18.0 - (fy - 6.0) * 0.6)
                    && fx <= 18.5)
                    || (fy >= 15.0 && fy <= 26.0 && fx >= 13.5 && fx <= (14.0 + (26.0 - fy) * 0.6));
                if in_bolt {
                    rgba.extend_from_slice(&[9, 13, 22, 255]);
                } else {
                    let r = 16u8;
                    let g = (185u32.saturating_sub(y * 2)) as u8;
                    let b = (129u32 + x * 3).min(235) as u8;
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, w, h).ok()
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

    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("AIKD");
    if let Some(icon) = load_icon() {
        builder = builder.with_icon(icon);
    }
    let _tray = builder.build().expect("tray icon");

    let menu_rx = MenuEvent::receiver();
    let tray_rx = TrayIconEvent::receiver();

    // Refresh status line every 5s without blocking menu events.
    status_item.set_text(format!(
        "Status: {}",
        if daemon_running() {
            "running"
        } else {
            "stopped"
        }
    ));
    let mut last = std::time::Instant::now();

    #[allow(deprecated)]
    let _ = event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::WaitUntil(last + Duration::from_secs(5)));
        if last.elapsed() >= Duration::from_secs(5) {
            last = std::time::Instant::now();
            status_item.set_text(format!(
                "Status: {}",
                if daemon_running() {
                    "running"
                } else {
                    "stopped"
                }
            ));
        }
        if let winit::event::Event::NewEvents(winit::event::StartCause::ResumeTimeReached {
            ..
        }) = event
        {
            elwt.set_control_flow(ControlFlow::WaitUntil(last + Duration::from_secs(5)));
        }
        while let Ok(ev) = menu_rx.try_recv() {
            match ev.id.0.as_str() {
                "open" => open_ui(),
                "start" => {
                    let out = run_aikd(&["daemon"]);
                    status_item.set_text(format!(
                        "Status: {} ({})",
                        if daemon_running() {
                            "running"
                        } else {
                            "start?"
                        },
                        truncate(&out, 40)
                    ));
                }
                "stop" => {
                    let out = run_aikd(&["daemon-stop"]);
                    status_item.set_text(format!("Status: stopped ({})", truncate(&out, 40)));
                }
                "doctor" => {
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("cmd")
                            .args([
                                "/C",
                                "start",
                                "",
                                &aikd_exe().to_string_lossy().to_string(),
                                "doctor",
                            ])
                            .status();
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
