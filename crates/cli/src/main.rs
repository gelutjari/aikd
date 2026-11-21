use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use aikd_core::{Config, ResourceProfile, SearchFilters};
use aikd_embedder as embedder;
use aikd_indexer::TantivyEngine;
use aikd_session as session;
use aikd_storage::Database;

struct QueryParams<'a> {
    limit: usize,
    path_filter: Option<&'a str>,
    exclude_path: Option<&'a str>,
    file_type: Option<&'a str>,
    heading_filter: Option<&'a str>,
    json: bool,
    hybrid: bool,
}

static QUIET: AtomicBool = AtomicBool::new(false);

macro_rules! info {
    ($($arg:tt)*) => {
        if !QUIET.load(Ordering::Relaxed) {
            println!($($arg)*);
        }
    };
}

macro_rules! einfo {
    ($($arg:tt)*) => {
        if !QUIET.load(Ordering::Relaxed) {
            eprintln!($($arg)*);
        }
    };
}

#[derive(Parser)]
#[command(name = "aikd")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "AIKD — AI Knowledge Daemon: indexed code search for AI agents")]
#[command(long_about = "AIKD — AI Knowledge Daemon v2.0

3 MODES:

  CLI TOOLS (default) — Panggil langsung dari terminal atau AI agent
    aikd query \"login\" --json
    aikd scan
    aikd stats

  MCP SERVER — Untuk AI assistant yang support MCP protocol
    aikd serve                    # stdio transport
    Config: ~/.aikd/mcp.json

  DAEMON — Background service dengan REST API
    aikd daemon --foreground      # REST API di http://localhost:9090
    aikd watch                    # Auto-sync saat file berubah")]
struct Cli {
    #[arg(short, long, default_value = "~/.aikd/config.yaml")]
    config: String,
    #[arg(long, global = true)]
    json: bool,
    #[arg(short, long, global = true)]
    quiet: bool,
    #[arg(long, global = true)]
    version_json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum DaemonAction {
    #[command(about = "Start in foreground mode")]
    Foreground,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Initialize AIKD for current project")]
    Init {
        #[arg(short, long)]
        path: Option<String>,
    },
    #[command(about = "[MCP] Start MCP server (stdio, for AI assistants)")]
    Serve,
    #[command(about = "[DAEMON] Start background service (REST API + file watcher)")]
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },

    #[command(about = "[DAEMON] Stop running daemon")]
    DaemonStop,

    #[command(about = "[DAEMON] Show daemon PID")]
    DaemonPid,
    #[command(about = "[CLI] Scan and index files")]
    Scan {
        #[arg(short, long)]
        path: Option<String>,
    },
    #[command(about = "[CLI] Search the knowledge base")]
    Query {
        query: String,
        #[arg(short, long, default_value = "10")]
        limit: usize,
        #[arg(short, long)]
        path: Option<String>,
        #[arg(long)]
        exclude_path: Option<String>,
        #[arg(short = 't', long)]
        file_type: Option<String>,
        #[arg(short = 'H', long)]
        heading: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        hybrid: bool,
    },

    #[command(about = "[CLI] Search the knowledge base (alias for query)")]
    Search {
        query: String,
        #[arg(short, long, default_value = "10")]
        limit: usize,
        #[arg(short, long)]
        path: Option<String>,
        #[arg(long)]
        exclude_path: Option<String>,
        #[arg(short = 't', long)]
        file_type: Option<String>,
        #[arg(short = 'H', long)]
        heading: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        hybrid: bool,
    },
    #[command(about = "[CLI] Show index statistics (JSON)")]
    Stats,
    #[command(about = "[CLI] Export chunks to JSON")]
    Export {
        #[arg(short, long, default_value = "~/.aikd/chunks.json")]
        output: String,
    },
    #[command(about = "[CLI] Import embeddings from JSON")]
    Import {
        #[arg(short, long)]
        file: String,
    },
    #[command(about = "[CLI] Generate vector embeddings")]
    Embed {
        #[arg(short, long, default_value = "all-MiniLM-L6-v2")]
        model: String,
        #[arg(short, long, default_value = "64")]
        batch: usize,
    },
    #[command(about = "[DAEMON] Watch files and auto-index on change")]
    Watch {
        #[arg(short, long, default_value = "500")]
        debounce: u64,
    },
    #[command(about = "[CLI] Save conversation to memory")]
    Remember {
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value = "user")]
        role: String,
        #[arg(long)]
        content: String,
    },
    #[command(about = "[CLI] Search conversation memory")]
    Recall {
        query: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    #[command(about = "[CLI] Show system status (JSON)")]
    Status,
    #[command(about = "[CLI] Diagnose health, resources, growth and stale state")]
    Doctor,
    #[command(about = "[CLI] Wrap command with AIKD context injection")]
    Inject {
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },
    #[command(about = "[CLI] Run benchmark suite")]
    Benchmark,

    #[command(about = "[CLI] Manage sessions")]
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },

    #[command(about = "[CLI] Download or manage embedding model")]
    Model {
        #[command(subcommand)]
        action: Option<ModelAction>,
    },

    #[command(about = "[CLI] Database maintenance (status, check, backup, restore, compact)")]
    Db {
        #[command(subcommand)]
        action: DbAction,
    },

    #[command(about = "[CLI] Garbage-collect rows for deleted files and reclaim space")]
    Gc,

    #[command(about = "[CLI] Look up code symbols (functions, structs, classes) by name")]
    Symbol {
        name: String,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    #[command(about = "[ADAPTER] Append or list canonical agent events (event journal)")]
    Event {
        #[command(subcommand)]
        action: EventAction,
    },

    #[command(about = "[HANDOFF] Build the context package for a workstream (resume after compaction/transfer)")]
    Handoff {
        #[arg(long, default_value = "default")]
        workstream: String,
        #[arg(long, default_value = "30")]
        recent_events: usize,
    },

    #[command(about = "[ATLAS] Project intelligence: analyze, show, write artifacts")]
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },

    #[command(about = "[CTX] Inspect what AIKD would send for a query and why (token estimate, exclusions)")]
    Context {
        query: String,
        #[arg(long, default_value = "10")]
        limit: usize,
        #[arg(long)]
        hybrid: bool,
    },

    #[command(about = "[CLI] Structured memory: facts, decisions, constraints, tasks, checkpoints")]
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    #[command(about = "Save a memory item (kind: fact|decision|constraint|discovery|task|checkpoint)")]
    Save {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: String,
        #[arg(long, default_value = "default")]
        workstream: String,
        #[arg(long, default_value = "explicit")]
        source: String,
        #[arg(long, default_value = "1.0")]
        confidence: f64,
    },
    #[command(about = "List memory items with optional kind filter")]
    List {
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value = "default")]
        workstream: String,
        #[arg(long)]
        all: bool,
    },
    #[command(about = "Retire a memory item and record its replacement")]
    Supersede {
        id: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: String,
    },
    #[command(about = "Show active tasks and latest checkpoint for a workstream")]
    Task {
        #[arg(long, default_value = "default")]
        workstream: String,
    },
    #[command(about = "Mark an item STALE (kept as evidence, hidden from active views)")]
    Stale {
        id: String,
    },
    #[command(about = "Archive an item (long-term retention, out of active views)")]
    Archive {
        id: String,
    },
    #[command(about = "Flag two items as CONFLICTED with a note (both preserved, neither becomes fact)")]
    Conflict {
        id_a: String,
        id_b: String,
        #[arg(long)]
        note: String,
    },
}
#[derive(Subcommand)]
enum EventAction {
    #[command(about = "Append one canonical event (DSH-style adapter / batch import)")]
    Append {
        #[arg(long)]
        event_type: String,
        #[arg(long, default_value = "default")]
        workstream: String,
        #[arg(long, default_value = "dsh")]
        agent: String,
        #[arg(long, default_value = "")]
        session: String,
        #[arg(long, default_value = "")]
        source_ref: String,
        #[arg(long, default_value = "{}")]
        payload: String,
    },
    #[command(about = "List journal events (resume with --since)")]
    List {
        #[arg(long, default_value = "default")]
        workstream: String,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long, default_value = "0")]
        since: i64,
        #[arg(long, default_value = "50")]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum ProjectAction {
    #[command(about = "Analyze a project: scan, build Atlas snapshot, print intelligence summary")]
    Analyze {
        #[arg(long)]
        path: Option<String>,
    },
    #[command(about = "Show the stored Atlas snapshot (uses cache when fresh)")]
    Show {
        #[arg(long)]
        path: Option<String>,
    },
    #[command(about = "Write machine + human artifacts (.aikd/intelligence, .aikd/artifacts)")]
    Export {
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value = ".aikd")]
        out: String,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    #[command(about = "List all sessions")]
    List,
    #[command(about = "Create a new session")]
    New {
        #[arg(short, long)]
        name: Option<String>,
    },
    #[command(about = "Delete a session")]
    Delete { id: String },
}

