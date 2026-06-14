use aikd_benchmark::{start_resource_monitor, BenchmarkRunner};
use clap::Parser;
use std::sync::{atomic::AtomicBool, Arc};
use tracing::info;

#[derive(Parser)]
#[command(name = "aikd-benchmark")]
#[command(version = "1.0.0")]
#[command(about = "AIKD Benchmark & Stress Test Suite")]
struct Cli {
    #[arg(short, long)]
    config: Option<String>,

    #[arg(long, default_value = "false")]
    skip_stress: bool,

    #[arg(long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        tracing_subscriber::EnvFilter::new("debug")
    } else {
        tracing_subscriber::EnvFilter::new("info")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    let stop_flag = Arc::new(AtomicBool::new(false));
    start_resource_monitor(stop_flag.clone());

    info!("AIKD Benchmark Suite");
    info!("====================");
    info!("Resource limit: CPU <=50%, RAM <=50%");
    info!("");

    let runner = BenchmarkRunner::new(cli.config.as_deref())?;
    let initial_status = runner.resource_status();
    info!(
        "System: {} cores, {:.1} GB RAM ({} MB used)",
        num_cpus::get(),
        initial_status.mem_total_mb as f64 / 1024.0,
        initial_status.mem_used_mb,
    );
    info!("");

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
    println!("  Failed:  {}", failed);
    println!("  Total:   {:.2}s", total_duration.as_secs_f64());

    let final_status = runner.resource_status();
    println!(
        "  Peak:    CPU {:.1}%, RAM {:.1}% ({} MB)",
        final_status.cpu_percent, final_status.mem_percent, final_status.mem_used_mb,
    );

    if failed > 0 {
        println!("\n  FAILED benchmarks:");
        for r in results.iter().filter(|r| !r.success) {
            println!(
                "    - {}: {}",
                r.name,
                r.error.as_deref().unwrap_or("unknown")
            );
        }
    }

    println!();
    runner.stop();
    stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
