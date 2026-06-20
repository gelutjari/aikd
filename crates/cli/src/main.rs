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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
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
                "db_schema": 4,
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
        Commands::Inject { command } => cmd_inject(&cli.config, &command),
        Commands::Benchmark => cmd_benchmark(&cli.config).await,
        Commands::Session { action } => cmd_session(&cli.config, action, json_mode),
        Commands::Model { action } => cmd_model(&cli.config, action.unwrap_or(ModelAction::Status)),
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

    // Auto-download model
    let cfg = load_or_default(config_path);
    let model_dir = cfg.model_path();
    if !aikd_embedder::is_model_downloaded(&model_dir) {
        info!("  Downloading embedding model...");
        match aikd_embedder::download_model(&model_dir) {
            Ok(()) => info!("  Model downloaded ({})", embedder::MODEL_NAME),
            Err(e) => einfo!(
                "  Warning: Failed to download model: {}. Run: aikd model download",
                e
            ),
        }
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
    } else {
        // Write PID file for daemon management
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
                .args(["--config", config_path, "daemon", "--foreground"])
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
                .args(["--config", config_path, "daemon", "--foreground"])
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
    let start = std::time::Instant::now();

    if params.hybrid {
        let model_dir = cfg.model_path();
        if !embedder::is_model_downloaded(&model_dir) {
            eprintln!("[aikd] Model not downloaded. Run: aikd model download");
            return Ok(());
        }
        let mut model = embedder::create_model(&model_dir)?;
        let q_emb = model.embed(vec![query], None)?.remove(0);
        let vector_index = std::sync::Arc::new(aikd_indexer::VectorIndex::load_from_db(
            database.conn(),
            embedder::MODEL_NAME,
        )?);
        if vector_index.is_empty() {
            let tantivy_results = tantivy.search(query, params.limit, &filters)?;
            let results = enrich_with_line_numbers(database.conn(), &tantivy_results)?;
            print_results(query, &results, start.elapsed(), params.json);
            return Ok(());
        }
        let searcher =
            aikd_indexer::HybridSearcher::new(std::sync::Arc::new(tantivy), vector_index);
        let results = searcher.hybrid_search(query, &q_emb, params.limit, &filters, 60)?;
        let results = enrich_with_line_numbers(database.conn(), &results)?;
        print_results(query, &results, start.elapsed(), params.json);
    } else {
        let tantivy_results = tantivy.search(query, params.limit, &filters)?;
        let results = enrich_with_line_numbers(database.conn(), &tantivy_results)?;
        print_results(query, &results, start.elapsed(), params.json);
    }
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

fn cmd_embed(config_path: &str, _model: &str, _batch_size: usize) -> Result<()> {
    let cfg = load_or_default(config_path);
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
                &cfg.scan.include_paths.first().cloned().unwrap_or_default(),
            )?
            .id
        }
    };
    let conv = session::remember(database.conn(), &sid, role, content, &[])?;

    // Embed conversations if model is available
    let model_dir = cfg.model_path();
    if embedder::is_model_downloaded(&model_dir) {
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
            &conv.content[..conv.content.len().min(100)]
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
                &cfg.scan.include_paths.first().cloned().unwrap_or_default(),
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
                &c.content[..c.content.len().min(200)]
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

fn enrich_with_line_numbers(
    conn: &rusqlite::Connection,
    results: &[aikd_core::SearchResult],
) -> Result<Vec<aikd_core::SearchResult>> {
    let mut enriched = Vec::with_capacity(results.len());
    for r in results {
        let lines = conn
            .query_row(
                "SELECT line_start, line_end FROM chunks WHERE id=?1",
                rusqlite::params![r.chunk_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as usize,
                        row.get::<_, i64>(1)? as usize,
                    ))
                },
            )
            .unwrap_or((0, 0));
        enriched.push(aikd_core::SearchResult {
            chunk_id: r.chunk_id.clone(),
            file_path: r.file_path.clone(),
            heading_hierarchy: r.heading_hierarchy.clone(),
            heading_text: r.heading_text.clone(),
            content: r.content.clone(),
            line_start: lines.0,
            line_end: lines.1,
            score: r.score,
        });
    }
    Ok(enriched)
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
            database.conn().execute(
                "DELETE FROM conversations WHERE session_id = ?1",
                rusqlite::params![id],
            )?;
            database
                .conn()
                .execute("DELETE FROM sessions WHERE id = ?1", rusqlite::params![id])?;
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

fn cmd_inject(config_path: &str, command: &[String]) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;

    let session_id = session::get_or_create_session(
        database.conn(),
        &cfg.scan.include_paths.first().cloned().unwrap_or_default(),
    )
    .map(|s| s.id)
    .unwrap_or_default();

    // Recall recent context
    let context = session::recall(database.conn(), &session_id, "", 5)
        .unwrap_or_default()
        .iter()
        .map(|c| format!("[{}]: {}", c.role, &c.content[..c.content.len().min(200)]))
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