#[derive(Subcommand)]
enum ModelAction {
    #[command(about = "Show model status")]
    Status,
    #[command(about = "Download the embedding model")]
    Download,
    #[command(about = "Remove downloaded model files")]
    Remove,
}

#[derive(Subcommand)]
enum DbAction {
    #[command(about = "Show database health, sizes, and counts")]
    Status,
    #[command(about = "Verify integrity (SQLite + FTS sync + orphans)")]
    Check,
    #[command(about = "Online backup to a file (VACUUM INTO)")]
    Backup {
        #[arg(short, long)]
        output: String,
    },
    #[command(about = "Restore from a backup file (verifies first, atomic replace)")]
    Restore {
        #[arg(short, long)]
        file: String,
    },
    #[command(about = "Reclaim space (VACUUM); optionally rebuild the FTS index")]
    Compact {
        #[arg(long)]
        rebuild_fts: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Logs go to stderr: stdout is reserved for MCP stdio and --json output.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,aikd=info")),
        )
        .init();

    let cli = Cli::parse();

    if cli.version_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "model": embedder::MODEL_NAME,
                "db_schema": 11,
            }))
            .unwrap_or_default()
        );
        return Ok(());
    }

    let json_mode = cli.json;
    if cli.quiet {
        QUIET.store(true, Ordering::Relaxed);
    }
    match cli.command {
        Commands::Init { path } => cmd_init(&cli.config, path.as_deref()),
        Commands::Daemon { action } => {
            let foreground = matches!(action, Some(DaemonAction::Foreground));
            cmd_daemon(&cli.config, foreground).await
        }
        Commands::DaemonStop => cmd_daemon_stop(),
        Commands::DaemonPid => cmd_daemon_pid(),
        Commands::Scan { path } => cmd_scan(&cli.config, path.as_deref()),
        Commands::Query {
            query,
            limit,
            path,
            exclude_path,
            file_type,
            heading,
            json,
            hybrid,
        } => cmd_query(
            &cli.config,
            &query,
            &QueryParams {
                limit,
                path_filter: path.as_deref(),
                exclude_path: exclude_path.as_deref(),
                file_type: file_type.as_deref(),
                heading_filter: heading.as_deref(),
                json: json || json_mode,
                hybrid,
            },
        ),
        Commands::Search {
            query,
            limit,
            path,
            exclude_path,
            file_type,
            heading,
            json,
            hybrid,
        } => cmd_query(
            &cli.config,
            &query,
            &QueryParams {
                limit,
                path_filter: path.as_deref(),
                exclude_path: exclude_path.as_deref(),
                file_type: file_type.as_deref(),
                heading_filter: heading.as_deref(),
                json: json || json_mode,
                hybrid,
            },
        ),
        Commands::Stats => cmd_stats(&cli.config),
        Commands::Export { output } => cmd_export(&cli.config, &output),
        Commands::Import { file } => cmd_import(&cli.config, &file),
        Commands::Embed { model, batch } => cmd_embed(&cli.config, &model, batch),
        Commands::Serve => cmd_serve(&cli.config).await,
        Commands::Watch { debounce } => cmd_watch(&cli.config, debounce).await,
        Commands::Remember {
            session,
            role,
            content,
        } => cmd_remember(&cli.config, session.as_deref(), &role, &content, json_mode),
        Commands::Recall {
            query,
            session,
            limit,
        } => cmd_recall(&cli.config, &query, session.as_deref(), limit, json_mode),
        Commands::Status => cmd_status(&cli.config, json_mode),
        Commands::Doctor => cmd_doctor(&cli.config),
        Commands::Inject { command } => cmd_inject(&cli.config, &command),
        Commands::Benchmark => cmd_benchmark(&cli.config).await,
        Commands::Session { action } => cmd_session(&cli.config, action, json_mode),
        Commands::Model { action } => cmd_model(&cli.config, action.unwrap_or(ModelAction::Status)),
        Commands::Db { action } => cmd_db(&cli.config, action, json_mode),
        Commands::Gc => cmd_gc(&cli.config, json_mode),
        Commands::Symbol { name, limit } => cmd_symbol(&cli.config, &name, limit, json_mode),
        Commands::Memory { action } => cmd_memory(&cli.config, action, json_mode),
        Commands::Event { action } => cmd_event(&cli.config, action, json_mode),
        Commands::Handoff { workstream, recent_events } => cmd_handoff(&cli.config, &workstream, recent_events, json_mode),
        Commands::Project { action } => cmd_project(&cli.config, action, json_mode),
        Commands::Context { query, limit, hybrid } => cmd_context_inspect(&cli.config, &query, limit, hybrid),
    }
}

fn cmd_init(config_path: &str, scan_path: Option<&str>) -> Result<()> {
    let expanded = shellexpand::tilde(config_path);
    let config_exists = Path::new(expanded.as_ref()).exists();
    if config_exists {
        info!("Config already exists at {}", expanded);
    } else {
        let root = scan_path
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let cfg = aikd_core::config::generate_smart_config(&root);
        cfg.save(config_path)?;
        info!("  Config created at {}", expanded);
    }

    // Embedding is opt-in (ADR-0001): never auto-download the model here.
    // Enable explicitly with: aikd model download && aikd embed
    let cfg = load_or_default(config_path);
    let model_dir = cfg.model_path();
    if !aikd_embedder::is_model_downloaded(&model_dir) {
        info!("  Embedding model not present (opt-in). Run: aikd model download && aikd embed");
    } else {
        info!("  Model already downloaded ({})", embedder::MODEL_NAME);
    }

    // Generate shell hooks
    install_shell_hook(config_path);

    // Auto-register ke semua AI agent yang terinstall
    let aikd_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "aikd".to_string());

    info!("\nDetecting AI agents...");
    let results = aikd_core::agents::detect_and_register(&aikd_path);

    for (name, success) in &results {
        if *success {
            info!("  {} - registered", name);
        } else {
            info!("  {} - not found", name);
        }
    }

    // Always write standalone MCP config
    let mcp_config = serde_json::json!({
        "mcpServers": {
            "aikd": {
                "command": aikd_path,
                "args": ["serve"],
            }
        }
    });
    let mcp_path = shellexpand::tilde("~/.aikd/mcp.json");
    if let Some(parent) = Path::new(mcp_path.as_ref()).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        mcp_path.as_ref(),
        serde_json::to_string_pretty(&mcp_config).unwrap_or_default(),
    )?;
    info!("  MCP config: {}", mcp_path);

    info!("\nNext steps:");
    info!("  aikd scan          # Index your project");
    info!("  aikd embed         # Enable semantic search");
    info!("  aikd query \"...\"   # Start searching");

    // Output JSON summary for programmatic use
    let summary = serde_json::json!({
        "status": "ok",
        "config": expanded.to_string(),
        "model_downloaded": aikd_embedder::is_model_downloaded(&model_dir),
        "agents_registered": results.iter().filter(|(_, s)| *s).map(|(n, _)| n).collect::<Vec<_>>(),
        "cli_usage": "aikd query \"keyword\" --json",
        "mcp_config": mcp_path.to_string(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).unwrap_or_default()
    );

    Ok(())
}

fn install_shell_hook(_config_path: &str) {
    let hook_script = r#"# AIKD auto-start hook
aikd_auto_start() {
    if [ -f ".aikd/config.yaml" ] || [ -f "$HOME/.aikd/config.yaml" ]; then
        if ! pgrep -f "aikd daemon" > /dev/null 2>&1; then
            aikd daemon &>/dev/null &
        fi
    fi
}
cd() {
    builtin cd "$@" && aikd_auto_start
}
aikd_auto_start"#
        .to_string();

    // Bash hook
    let bashrc = shellexpand::tilde("~/.bashrc");
    if Path::new(bashrc.as_ref()).exists() {
        let content = std::fs::read_to_string(bashrc.as_ref()).unwrap_or_default();
        if !content.contains("aikd_auto_start") {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .append(true)
                .open(bashrc.as_ref())
            {
                use std::io::Write;
                let _ = writeln!(f, "\n{hook_script}");
                println!("Shell hook installed to {bashrc}");
            }
        }
    }

    // Zsh hook
    let zshrc = shellexpand::tilde("~/.zshrc");
    if Path::new(zshrc.as_ref()).exists() {
        let content = std::fs::read_to_string(zshrc.as_ref()).unwrap_or_default();
        if !content.contains("aikd_auto_start") {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .append(true)
                .open(zshrc.as_ref())
            {
                use std::io::Write;
                let _ = writeln!(f, "\n{hook_script}");
                println!("Shell hook installed to {zshrc}");
            }
        }
    }
}

