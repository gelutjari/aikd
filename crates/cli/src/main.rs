use std::path::Path;
use clap::{Parser, Subcommand};
use anyhow::Result;

use aikd_core::{Config, SearchFilters, ResourceProfile};
use aikd_storage::Database;
use aikd_indexer::TantivyEngine;
use aikd_embedder as embedder;
use aikd_session as session;

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
    #[command(subcommand)]
    command: Commands,
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
        #[arg(long)]
        foreground: bool,
    },
    #[command(about = "[CLI] Scan and index files")]
    Scan {
        #[arg(short, long)]
        path: Option<String>,
    },
    #[command(about = "[CLI] Search the knowledge base")]
    Query {
        query: String,
        #[arg(short, long, default_value = "10")] limit: usize,
        #[arg(short, long)] path: Option<String>,
        #[arg(short = 'H', long)] heading: Option<String>,
        #[arg(long)] json: bool,
        #[arg(long)] hybrid: bool,
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
    let json_mode = cli.json;
    let quiet = cli.quiet;
    match cli.command {
        Commands::Init { path } => cmd_init(&cli.config, path.as_deref()),
        Commands::Daemon { foreground } => cmd_daemon(&cli.config, foreground).await,
        Commands::Scan { path } => cmd_scan(&cli.config, path.as_deref()),
        Commands::Query { query, limit, path, heading, json, hybrid } =>
            cmd_query(&cli.config, &query, limit, path.as_deref(), heading.as_deref(), json || json_mode, hybrid),
        Commands::Stats => cmd_stats(&cli.config),
        Commands::Export { output } => cmd_export(&cli.config, &output),
        Commands::Import { file } => cmd_import(&cli.config, &file),
        Commands::Embed { model, batch } => cmd_embed(&cli.config, &model, batch),
        Commands::Serve => cmd_serve(&cli.config).await,
        Commands::Watch { debounce } => cmd_watch(&cli.config, debounce).await,
        Commands::Remember { session, role, content } => cmd_remember(&cli.config, session.as_deref(), &role, &content, json_mode),
        Commands::Recall { query, session, limit } => cmd_recall(&cli.config, &query, session.as_deref(), limit, json_mode),
        Commands::Status => cmd_status(&cli.config, json_mode),
        Commands::Inject { command } => cmd_inject(&cli.config, &command),
        Commands::Benchmark => cmd_benchmark(&cli.config).await,
    }
}

fn cmd_init(config_path: &str, scan_path: Option<&str>) -> Result<()> {
    let expanded = shellexpand::tilde(config_path);
    if Path::new(expanded.as_ref()).exists() {
        eprintln!("Config already exists at {}", expanded);
    } else {
        let root = scan_path.map(std::path::PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let cfg = aikd_core::config::generate_smart_config(&root);
        cfg.save(config_path)?;
        eprintln!("Created config at {}", expanded);
    }

    // Auto-download model
    let cfg = load_or_default(config_path);
    let model_dir = cfg.model_path();
    if !aikd_embedder::is_model_downloaded(&model_dir) {
        eprintln!("Downloading embedding model...");
        match aikd_embedder::download_model(&model_dir) {
            Ok(()) => eprintln!("Model downloaded to {}", model_dir.display()),
            Err(e) => eprintln!("Warning: Failed to download model: {}. You can download manually later.", e),
        }
    } else {
        eprintln!("Model already downloaded at {}", model_dir.display());
    }

    // Generate shell hooks
    install_shell_hook(config_path);

    // Auto-register ke semua AI agent yang terinstall
    let aikd_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "aikd".to_string());

    eprintln!("\nDetecting AI agents...");
    let results = aikd_core::agents::detect_and_register(&aikd_path);

    if results.is_empty() {
        eprintln!("No AI agents detected. AIKD is available as CLI tool:");
        eprintln!("  aikd query \"keyword\" --json");
        eprintln!("  aikd scan");
        eprintln!("  aikd stats");
    } else {
        eprintln!("Registered AIKD as MCP server for:");
        for (name, success) in &results {
            if *success {
                eprintln!("  + {} - OK", name);
            } else {
                eprintln!("  + {} - FAILED", name);
            }
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
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(mcp_path.as_ref(), serde_json::to_string_pretty(&mcp_config).unwrap_or_default());
    eprintln!("\nMCP config: {}", mcp_path);

    // Output JSON summary for programmatic use
    let summary = serde_json::json!({
        "status": "ok",
        "config": expanded.to_string(),
        "model_downloaded": aikd_embedder::is_model_downloaded(&model_dir),
        "agents_registered": results.iter().filter(|(_, s)| *s).map(|(n, _)| n).collect::<Vec<_>>(),
        "cli_usage": "aikd query \"keyword\" --json",
        "mcp_config": mcp_path.to_string(),
    });
    println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());

    Ok(())
}

fn install_shell_hook(_config_path: &str) {
    let hook_script = format!(
        r#"# AIKD auto-start hook
aikd_auto_start() {{
    if [ -f ".aikd/config.yaml" ] || [ -f "$HOME/.aikd/config.yaml" ]; then
        if ! pgrep -f "aikd daemon" > /dev/null 2>&1; then
            aikd daemon --foreground &>/dev/null &
        fi
    fi
}}
cd() {{
    builtin cd "$@" && aikd_auto_start
}}
aikd_auto_start"#,
    );

    // Bash hook
    let bashrc = shellexpand::tilde("~/.bashrc");
    if Path::new(bashrc.as_ref()).exists() {
        let content = std::fs::read_to_string(bashrc.as_ref()).unwrap_or_default();
        if !content.contains("aikd_auto_start") {
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(bashrc.as_ref()) {
                use std::io::Write;
                let _ = writeln!(f, "\n{}", hook_script);
                println!("Shell hook installed to {}", bashrc);
            }
        }
    }

    // Zsh hook
    let zshrc = shellexpand::tilde("~/.zshrc");
    if Path::new(zshrc.as_ref()).exists() {
        let content = std::fs::read_to_string(zshrc.as_ref()).unwrap_or_default();
        if !content.contains("aikd_auto_start") {
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(zshrc.as_ref()) {
                use std::io::Write;
                let _ = writeln!(f, "\n{}", hook_script);
                println!("Shell hook installed to {}", zshrc);
            }
        }
    }

    // Write MCP config for AI assistants
    let mcp_config = serde_json::json!({
        "mcpServers": {
            "aikd": {
                "command": "aikd",
                "args": ["serve"],
                "env": {}
            }
        }
    });
    let mcp_path = shellexpand::tilde("~/.aikd/mcp.json");
    if let Some(parent) = Path::new(mcp_path.as_ref()).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(mcp_path.as_ref(), serde_json::to_string_pretty(&mcp_config).unwrap_or_default());
    println!("MCP config written to {}", mcp_path);
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
        let mcp_handle = tokio::spawn(async move {
            aikd_server::mcp::run_server(&config_path_owned2).await
        });

        tokio::select! {
            r = rest_handle => { let _ = r?; }
            r = mcp_handle => { let _ = r?; }
        }
    } else {
        println!("Starting AIKD daemon in background...");
        println!("Use 'aikd daemon --foreground' for interactive mode");
        Box::pin(cmd_daemon(config_path, true)).await?;
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

    println!("Scanning...");
    let progress = aikd_scanner::run_scan(&cfg, &database, &tantivy, &opts)?;
    println!("Indexed {} files, {} chunks in {:?}", progress.files_indexed, progress.chunks_created, progress.elapsed);
    if progress.files_skipped > 0 {
        println!("Skipped {} unchanged files", progress.files_skipped);
    }
    Ok(())
}

fn cmd_query(config_path: &str, query: &str, limit: usize, path_filter: Option<&str>, heading_filter: Option<&str>, json: bool, hybrid: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let tantivy = TantivyEngine::open(&cfg.tantivy_path())?;

    // Auto-scan if DB is empty
    let file_count: i64 = database.conn().query_row("SELECT COUNT(*) FROM files WHERE status='active'", [], |r| r.get(0)).unwrap_or(0);
    if file_count == 0 {
        if !json { eprintln!("[aikd] Database kosong, auto-scan..."); }
        let opts = aikd_scanner::ScanOptions::default();
        aikd_scanner::run_scan(&cfg, &database, &tantivy, &opts)?;
    }

    let filters = SearchFilters {
        path_contains: path_filter.map(String::from),
        heading_contains: heading_filter.map(String::from),
        ..Default::default()
    };
    let start = std::time::Instant::now();

    if hybrid {
        let kw_results = tantivy.search(query, limit * 2, &filters)?;
        let kw_ids: Vec<String> = kw_results.iter().map(|r| r.chunk_id.clone()).collect();
        let all_embs = embedder::load_all_embeddings(database.conn())?;
        if all_embs.is_empty() {
            let results = enrich_with_line_numbers(database.conn(), &kw_results)?;
            print_results(query, &results, start.elapsed(), json);
            return Ok(());
        }
        let q_emb = {
            let first = kw_results.first();
            match first {
                Some(r) => {
                    let emb = all_embs.iter().find(|(id, _)| id == &r.chunk_id).map(|(_, e)| e.clone());
                    emb.unwrap_or_else(|| vec![0.0; embedder::DIMENSIONS])
                }
                None => vec![0.0; embedder::DIMENSIONS],
            }
        };
        let vec_scored = embedder::vector_search(&q_emb, &all_embs, limit * 2);
        let vec_ids: Vec<String> = vec_scored.iter().map(|(id, _)| id.clone()).collect();
        let fused = aikd_core::fusion::reciprocal_rank_fusion(&kw_ids, &vec_ids, 60);
        let fused_ids: Vec<String> = fused.iter().take(limit).map(|(id, _)| id.clone()).collect();
        let results = load_chunks(database.conn(), &fused_ids, &filters)?;
        print_results(query, &results, start.elapsed(), json);
    } else {
        let tantivy_results = tantivy.search(query, limit, &filters)?;
        let results = enrich_with_line_numbers(database.conn(), &tantivy_results)?;
        print_results(query, &results, start.elapsed(), json);
    }
    Ok(())
}

fn cmd_export(config_path: &str, output: &str) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let expanded = shellexpand::tilde(output);
    let count = embedder::export_chunks_for_embedding(database.conn(), expanded.as_ref())?;
    println!("Exported {} chunks to {}", count, expanded);
    Ok(())
}