async fn cmd_daemon(config_path: &str, foreground: bool) -> Result<()> {
    if foreground {
        println!("Starting AIKD daemon in foreground...");
        let cfg = Config::load(config_path).unwrap_or_default();
        let port = cfg.server.rest_port;

        // Background mode (AIKD_DAEMON_BG=1, stdin is null): run REST only.
        // The MCP stdio server would exit immediately on EOF stdin and take
        // the daemon down with it via the select! below.
        if std::env::var("AIKD_DAEMON_BG").as_deref() == Ok("1") {
            return aikd_server::rest::run_rest_server(config_path, port).await;
        } else {
            let config_path_owned = config_path.to_string();
            let rest_handle = tokio::spawn(async move {
                aikd_server::rest::run_rest_server(&config_path_owned, port).await
            });

            let config_path_owned2 = config_path.to_string();
            let mcp_handle =
                tokio::spawn(async move { aikd_server::mcp::run_server(&config_path_owned2).await });

            tokio::select! {
                r = rest_handle => { let _ = r?; }
                r = mcp_handle => { let _ = r?; }
            }
        }
    } else {
        let pid_dir = shellexpand::tilde("~/.aikd");
        let pid_path = format!("{pid_dir}/aikd.pid");
        std::fs::create_dir_all(pid_dir.as_ref())?;

        // Check if daemon already running
        if let Ok(existing_pid) = std::fs::read_to_string(&pid_path) {
            let pid: u32 = existing_pid.trim().parse().unwrap_or(0);
            if pid > 0 {
                #[cfg(unix)]
                {
                    let check = std::process::Command::new("kill")
                        .args(["-0", &pid.to_string()])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    if check.map(|s| s.success()).unwrap_or(false) {
                        eprintln!("AIKD daemon already running (PID {})", pid);
                        return Ok(());
                    }
                }
            }
        }

        // Spawn background process
        let aikd_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "aikd".to_string());
        let log_path = format!("{pid_dir}/aikd.log");

        #[cfg(unix)]
        {
            let child = std::process::Command::new(&aikd_path)
                .args(["--config", config_path, "daemon", "foreground"]).env("AIKD_DAEMON_BG", "1")
                .stdout(std::fs::File::create(&log_path)?)
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null())
                .spawn()?;
            std::fs::write(&pid_path, child.id().to_string())?;
            println!("AIKD daemon started (PID {})", child.id());
            println!("Log: {}", log_path);
            println!("Stop: kill $(cat {})", pid_path);
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let child = std::process::Command::new(&aikd_path)
                .args(["--config", config_path, "daemon", "foreground"]).env("AIKD_DAEMON_BG", "1")
                .stdout(std::fs::File::create(&log_path)?)
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null())
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .spawn()?;
            std::fs::write(&pid_path, child.id().to_string())?;
            println!("AIKD daemon started (PID {})", child.id());
            println!("Log: {log_path}");
        }
    }
    Ok(())
}

fn cmd_daemon_stop() -> Result<()> {
    let pid_path = shellexpand::tilde("~/.aikd/aikd.pid");
    if !std::path::Path::new(pid_path.as_ref()).exists() {
        eprintln!("No daemon PID file found. Daemon may not be running.");
        return Ok(());
    }
    let pid_str = std::fs::read_to_string(pid_path.as_ref())?;
    let pid: u32 = pid_str.trim().parse().unwrap_or(0);
    if pid == 0 {
        eprintln!("Invalid PID file.");
        return Ok(());
    }

    #[cfg(unix)]
    {
        let status = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status()?;
        if status.success() {
            println!("Daemon stopped (PID {})", pid);
            let _ = std::fs::remove_file(pid_path.as_ref());
        } else {
            eprintln!("Failed to kill PID {}. Daemon may already be stopped.", pid);
        }
    }

    #[cfg(windows)]
    {
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()?;
        if status.success() {
            println!("Daemon stopped (PID {pid})");
            let _ = std::fs::remove_file(pid_path.as_ref());
        } else {
            eprintln!("Failed to kill PID {pid}. Daemon may already be stopped.");
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        eprintln!("Unsupported platform for daemon stop.");
    }

    Ok(())
}

fn cmd_daemon_pid() -> Result<()> {
    let pid_path = shellexpand::tilde("~/.aikd/aikd.pid");
    if !std::path::Path::new(pid_path.as_ref()).exists() {
        println!("Daemon not running (no PID file).");
        return Ok(());
    }
    let pid_str = std::fs::read_to_string(pid_path.as_ref())?;
    let pid: u32 = pid_str.trim().parse().unwrap_or(0);
    if pid == 0 {
        println!("Invalid PID file.");
        return Ok(());
    }

    #[cfg(unix)]
    {
        let check = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if check.map(|s| s.success()).unwrap_or(false) {
            println!("Daemon running (PID {})", pid);
        } else {
            println!("Daemon not running (stale PID {})", pid);
            let _ = std::fs::remove_file(pid_path.as_ref());
        }
    }

    #[cfg(not(unix))]
    {
        println!("Daemon PID: {pid}");
    }

    Ok(())
}

fn cmd_scan(config_path: &str, override_path: Option<&str>) -> Result<()> {
    let cfg = load_or_default(config_path);
    // Enforce worker cap: bound the global rayon pool to the resource profile.
    // (One-shot CLI process; `build_global` no-ops if already initialized.)
    let profile = ResourceProfile::detect_with_mode(&cfg.resource.mode);
    let _ = rayon::ThreadPoolBuilder::new().num_threads(profile.parallelism.max(1)).build_global();
    let database = Database::open(&cfg.db_path())?;
    let tantivy = TantivyEngine::open(&cfg.tantivy_path())?;

    let opts = aikd_scanner::ScanOptions {
        override_path: override_path.map(std::path::PathBuf::from),
    };

    einfo!("[aikd] Scanning...");
    let progress = aikd_scanner::run_scan(&cfg, &database, &tantivy, &opts)?;
    einfo!(
        "[aikd] Indexed {} files, {} chunks in {:?}",
        progress.files_indexed,
        progress.chunks_created,
        progress.elapsed
    );
    if progress.files_skipped > 0 {
        einfo!("[aikd] Skipped {} unchanged files", progress.files_skipped);
    }
    Ok(())
}

fn cmd_query(config_path: &str, query: &str, params: &QueryParams) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let tantivy = TantivyEngine::open(&cfg.tantivy_path())?;

    // Auto-scan if DB is empty
    let file_count: i64 = database
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM files WHERE status='active'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if file_count == 0 {
        if !params.json {
            einfo!("[aikd] No files indexed yet. Run: aikd scan");
        }
        return Ok(());
    }

    let filters = SearchFilters {
        path_contains: params.path_filter.map(String::from),
        path_exclude: params.exclude_path.map(String::from),
        file_types: params.file_type.map(|ft| vec![ft.to_string()]),
        heading_contains: params.heading_filter.map(String::from),
    };
    let req = aikd_context::QueryRequest {
        query: query.to_string(),
        filters,
        limit: params.limit,
        hybrid: params.hybrid,
        budget: aikd_context::ContextBudget::default(),
    };
    let model_dir = cfg.model_path();
    let (source, enabled) = aikd_context::SemanticSource::from_config(&cfg, &model_dir);
    let outcome = aikd_context::run_query_with_source(
        &database,
        std::sync::Arc::new(tantivy),
        source,
        enabled,
        &req,
    );
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) if params.hybrid => {
            anyhow::bail!("{e}. Semantic is opt-in: run `aikd model download && aikd embed`, or omit --hybrid for lexical search.");
        }
        Err(e) => anyhow::bail!("{e}"),
    };
    if let aikd_context::RetrievalKind::LexicalFallback { reason } = &outcome.retrieval {
        einfo!("[aikd] Hybrid unavailable ({reason}); used lexical search.");
    }
    if outcome.truncated_chars > 0 {
        einfo!(
            "[aikd] Context budget cut {} chars.",
            outcome.truncated_chars
        );
    }
    print_results(
        query,
        &outcome.results,
        std::time::Duration::from_millis(outcome.elapsed_ms as u64),
        params.json,
    );
    Ok(())
}

fn cmd_export(config_path: &str, output: &str) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let expanded = shellexpand::tilde(output);
    let count = embedder::export_chunks_for_embedding(database.conn(), expanded.as_ref())?;
    println!("Exported {count} chunks to {expanded}");
    Ok(())
}