fn cmd_import(config_path: &str, file: &str) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let expanded = shellexpand::tilde(file);
    let count = embedder::import_embeddings_json(database.conn(), expanded.as_ref())?;
    println!("Imported {} embeddings", count);
    Ok(())
}

fn cmd_stats(config_path: &str) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let fc: i64 = database.conn().query_row("SELECT COUNT(*) FROM files WHERE status='active'", [], |r| r.get(0))?;
    let cc: i64 = database.conn().query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let ts: i64 = database.conn().query_row("SELECT COALESCE(SUM(size),0) FROM files WHERE status='active'", [], |r| r.get(0))?;
    let ec: i64 = database.conn().query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0)).unwrap_or(0);
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
    println!("{}", serde_json::to_string_pretty(&stats).unwrap_or_default());
    Ok(())
}

fn cmd_embed(config_path: &str, _model: &str, batch_size: usize) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let count: i64 = database.conn().query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    if count == 0 {
        eprintln!("[aikd] No chunks to embed. Run: aikd scan first");
        return Ok(());
    }
    let existing: i64 = database.conn().query_row("SELECT COUNT(*) FROM embeddings WHERE model = ?1", rusqlite::params![embedder::MODEL_NAME], |r| r.get(0)).unwrap_or(0);
    let remaining = count - existing;
    if remaining <= 0 {
        eprintln!("[aikd] All {} chunks already embedded.", count);
        return Ok(());
    }
    let model_dir = cfg.model_path();
    eprintln!("[aikd] {} chunks total, {} already embedded, {} to process", count, existing, remaining);
    eprintln!("[aikd] Loading model...");
    let start = std::time::Instant::now();
    let imported = embedder::embed_and_store(database.conn(), &model_dir, batch_size)?;
    eprintln!("[aikd] {} embeddings stored in {:.1}s", imported, start.elapsed().as_secs_f64());
    eprintln!("[aikd] Hybrid search now available.");
    // JSON output
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "status": "ok",
        "embeddings_created": imported,
        "total_chunks": count,
        "elapsed_ms": start.elapsed().as_millis(),
    })).unwrap_or_default());
    Ok(())
}