fn cmd_import(config_path: &str, file: &str) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let expanded = shellexpand::tilde(file);
    let count = embedder::import_embeddings_json(database.conn(), expanded.as_ref())?;
    println!("Imported {count} embeddings");
    Ok(())
}

fn cmd_stats(config_path: &str) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let fc: i64 = database.conn().query_row(
        "SELECT COUNT(*) FROM files WHERE status='active'",
        [],
        |r| r.get(0),
    )?;
    let cc: i64 = database
        .conn()
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let ts: i64 = database.conn().query_row(
        "SELECT COALESCE(SUM(size),0) FROM files WHERE status='active'",
        [],
        |r| r.get(0),
    )?;
    let ec: i64 = database
        .conn()
        .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
        .unwrap_or(0);
    let (sc, convc, ce) = session::get_session_stats(database.conn()).unwrap_or((0, 0, 0));
    let profile = ResourceProfile::detect_with_mode(&cfg.resource.mode);
    let stats = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "files": fc,
        "chunks": cc,
        "embeddings": ec,
        "dimensions": embedder::DIMENSIONS,
        "sessions": sc,
        "conversations": convc,
        "conversation_embeddings": ce,
        "total_size_kb": ts as f64 / 1024.0,
        "db_path": cfg.index.db_path,
        "tantivy_path": cfg.index.tantivy_path,
        "cpu_cores": profile.cpu_cores,
        "ram_gb": profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        "embedding_enabled": profile.embedding_enabled,
        "rest_port": cfg.server.rest_port,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&stats).unwrap_or_default()
    );
    Ok(())
}

#[cfg(feature = "semantic-ext")]
fn cmd_embed_external(_config_path: &str, cfg: &aikd_core::Config) -> Result<()> {
    use aikd_core::SemanticProvider;
    let database = Database::open(&cfg.db_path())?;
    let provider = embedder::ExternalProvider::from_env(&cfg.embedding.endpoint, &cfg.embedding.model, &cfg.embedding.api_key_env)?;
    if !provider.is_available() {
        anyhow::bail!("external provider unavailable (endpoint/key)");
    }
    let batch: usize = cfg.embedding.batch_size.parse().unwrap_or(32);
    let n = embedder::embed_and_store_external(database.conn(), &provider, batch)?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "status": "ok", "provider": provider.name(), "model": provider.model_id(), "embeddings_created": n,
    })).unwrap_or_default());
    Ok(())
}

#[cfg(not(feature = "semantic-ext"))]
fn cmd_embed_external(_config_path: &str, cfg: &aikd_core::Config) -> Result<()> {
    let _ = cfg;
    anyhow::bail!("provider=external needs the semantic-ext build: cargo build --features semantic-ext (zero-ONNX HTTP provider, no model download)")
}

fn cmd_embed(config_path: &str, _model: &str, _batch_size: usize) -> Result<()> {
    let cfg = load_or_default(config_path);
    if !cfg.embedding.enabled {
        anyhow::bail!("Semantic embedding is disabled (embedding.enabled=false). Set embedding.enabled=true in config to opt in");
    }
    // External HTTP provider: zero-ONNX, no model download. The provider
    // owns inference behind the SemanticProvider contract (§E).
    if cfg.embedding.provider == "external" {
        return cmd_embed_external(config_path, &cfg);
    }
    let database = Database::open(&cfg.db_path())?;
    let count: i64 = database
        .conn()
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    if count == 0 {
        einfo!("[aikd] No chunks to embed. Run: aikd scan first");
        return Ok(());
    }
    let existing: i64 = database
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE model = ?1",
            rusqlite::params![embedder::MODEL_NAME],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let remaining = count - existing;
    if remaining <= 0 {
        einfo!("[aikd] All {} chunks already embedded.", count);
        return Ok(());
    }
    let model_dir = cfg.model_path();
    einfo!(
        "[aikd] {} chunks total, {} already embedded, {} to process",
        count,
        existing,
        remaining
    );

    let profile = ResourceProfile::detect_with_mode(&cfg.resource.mode);
    let start = std::time::Instant::now();

    if !QUIET.load(Ordering::Relaxed) {
        use indicatif::{ProgressBar, ProgressStyle};
        let pb = ProgressBar::new(remaining as u64);
        pb.set_style(ProgressStyle::default_bar()
            .template("[aikd] {spinner:.green} Embedding [{bar:40.cyan/blue}] {pos}/{len} chunks | {per_sec} | ETA: {eta}")
            .unwrap()
            .progress_chars("█░"));

        // Use embed_and_store_with_profile for resource-aware embedding
        let imported =
            embedder::embed_and_store_with_profile(database.conn(), &model_dir, &profile)?;
        pb.finish_with_message("done");

        einfo!(
            "[aikd] {} embeddings stored in {:.1}s",
            imported,
            start.elapsed().as_secs_f64()
        );
        einfo!("[aikd] Hybrid search now available.");

        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "ok",
                "embeddings_created": imported,
                "total_chunks": count,
                "elapsed_ms": start.elapsed().as_millis(),
            }))
            .unwrap_or_default()
        );
    } else {
        let imported =
            embedder::embed_and_store_with_profile(database.conn(), &model_dir, &profile)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "ok",
                "embeddings_created": imported,
                "total_chunks": count,
                "elapsed_ms": start.elapsed().as_millis(),
            }))
            .unwrap_or_default()
        );
    }
    Ok(())
}

async fn cmd_serve(config_path: &str) -> Result<()> {
    aikd_server::run_mcp_server(config_path).await
}

async fn cmd_watch(config_path: &str, debounce: u64) -> Result<()> {
    aikd_watcher::run_watcher(config_path, debounce).await
}

fn cmd_remember(
    config_path: &str,
    session_id: Option<&str>,
    role: &str,
    content: &str,
    json: bool,
) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let sid = match session_id {
        Some(id) => id.to_string(),
        None => {
            session::get_or_create_session(
                database.conn(),
                &aikd_core::platform::canonical_project(&cfg.scan.include_paths),
            )?
            .id
        }
    };
    let conv = session::remember(database.conn(), &sid, role, content, &[])?;

    // Semantic is opt-in (ADR-0001): only embed when explicitly enabled in config.
    let model_dir = cfg.model_path();
    if cfg.embedding.enabled && embedder::is_model_downloaded(&model_dir) {
        if let Err(e) = session::embed_conversations(database.conn(), &model_dir, &sid) {
            eprintln!("[aikd] Warning: failed to embed conversations: {e}");
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "success": true,
                "conversation_id": conv.id,
                "session_id": conv.session_id,
                "role": conv.role,
            }))
            .unwrap_or_default()
        );
    } else {
        info!(
            "Remembered in session {}: [{}] {}",
            conv.session_id,
            conv.role,
            conv.content.chars().take(100).collect::<String>()
        );
    }
    Ok(())
}

fn cmd_recall(
    config_path: &str,
    query: &str,
    session_id: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let sid = match session_id {
        Some(id) => id.to_string(),
        None => {
            session::get_or_create_session(
                database.conn(),
                &aikd_core::platform::canonical_project(&cfg.scan.include_paths),
            )?
            .id
        }
    };
    let convs = session::recall(database.conn(), &sid, query, limit)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&convs).unwrap_or_default()
        );
    } else if convs.is_empty() {
        println!("No matching conversations found.");
    } else {
        println!("{} results for '{}':\n", convs.len(), query);
        for (i, c) in convs.iter().enumerate() {
            println!(
                "{}. [{}] {}: {}",
                i + 1,
                c.created_at,
                c.role,
                c.content.chars().take(200).collect::<String>()
            );
        }
    }
    Ok(())
}

fn cmd_status(config_path: &str, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let profile = ResourceProfile::detect_with_mode(&cfg.resource.mode);
    if json {
        let status = serde_json::json!({
            "cpu_cores": profile.cpu_cores,
            "ram_gb": profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            "has_gpu": profile.has_gpu,
            "embedding_enabled": profile.embedding_enabled,
            "batch_size": profile.batch_size,
            "parallelism": profile.parallelism,
            "hnsw_m": profile.hnsw_m,
            "rest_port": cfg.server.rest_port,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&status).unwrap_or_default()
        );
    } else {
        println!("=== AIKD Daemon Status ===");
        println!("CPU Cores:   {}", profile.cpu_cores);
        println!(
            "RAM:         {:.1} GB",
            profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        println!("GPU:         {}", profile.has_gpu);
        println!(
            "Embedding:   {}",
            if profile.embedding_enabled {
                "ON"
            } else {
                "OFF"
            }
        );
        println!("Batch Size:  {}", profile.batch_size);
        println!("Parallelism: {}", profile.parallelism);
        println!("HNSW M:      {}", profile.hnsw_m);
        println!("REST Port:   {}", cfg.server.rest_port);
    }
    Ok(())
}

/// Doctor (§19): live diagnostics — process RSS, DB/index/model sizes,
/// index health, Atlas freshness, memory conflicts, integrity check.
/// Every line comes from real state; warnings name the fix command.
fn cmd_doctor(config_path: &str) -> Result<()> {
    use sysinfo::{Pid, System};
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let conn = database.conn();
    let qi = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(-1) };
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let rss_mb = sys.process(Pid::from(std::process::id() as usize))
        .map(|p| p.memory() as f64 / (1024.0 * 1024.0))
        .unwrap_or(-1.0);
    sys.refresh_memory();
    let host_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let profile = ResourceProfile::detect_with_mode(&cfg.resource.mode);
    let db_mb = std::fs::metadata(cfg.db_path()).map(|m| m.len() as f64 / (1024.0 * 1024.0)).unwrap_or(-1.0);
    let tantivy_mb = dir_mb(&cfg.tantivy_path());
    let model_mb = dir_mb(&cfg.model_path());
    let files = qi("SELECT COUNT(*) FROM files");
    let chunks = qi("SELECT COUNT(*) FROM chunks");
    let symbols = qi("SELECT COUNT(*) FROM code_symbols");
    let events = qi("SELECT COUNT(*) FROM events");
    let mem_rows: Vec<(String, i64)> = conn.prepare("SELECT status, COUNT(*) FROM memory GROUP BY status")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).and_then(|rows| rows.collect::<std::result::Result<Vec<_>, _>>()))
        .unwrap_or_default();
    let atlases: Vec<(String, String)> = conn.prepare("SELECT project_path, status FROM atlas")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).and_then(|rows| rows.collect::<std::result::Result<Vec<_>, _>>()))
        .unwrap_or_default();
    let integrity = aikd_storage::maintenance::db_check(conn).unwrap_or_default();
    let mut warnings: Vec<String> = Vec::new();
    if !integrity.is_empty() {
        warnings.push(format!("integrity check found {} issue(s); run `aikd db check` for detail", integrity.len()));
    }
    for (p, st) in &atlases {
        if st == "stale" {
            warnings.push(format!("Atlas STALE for {p}; run `aikd project analyze --path {p}`"));
        }
    }
    let conf: i64 = mem_rows.iter().find(|(s, _)| s == "conflicted").map(|(_, n)| *n).unwrap_or(0);
    if conf > 0 {
        warnings.push(format!("{conf} conflicted memor(ies); resolve with `aikd memory` supersede/archive, see `aikd handoff`"));
    }
    if db_mb > 500.0 {
        warnings.push(format!("DB is {db_mb:.0} MB; consider `aikd db backup` then `aikd gc`/`aikd db compact`"));
    }
    if !aikd_embedder::SEMANTIC_COMPILED && cfg.embedding.enabled {
        warnings.push("config enables embedding but this is a zero-ONNX build; set embedding.enabled=false or rebuild with --features semantic".to_string());
    }
    println!("=== AIKD Doctor ===");
    println!("Process RSS:  {rss_mb:.1} MB (host RAM {host_gb:.1} GB, {cores} cores, mode {mode:?})",
        cores = profile.cpu_cores, mode = cfg.resource.mode);
    println!("Enforced caps: parallelism={} batch={} tantivy_heap={}MB cache={}MB debounce={}ms",
        profile.parallelism, profile.batch_size, profile.tantivy_heap_mb, profile.cache_size_mb, profile.watcher_debounce_ms);
    println!("DB: {db_mb:.1} MB (schema v{}) · tantivy {tantivy_mb:.1} MB · model dir {model_mb:.1} MB · semantic_compiled={}",
        qi("SELECT version FROM schema_version LIMIT 1"), aikd_embedder::SEMANTIC_COMPILED);
    println!("Index: files={files} chunks={chunks} symbols={symbols} events={events}");
    print!("Memory:");
    for (s, n) in &mem_rows {
        print!(" {s}={n}");
    }
    println!();
    println!("Atlas snapshots: {}", if atlases.is_empty() { "(none)".to_string() } else { atlases.iter().map(|(p, s)| format!("{p}[{s}]")).collect::<Vec<_>>().join(", ") });
    println!("Integrity: {}", if integrity.is_empty() { "OK".to_string() } else { format!("{} issue(s)", integrity.len()) });
    if warnings.is_empty() {
        println!("Warnings: none — all healthy.");
    } else {
        println!("Warnings ({}):", warnings.len());
        for w in &warnings {
            println!("  ! {w}");
        }
    }
    Ok(())
}

fn dir_mb(path: &std::path::Path) -> f64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        if let Ok(md) = std::fs::metadata(&p) {
            if md.is_file() {
                total += md.len();
            } else if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    stack.push(e.path());
                }
            }
        }
    }
    total as f64 / (1024.0 * 1024.0)
}

fn print_results(
    query: &str,
    results: &[aikd_core::SearchResult],
    elapsed: std::time::Duration,
    json: bool,
) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(results).unwrap_or_default()
        );
        return;
    }
    if results.is_empty() {
        println!("No results for: {query}");
        return;
    }
    println!(
        "{} results for '{}' ({:?}):\n",
        results.len(),
        query,
        elapsed
    );
    for (i, r) in results.iter().enumerate() {
        println!("{}. {}", i + 1, r.file_path);
        if !r.heading_text.is_empty() {
            println!("   Heading: {}", r.heading_hierarchy);
        }
        if r.line_start > 0 {
            println!("   Lines: {}-{}", r.line_start, r.line_end);
        }
        if r.score > 0.0 {
            println!("   Score: {:.3}", r.score);
        }
        let preview = if r.content.chars().count() > 200 {
            let end = r
                .content
                .char_indices()
                .nth(200)
                .map(|(i, _)| i)
                .unwrap_or(r.content.len());
            format!("{}...", &r.content[..end])
        } else {
            r.content.clone()
        };
        println!("   {}", preview.replace('\n', "\n   "));
        println!();
    }
}

fn load_or_default(p: &str) -> Config {
    Config::load(p).unwrap_or_else(|_| {
        log::warn!("Using defaults");
        Config::default()
    })
}

fn cmd_session(config_path: &str, action: SessionAction, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;

    match action {
        SessionAction::List => {
            let sessions = session::list_sessions(database.conn())?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&sessions).unwrap_or_default()
                );
            } else if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("{} sessions:\n", sessions.len());
                for (i, s) in sessions.iter().enumerate() {
                    println!("{}. {} ({})", i + 1, s.name, s.id);
                    println!("   Project: {}", s.project_path);
                    println!("   Last active: {}", s.last_active);
                    println!();
                }
            }
        }
        SessionAction::New { name } => {
            let project_path = cfg
                .scan
                .include_paths
                .first()
                .cloned()
                .unwrap_or_else(|| ".".to_string());
            let session_name = name.unwrap_or_else(|| {
                format!("Session {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))
            });
            let s = session::create_session(database.conn(), &session_name, &project_path)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "id": s.id,
                        "name": s.name,
                        "project_path": s.project_path,
                    }))
                    .unwrap_or_default()
                );
            } else {
                println!("Created session: {} ({})", s.name, s.id);
            }
        }
        SessionAction::Delete { id } => {
            aikd_storage::cascade::delete_session_cascade(database.conn(), &id)?;
            if json {
                println!("{}", serde_json::json!({"deleted": id}));
            } else {
                println!("Deleted session: {id}");
            }
        }
    }
    Ok(())
}