async fn cmd_serve(config_path: &str) -> Result<()> {
    aikd_server::run_mcp_server(config_path).await
}

async fn cmd_watch(config_path: &str, debounce: u64) -> Result<()> {
    aikd_watcher::run_watcher(config_path, debounce).await
}

fn cmd_remember(config_path: &str, session_id: Option<&str>, role: &str, content: &str, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let sid = match session_id {
        Some(id) => id.to_string(),
        None => session::get_or_create_session(database.conn(), &cfg.scan.include_paths.first().cloned().unwrap_or_default())?.id,
    };
    let conv = session::remember(database.conn(), &sid, role, content, &[])?;
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "conversation_id": conv.id,
            "session_id": conv.session_id,
            "role": conv.role,
        })).unwrap_or_default());
    } else {
        println!("Remembered in session {}: [{}] {}", conv.session_id, conv.role, &conv.content[..conv.content.len().min(100)]);
    }
    Ok(())
}

fn cmd_recall(config_path: &str, query: &str, session_id: Option<&str>, limit: usize, json: bool) -> Result<()> {
    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;
    let sid = match session_id {
        Some(id) => id.to_string(),
        None => session::get_or_create_session(database.conn(), &cfg.scan.include_paths.first().cloned().unwrap_or_default())?.id,
    };
    let convs = session::recall(database.conn(), &sid, query, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&convs).unwrap_or_default());
    } else if convs.is_empty() {
        println!("No matching conversations found.");
    } else {
        println!("{} results for '{}':\n", convs.len(), query);
        for (i, c) in convs.iter().enumerate() {
            println!("{}. [{}] {}: {}", i + 1, c.created_at, c.role,
                &c.content[..c.content.len().min(200)]);
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
        println!("{}", serde_json::to_string_pretty(&status).unwrap_or_default());
    } else {
        println!("=== AIKD Daemon Status ===");
        println!("CPU Cores:   {}", profile.cpu_cores);
        println!("RAM:         {:.1} GB", profile.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0));
        println!("GPU:         {}", profile.has_gpu);
        println!("Embedding:   {}", if profile.embedding_enabled { "ON" } else { "OFF" });
        println!("Batch Size:  {}", profile.batch_size);
        println!("Parallelism: {}", profile.parallelism);
        println!("HNSW M:      {}", profile.hnsw_m);
        println!("REST Port:   {}", cfg.server.rest_port);
    }
    Ok(())
}

fn load_chunks(conn: &rusqlite::Connection, ids: &[String], filters: &SearchFilters) -> Result<Vec<aikd_core::SearchResult>> {
    let mut results = Vec::new();
    for id in ids {
        let row = conn.query_row(
            "SELECT c.id,f.path,c.heading_hierarchy,c.heading_text,c.content,c.line_start,c.line_end FROM chunks c JOIN files f ON c.file_id=f.id WHERE c.id=?1",
            rusqlite::params![id],
            |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?, r.get::<_,String>(4)?, r.get::<_,i64>(5)? as usize, r.get::<_,i64>(6)? as usize)),
        );
        match row {
            Ok((cid, fp, hj, ht, co, ls, le)) => {
                if let Some(ref p) = filters.path_contains { if !fp.contains(p.as_str()) { continue; } }
                if let Some(ref h) = filters.heading_contains { if !ht.contains(h.as_str()) { continue; } }
                let hier: Vec<String> = serde_json::from_str(&hj).unwrap_or_default();
                results.push(aikd_core::SearchResult { chunk_id: cid, file_path: fp, heading_hierarchy: hier.join(" > "), heading_text: ht, content: co, line_start: ls, line_end: le, score: 0.0 });
            }
            Err(_) => continue,
        }
    }
    Ok(results)
}

fn enrich_with_line_numbers(conn: &rusqlite::Connection, results: &[aikd_core::SearchResult]) -> Result<Vec<aikd_core::SearchResult>> {
    let mut enriched = Vec::with_capacity(results.len());
    for r in results {
        let lines = conn.query_row(
            "SELECT line_start, line_end FROM chunks WHERE id=?1",
            rusqlite::params![r.chunk_id],
            |row| Ok((row.get::<_, i64>(0)? as usize, row.get::<_, i64>(1)? as usize)),
        ).unwrap_or((0, 0));
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

fn print_results(query: &str, results: &[aikd_core::SearchResult], elapsed: std::time::Duration, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(results).unwrap_or_default());
        return;
    }
    if results.is_empty() { println!("No results for: {}", query); return; }
    println!("{} results for '{}' ({:?}):\n", results.len(), query, elapsed);
    for (i, r) in results.iter().enumerate() {
        println!("{}. {}", i + 1, r.file_path);
        if !r.heading_text.is_empty() { println!("   Heading: {}", r.heading_hierarchy); }
        if r.line_start > 0 { println!("   Lines: {}-{}", r.line_start, r.line_end); }
        if r.score > 0.0 { println!("   Score: {:.3}", r.score); }
        let preview = if r.content.chars().count() > 200 {
            let end = r.content.char_indices().nth(200).map(|(i, _)| i).unwrap_or(r.content.len());
            format!("{}...", &r.content[..end])
        } else { r.content.clone() };
        println!("   {}", preview.replace('\n', "\n   "));
        println!();
    }
}

fn load_or_default(p: &str) -> Config {
    Config::load(p).unwrap_or_else(|_| { log::warn!("Using defaults"); Config::default() })
}

fn cmd_inject(config_path: &str, command: &[String]) -> Result<()> {
    use std::process::{Command, Stdio};
    use std::io::{BufRead, BufReader, Write};

    let cfg = load_or_default(config_path);
    let database = Database::open(&cfg.db_path())?;

    let session_id = session::get_or_create_session(database.conn(), &cfg.scan.include_paths.first().cloned().unwrap_or_default())
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
        eprintln!("[AIKD] Injected {} context messages", context.lines().count());
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
            writeln!(stdin, "# AIKD Context:\n{}", context)?;
        }
    }

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line?;
            println!("{}", line);
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
    println!("System: {} cores, {:.1} GB RAM\n", num_cpus::get(), status.mem_total_mb as f64 / 1024.0);

    let results = runner.run_all().await;

    println!("\n========================================");
    println!("  AIKD Benchmark Results");
    println!("========================================\n");

    let mut passed = 0;
    let mut failed = 0;

    for (i, result) in results.iter().enumerate() {
        println!("{:2}. {}", i + 1, result);
        if result.success { passed += 1; } else { failed += 1; }
    }

    let total_duration: std::time::Duration = results.iter().map(|r| r.duration).sum();
    println!("\n----------------------------------------");
    println!("  Summary");
    println!("----------------------------------------");
    println!("  Passed:  {}/{}", passed, passed + failed);
    println!("  Failed:  {}", failed);
    println!("  Total:   {:.2}s", total_duration.as_secs_f64());

    let final_status = runner.resource_status();
    println!("  Peak:    CPU {:.1}%, RAM {:.1}% ({} MB)",
        final_status.cpu_percent, final_status.mem_percent, final_status.mem_used_mb);

    // Write JSON report
    let report = serde_json::json!({
        "version": "1.1.0",
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
    println!("\n  Report:  {}", report_path);

    runner.stop();
    if failed > 0 { std::process::exit(1); }
    Ok(())
}