fn cmd_model(config_path: &str, action: ModelAction) -> Result<()> {
    let cfg = load_or_default(config_path);
    let model_dir = cfg.model_path();

    match action {
        ModelAction::Status => {
            let downloaded = embedder::is_model_downloaded(&model_dir);
            if downloaded {
                let size: u64 = std::fs::read_dir(&model_dir)
                    .map(|entries| {
                        entries
                            .filter_map(|e| e.ok())
                            .filter_map(|e| e.metadata().ok())
                            .map(|m| m.len())
                            .sum()
                    })
                    .unwrap_or(0);
                println!("Model: {}", embedder::MODEL_NAME);
                println!("Status: Downloaded");
                println!("Path: {}", model_dir.display());
                println!("Size: {:.1} MB", size as f64 / (1024.0 * 1024.0));
                println!("Dimensions: {}", embedder::DIMENSIONS);
            } else {
                println!("Model: {}", embedder::MODEL_NAME);
                println!("Status: Not downloaded");
                println!("Download: aikd model download");
            }
        }
        ModelAction::Download => {
            if embedder::is_model_downloaded(&model_dir) {
                println!("Model already downloaded at {}", model_dir.display());
            } else {
                println!("Downloading {}...", embedder::MODEL_NAME);
                embedder::download_model(&model_dir)?;
                println!("Model downloaded to {}", model_dir.display());
            }
        }
        ModelAction::Remove => {
            if model_dir.exists() {
                let size: u64 = std::fs::read_dir(&model_dir)
                    .map(|entries| {
                        entries
                            .filter_map(|e| e.ok())
                            .filter_map(|e| e.metadata().ok())
                            .map(|m| m.len())
                            .sum()
                    })
                    .unwrap_or(0);
                std::fs::remove_dir_all(&model_dir)?;
                println!(
                    "Removed model files ({:.1} MB)",
                    size as f64 / (1024.0 * 1024.0)
                );
            } else {
                println!("No model files found at {}", model_dir.display());
            }
        }
    }
    Ok(())
}

fn cmd_db(config_path: &str, action: DbAction, json: bool) -> Result<()> {
    use aikd_storage::maintenance::{db_backup, db_check, db_compact, db_restore, db_status};
    let cfg = load_or_default(config_path);
    let db_path = cfg.db_path();
    match action {
        DbAction::Status => {
            let database = Database::open(&db_path)?;
            let st = db_status(database.conn(), &db_path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&st).unwrap_or_default());
            } else {
                println!("DB: {} ({:.1} MB)", st.db_path, st.db_bytes as f64 / (1024.0 * 1024.0));
                println!("Schema: v{} | journal: {}", st.schema_version, st.journal_mode);
                println!(
                    "Files: {} | chunks: {} | embeddings: {} | sessions: {} | conversations: {}",
                    st.files, st.chunks, st.embeddings, st.sessions, st.conversations
                );
                println!(
                    "FTS docs: {} | pages: {} x {} bytes | freelist: {}",
                    st.fts_docs, st.page_count, st.page_size, st.freelist_pages
                );
            }
        }
        DbAction::Check => {
            let database = Database::open(&db_path)?;
            let problems = db_check(database.conn())?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "healthy": problems.is_empty(),
                        "problems": problems,
                    }))
                    .unwrap_or_default()
                );
            } else if problems.is_empty() {
                println!("OK: database healthy");
            } else {
                println!("Found {} problem(s):", problems.len());
                for p in &problems {
                    println!("  - {p}");
                }
                anyhow::bail!("database check failed");
            }
        }
        DbAction::Backup { output } => {
            let database = Database::open(&db_path)?;
            let expanded = shellexpand::tilde(&output);
            let n = db_backup(database.conn(), std::path::Path::new(expanded.as_ref()))?;
            println!("Backup: {expanded} ({:.1} MB)", n as f64 / (1024.0 * 1024.0));
        }
        DbAction::Restore { file } => {
            let expanded = shellexpand::tilde(&file);
            db_restore(std::path::Path::new(expanded.as_ref()), &db_path)?;
            println!("Restored from {expanded}. Re-run `aikd db check` to verify.");
        }
        DbAction::Compact { rebuild_fts } => {
            let database = Database::open(&db_path)?;
            let msg = db_compact(database.conn(), rebuild_fts)?;
            println!("Compact: {msg}");
        }
    }
    Ok(())
}

fn cmd_gc(config_path: &str, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let tantivy = TantivyEngine::open(&cfg.tantivy_path())?;
    // Collect stale chunk IDs for the index before rows disappear.
    let stale: Vec<String> = {
        let mut stmt = database
            .conn()
            .prepare("SELECT c.id, f.path FROM chunks c JOIN files f ON f.id = c.file_id")?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        rows.into_iter()
            .filter(|(_, ps)| !std::path::Path::new(ps).exists())
            .map(|(id, _)| id)
            .collect()
    };
    if !stale.is_empty() {
        tantivy.delete_chunks(&stale)?;
    }
    let (files_removed, orphan_chunks) =
        aikd_storage::maintenance::gc(database.conn(), true)?;
    let retention = aikd_storage::maintenance::enforce_retention(database.conn(), &cfg.retention)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "files_removed": files_removed,
                "orphan_chunks_removed": orphan_chunks,
                "index_docs_removed": stale.len(),
                "events_archived": retention.events_archived,
                "memory_marked_stale": retention.memory_marked_stale,
            }))
            .unwrap_or_default()
        );
    } else {
        println!(
            "GC: {files_removed} file(s), {orphan_chunks} orphan chunk(s), {} index doc(s) removed",
            stale.len()
        );
        println!(
            "Retention: {} event(s) archived, {} memor(ies) marked stale",
            retention.events_archived, retention.memory_marked_stale
        );
    }
    Ok(())
}

fn cmd_symbol(config_path: &str, name: &str, limit: usize, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let symbols = aikd_storage::symbols::find_symbols(database.conn(), name, limit)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "query": name,
                "provenance": "EXTRACTED",
                "count": symbols.len(),
                "symbols": symbols,
            }))
            .unwrap_or_default()
        );
    } else if symbols.is_empty() {
        println!("No symbols matching '{name}'. Run: aikd scan");
    } else {
        println!("{} symbol(s) for '{name}' (EXTRACTED, heuristic):", symbols.len());
        for s in &symbols {
            println!(
                "  {} {} — {}:{}-{} [{}]",
                s.kind, s.name, s.file_path, s.line_start, s.line_end, s.language
            );
            if !s.signature.is_empty() {
                println!("    {}", s.signature);
            }
        }
    }
    Ok(())
}

fn cmd_memory(config_path: &str, action: MemoryAction, json: bool) -> Result<()> {
    use aikd_storage::memory::{active_tasks, latest_checkpoint, list_memory, save_memory, supersede};
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let project = cfg
        .scan
        .include_paths
        .first()
        .cloned()
        .unwrap_or_default();
    match action {
        MemoryAction::Save { kind, title, body, workstream, source, confidence } => {
            const KINDS: [&str; 6] = ["fact", "decision", "constraint", "discovery", "task", "checkpoint"];
            if !KINDS.contains(&kind.as_str()) {
                anyhow::bail!("Invalid kind '{kind}'. Use one of: fact, decision, constraint, discovery, task, checkpoint");
            }
            if !["explicit", "agent-asserted", "inferred"].contains(&source.as_str()) {
                anyhow::bail!("Invalid source '{source}'. Use: explicit, agent-asserted, inferred");
            }
            let m = save_memory(database.conn(), &project, &workstream, &kind, &title, &body, &source, confidence)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&m).unwrap_or_default());
            } else {
                println!("Saved {} '{}' [{}] (id {})", m.kind, m.title, m.source, m.id);
            }
        }
        MemoryAction::List { kind, workstream, all } => {
            let items = list_memory(database.conn(), &project, Some(&workstream), kind.as_deref(), all)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&items).unwrap_or_default());
            } else if items.is_empty() {
                println!("No memory items.");
            } else {
                for m in &items {
                    println!("[{}] {} — {} [{}|{}|conf {:.2}]", m.kind, m.title, m.id, m.source, m.status, m.confidence);
                    println!("    {}", m.body.lines().next().unwrap_or(""));
                }
            }
        }
        MemoryAction::Supersede { id, title, body } => {
            // Carry over kind/project/workstream/source from the retired item.
            let old: Option<aikd_core::MemoryItem> = list_memory(database.conn(), &project, None, None, true)?
                .into_iter()
                .find(|m| m.id == id);
            let old = old.ok_or_else(|| anyhow::anyhow!("No memory item with id {id}"))?;
            let m = supersede(database.conn(), &id, &project, &old.workstream, &old.kind, &title, &body, &old.source, old.confidence)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&m).unwrap_or_default());
            } else {
                println!("Superseded {} -> {} ('{}')", id, m.id, m.title);
            }
        }
        MemoryAction::Task { workstream } => {
            let tasks = active_tasks(database.conn(), &project, &workstream)?;
            let cp = latest_checkpoint(database.conn(), &project, &workstream)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "workstream": workstream,
                        "tasks": tasks,
                        "checkpoint": cp,
                    }))
                    .unwrap_or_default()
                );
            } else {
                println!("Workstream: {workstream}");
                for t in &tasks {
                    println!("\nTASK {} — {}\n{}", t.id, t.title, t.body);
                }
                if tasks.is_empty() {
                    println!("(no active tasks)");
                }
                match &cp {
                    Some(c) => println!("\nCHECKPOINT {} — {}\n{}", c.id, c.title, c.body),
                    None => println!("\n(no checkpoint)"),
                }
            }
        }
        MemoryAction::Stale { id } => {
            use aikd_storage::memory::mark_stale;
            let n = mark_stale(database.conn(), &id)?;
            println!("{}", if n == 1 { format!("Marked stale: {id}") } else { format!("No change (id {id} not active/conflicted)") });
        }
        MemoryAction::Archive { id } => {
            use aikd_storage::memory::archive;
            let n = archive(database.conn(), &id)?;
            println!("{}", if n == 1 { format!("Archived: {id}") } else { format!("No change (id {id} not found)") });
        }
        MemoryAction::Conflict { id_a, id_b, note } => {
            use aikd_storage::memory::flag_conflict;
            flag_conflict(database.conn(), &id_a, &id_b, &note)?;
            println!("Flagged conflict: {id_a} <-> {id_b} ({note}) — both preserved, neither is fact");
        }
    }
    Ok(())
}

fn cmd_event(config_path: &str, action: EventAction, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
    match action {
        EventAction::Append { event_type, workstream, agent, session, source_ref, payload } => {
            // Validate payload is JSON (canonical model stores payload as JSON text).
            let v: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|e| anyhow::anyhow!("payload must be JSON: {e}"))?;
            let ev = aikd_storage::events::append_event(
                database.conn(), &project, &workstream, &agent, &session, &event_type, &source_ref,
                &v.to_string(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&ev).unwrap_or_default());
            } else {
                println!("Event {} seq={} ({}:{})", ev.event_id, ev.sequence, ev.agent_id, ev.event_type);
            }
        }
        EventAction::List { workstream, event_type, since, limit } => {
            let items = aikd_storage::events::list_events(database.conn(), &project, &workstream, event_type.as_deref(), since, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&items).unwrap_or_default());
            } else if items.is_empty() {
                println!("No events (workstream '{workstream}', since {since}).");
            } else {
                for e in &items {
                    println!("#{} [{}] {}:{} {}", e.sequence, e.timestamp, e.agent_id, e.event_type, e.payload.lines().next().unwrap_or(""));
                }
            }
        }
    }
    Ok(())
}

/// Context inspector (§25): show exactly what would be sent and why.
fn cmd_context_inspect(config_path: &str, query: &str, limit: usize, hybrid: bool) -> Result<()> {
    use aikd_core::SearchFilters;
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let tantivy = TantivyEngine::open(&cfg.tantivy_path())?;
    let req = aikd_context::QueryRequest {
        query: query.to_string(),
        filters: SearchFilters::default(),
        limit,
        hybrid,
        budget: aikd_context::ContextBudget::default(),
    };
    let model_dir = cfg.model_path();
    let (source, enabled) = aikd_context::SemanticSource::from_config(&cfg, &model_dir);
    let insp = aikd_context::inspect_context(
        &database,
        std::sync::Arc::new(tantivy),
        source,
        enabled,
        &req,
    )?;
    println!("CONTEXT REQUEST\n  Task: {query}\n");
    println!("RETRIEVAL: {} ({}ms)", insp.retrieval, insp.elapsed_ms);
    println!("\nSELECTED:");
    for s in &insp.selected {
        println!("  - {} @ {} (score {:.3}) — {}", s["file"].as_str().unwrap_or("?"), s["lines"].as_str().unwrap_or("?"), s["score"].as_f64().unwrap_or(0.0), s["why"].as_str().unwrap_or(""));
    }
    if insp.selected.is_empty() {
        println!("  (none)");
    }
    println!("\nEXCLUDED:");
    for e in &insp.excluded_reasons {
        println!("  - {e}");
    }
    println!("\nESTIMATED TOKENS: ~{} (truncated {} chars)", insp.estimated_tokens, insp.truncated_chars);
    Ok(())
}

/// Context package for session resume / cross-agent handoff (attachment P5).
/// Assembled by `aikd_context::assemble_handoff` — same constructor as MCP.
fn cmd_handoff(config_path: &str, workstream: &str, recent_events: usize, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let project = aikd_core::platform::canonical_project(&cfg.scan.include_paths);
    let pkg = aikd_context::assemble_handoff(database.conn(), &project, workstream, recent_events)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&pkg).unwrap_or_default());
    } else {
        println!("{}", pkg.render_text());
    }
    Ok(())
}

fn cmd_project(config_path: &str, action: ProjectAction, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let resolve = |p: &Option<String>| -> String {
        match p {
            Some(v) => v.replace('\\', "/"),
            None => cfg.scan.include_paths.first().cloned().unwrap_or_else(|| ".".to_string()).replace('\\', "/"),
        }
    };
    match action {
        ProjectAction::Analyze { path } => {
            let project = resolve(&path);
            // Ensure the index is current first (incremental: unchanged files skipped).
            let tantivy = TantivyEngine::open(&cfg.tantivy_path())?;
            let opts = aikd_scanner::ScanOptions { override_path: Some(project.clone().into()) };
            let progress = aikd_scanner::run_scan(&cfg, &database, &tantivy, &opts)?;
            let (snap, _) = aikd_storage::atlas::refresh_atlas(database.conn(), &project)?;
            print_atlas(&snap, &progress, json);
        }
        ProjectAction::Show { path } => {
            let project = resolve(&path);
            match aikd_storage::atlas::read_atlas(database.conn(), &project)? {
                Some((snap, status, updated)) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&snap).unwrap_or_default());
                    } else {
                        println!("ATLAS {project} [{status}] updated {updated}");
                        print_atlas_body(&snap);
                    }
                }
                None => println!("No Atlas for {project}. Run: aikd project analyze --path {project}"),
            }
        }
        ProjectAction::Export { path, out } => {
            let project = resolve(&path);
            let (snap, _) = aikd_storage::atlas::refresh_atlas(database.conn(), &project)?;
            let (n_json, n_md) = write_artifacts(&project, &out, &snap)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({"intelligence_files": n_json, "artifact_files": n_md, "dir": out})).unwrap_or_else(|_| "{}".to_string()));
            } else {
                println!("Wrote {n_json} intelligence + {n_md} artifact files to {out}/ (from Atlas snapshot, status fresh)");
            }
        }
    }
    Ok(())
}

fn print_atlas(snap: &aikd_storage::atlas::AtlasSnapshot, progress: &aikd_scanner::ScanProgress, json: bool) {
    if json {
        let v = serde_json::json!({
            "files_indexed": progress.files_indexed, "chunks_created": progress.chunks_created,
            "files_skipped": progress.files_skipped, "atlas": snap,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    let r = &snap.repository.data;
    println!("\nPROJECT ANALYZED\n");
    println!("Project:\n  {0}", snap.project);
    println!("Files: {}  Chunks: {}  Symbols: {}", r.file_count, r.chunk_count, r.symbol_count);
    println!("Languages:");
    for (l, n) in &r.languages {
        println!("  - {l}: {n} files");
    }
    println!("Frameworks ({}):", r.frameworks.len());
    for f in &r.frameworks {
        println!("  - {f}");
    }
    println!("Entry points:");
    for e in &r.entry_points {
        println!("  - {e}");
    }
    let a = &snap.architecture.data;
    println!("Top directories:");
    for (d, n) in a.top_dirs.iter().take(10) {
        let disp = if d == "(root)" { d.clone() } else { format!("{d}/") };
        println!("  - {disp} ({n} files)");
    }
    println!("Symbol kinds:");
    for (k, n) in &a.symbol_kinds {
        println!("  - {k}: {n}");
    }
    println!("Decisions: {}  Open tasks: {}  Checkpoints: {}  Events: {}",
        snap.decisions.len(), snap.open_tasks.len(), snap.checkpoints.len(), snap.event_count);
    if !snap.known_issues.data.is_empty() {
        println!("Known issues ({} markers):", snap.known_issues.data.len());
        for i in snap.known_issues.data.iter().take(10) {
            println!("  - {}:{} [{}] {}", i.file, i.line, i.marker, i.text.lines().next().unwrap_or(""));
        }
    }
    println!("\nCoverage: files+symbols VERIFIED from index; frameworks/entry-point grouping INFERRED from markers.");
    println!("Unchanged data reused: {} files skipped, Atlas fingerprint fresh.", progress.files_skipped);
}

fn print_atlas_body(snap: &aikd_storage::atlas::AtlasSnapshot) {
    let r = &snap.repository.data;
    println!("Files: {}  Chunks: {}  Symbols: {}", r.file_count, r.chunk_count, r.symbol_count);
    println!("Languages: {}", r.languages.iter().map(|(l, n)| format!("{l}({n})")).collect::<Vec<_>>().join(", "));
    println!("Frameworks [INFERRED]: {}", if r.frameworks.is_empty() { "(none detected)".into() } else { r.frameworks.join(", ") });
    println!("Decisions: {}  Open tasks: {}  Checkpoints: {}  Events: {}",
        snap.decisions.len(), snap.open_tasks.len(), snap.checkpoints.len(), snap.event_count);
}

/// Write machine-readable JSON + human-readable Markdown from the canonical
/// snapshot. Both projections come from the same data — no disconnected docs.
fn write_artifacts(project: &str, out_dir: &str, snap: &aikd_storage::atlas::AtlasSnapshot) -> Result<(usize, usize)> {
    let intel = format!("{out_dir}/intelligence");
    let arts = format!("{out_dir}/artifacts");
    std::fs::create_dir_all(&intel)?;
    std::fs::create_dir_all(&arts)?;
    let r = &snap.repository.data;
    let a = &snap.architecture.data;
    let mut n_json = 0;
    let put = |name: &str, v: &serde_json::Value| -> Result<()> {
        std::fs::write(format!("{intel}/{name}"), serde_json::to_string_pretty(v)?)?;
        Ok(())
    };
    put("project.json", &serde_json::json!({"root": r.root, "files": r.file_count, "chunks": r.chunk_count, "symbols": r.symbol_count, "languages": r.languages, "provenance": "VERIFIED"}))?; n_json += 1;
    put("architecture.json", &serde_json::json!({"top_dirs": a.top_dirs, "symbol_kinds": a.symbol_kinds, "provenance": "INFERRED"}))?; n_json += 1;
    put("components.json", &serde_json::json!({"top_dirs": a.top_dirs}))?; n_json += 1;
    put("symbols.json", &serde_json::json!({"kinds": a.symbol_kinds, "total": r.symbol_count}))?; n_json += 1;
    put("dependencies.json", &serde_json::json!({"frameworks": r.frameworks, "provenance": "INFERRED"}))?; n_json += 1;
    put("workflows.json", &serde_json::json!({"entry_points": r.entry_points, "open_tasks": snap.open_tasks, "provenance": "INFERRED"}))?; n_json += 1;
    put("api.json", &serde_json::json!({"entry_points": r.entry_points}))?; n_json += 1;
    put("database.json", &serde_json::json!({"detected": [], "note": "DB detection via migration/model files — extend detect_frameworks when needed"}))?; n_json += 1;
    put("development.json", &serde_json::json!({"frameworks": r.frameworks}))?; n_json += 1;
    put("issues.json", &serde_json::to_value(&snap.known_issues.data)?)?; n_json += 1;
    let mut n_md = 0;
    let md = |name: &str, title: &str, body: String| -> Result<()> {
        std::fs::write(format!("{arts}/{name}"), format!("# {title}\n\n_Generated from Atlas snapshot of `{project}` — do not edit by hand; regenerate with `aikd project export`._\n\n{body}"))?;
        Ok(())
    };
    md("PROJECT_OVERVIEW.md", "Project overview", format!("Files: {}\nChunks: {}\nSymbols: {}\nLanguages: {}\n", r.file_count, r.chunk_count, r.symbol_count, r.languages.iter().map(|(l, n)| format!("{l}({n})")).collect::<Vec<_>>().join(", ")))?; n_md += 1;
    md("ARCHITECTURE.md", "Architecture", format!("Top dirs:\n{}\n\nSymbol kinds:\n{}\n", a.top_dirs.iter().map(|(d, n)| format!("- {d}/ ({n})")).collect::<Vec<_>>().join("\n"), a.symbol_kinds.iter().map(|(k, n)| format!("- {k}: {n}")).collect::<Vec<_>>().join("\n")))?; n_md += 1;
    md("COMPONENTS.md", "Components", a.top_dirs.iter().map(|(d, n)| format!("- {d}/ ({n} files)")).collect::<Vec<_>>().join("\n"))?; n_md += 1;
    md("WORKFLOWS.md", "Workflows", format!("Entry points:\n{}\n\nOpen tasks: {}\n", r.entry_points.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n"), snap.open_tasks.len()))?; n_md += 1;
    md("DATA_FLOW.md", "Data flow", "Derived from imports/contains relations in the symbol index; extend with call-graph analysis as needed.\n".to_string())?; n_md += 1;
    md("API_REFERENCE.md", "API reference", if r.entry_points.is_empty() { "(no entry points detected)\n".to_string() } else { r.entry_points.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n") + "\n" })?; n_md += 1;
    md("DATABASE.md", "Database", "No migration/model markers analyzed yet — recorded as UNVERIFIED.\n".to_string())?; n_md += 1;
    md("DEVELOPMENT.md", "Development", format!("Detected stacks: {}\n", if r.frameworks.is_empty() { "(none)".into() } else { r.frameworks.join(", ") }))?; n_md += 1;
    let mut iss = String::new();
    for i in &snap.known_issues.data {
        iss.push_str(&format!("- {}:{} [{}] {}", i.file, i.line, i.marker, i.text.lines().next().unwrap_or("")));
    }
    md("KNOWN_ISSUES.md", "Known issues", if iss.is_empty() { "(none found)\n".to_string() } else { iss })?; n_md += 1;
    Ok((n_json, n_md))
}

fn cmd_inject(config_path: &str, command: &[String]) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;

    let session_id = session::get_or_create_session(
        database.conn(),
        &aikd_core::platform::canonical_project(&cfg.scan.include_paths),
    )
    .map(|s| s.id)
    .unwrap_or_default();

    // Recall recent context
    let context = session::recall(database.conn(), &session_id, "", 5)
        .unwrap_or_default()
        .iter()
        .map(|c| format!("[{}]: {}", c.role, c.content.chars().take(200).collect::<String>()))
        .collect::<Vec<_>>()
        .join("\n");

    if !context.is_empty() {
        eprintln!(
            "[AIKD] Injected {} context messages",
            context.lines().count()
        );
    }

    let program = &command[0];
    let args = &command[1..];

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(ref mut stdin) = child.stdin {
        if !context.is_empty() {
            writeln!(stdin, "# AIKD Context:\n{context}")?;
        }
    }

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line?;
            println!("{line}");
        }
    }

    let status = child.wait()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

async fn cmd_benchmark(config_path: &str) -> Result<()> {
    let runner = aikd_benchmark::BenchmarkRunner::new(Some(config_path))?;

    println!("AIKD Benchmark Suite");
    println!("====================");
    println!("Resource limit: CPU <=50%, RAM <=50%\n");

    let status = runner.resource_status();
    println!(
        "System: {} cores, {:.1} GB RAM\n",
        num_cpus::get(),
        status.mem_total_mb as f64 / 1024.0
    );

    let results = runner.run_all().await;

    println!("\n========================================");
    println!("  AIKD Benchmark Results");
    println!("========================================\n");

    let mut passed = 0;
    let mut failed = 0;

    for (i, result) in results.iter().enumerate() {
        println!("{:2}. {}", i + 1, result);
        if result.success {
            passed += 1;
        } else {
            failed += 1;
        }
    }

    let total_duration: std::time::Duration = results.iter().map(|r| r.duration).sum();
    println!("\n----------------------------------------");
    println!("  Summary");
    println!("----------------------------------------");
    println!("  Passed:  {}/{}", passed, passed + failed);
    println!("  Failed:  {failed}");
    println!("  Total:   {:.2}s", total_duration.as_secs_f64());

    let final_status = runner.resource_status();
    println!(
        "  Peak:    CPU {:.1}%, RAM {:.1}% ({} MB)",
        final_status.cpu_percent, final_status.mem_percent, final_status.mem_used_mb
    );

    // Write JSON report
    let report = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "system": {
            "cpu_cores": num_cpus::get(),
            "ram_mb": final_status.mem_total_mb,
        },
        "results": results.iter().map(|r| serde_json::json!({
            "name": r.name,
            "success": r.success,
            "duration_ms": r.duration.as_millis(),
            "details": r.details,
            "error": r.error,
            "throughput": r.throughput,
        })).collect::<Vec<_>>(),
        "summary": {
            "passed": passed,
            "failed": failed,
            "total_ms": total_duration.as_millis(),
            "cpu_peak": final_status.cpu_percent,
            "ram_peak_percent": final_status.mem_percent,
        }
    });

    let report_path = "benchmark_report.json";
    std::fs::write(report_path, serde_json::to_string_pretty(&report)?)?;
    println!("\n  Report:  {report_path}");

    runner.stop();
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
