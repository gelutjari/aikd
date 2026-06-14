use aikd_core::{Chunk, Config, SearchFilters};
use aikd_indexer::TantivyEngine;
use aikd_storage::Database;
use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::{
    fs,
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use sysinfo::System;
use tracing::{info, warn};

const MAX_CPU_PERCENT: f32 = 50.0;
const MAX_MEM_PERCENT: f64 = 50.0;

#[derive(Debug, Clone)]
pub struct BenchResult {
    pub name: String,
    pub success: bool,
    pub duration: Duration,
    pub details: String,
    pub error: Option<String>,
    pub throughput: Option<f64>,
}

impl std::fmt::Display for BenchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.success { "PASS" } else { "FAIL" };
        let ms = self.duration.as_secs_f64() * 1000.0;
        let tp = self
            .throughput
            .map(|t| format!(" ({:.1} ops/s)", t))
            .unwrap_or_default();
        write!(
            f,
            "[{}] {} {:.1}ms{} — {}",
            status, self.name, ms, tp, self.details
        )?;
        if let Some(ref e) = self.error {
            write!(f, " | ERROR: {}", e)?;
        }
        Ok(())
    }
}

pub struct ResourceMonitor {
    sys: std::sync::Mutex<System>,
    cpu_cores: usize,
    total_mem: u64,
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let cpu_cores = sys.cpus().len();
        let total_mem = sys.total_memory();
        Self {
            sys: std::sync::Mutex::new(sys),
            cpu_cores,
            total_mem,
        }
    }

    pub fn check(&self) -> Result<ResourceStatus> {
        let mut sys = self.sys.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        sys.refresh_all();

        let cpu_usage: f32 =
            sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / self.cpu_cores as f32;
        let mem_usage = sys.used_memory();
        let mem_percent = (mem_usage as f64 / self.total_mem as f64) * 100.0;

        Ok(ResourceStatus {
            cpu_percent: cpu_usage,
            mem_percent: mem_percent as f32,
            mem_used_mb: mem_usage / (1024 * 1024),
            mem_total_mb: self.total_mem / (1024 * 1024),
        })
    }

    pub fn throttle_if_needed(&self) {
        loop {
            match self.check() {
                Ok(status) => {
                    if status.cpu_percent <= MAX_CPU_PERCENT
                        && status.mem_percent as f64 <= MAX_MEM_PERCENT
                    {
                        break;
                    }
                    warn!(
                        "Throttling: CPU {:.1}% (limit {}%), RAM {:.1}% (limit {}%)",
                        status.cpu_percent, MAX_CPU_PERCENT, status.mem_percent, MAX_MEM_PERCENT
                    );
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => {
                    warn!("Resource check failed: {}", e);
                    break;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResourceStatus {
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
}

pub struct BenchmarkRunner {
    config: Config,
    db: Database,
    tantivy: TantivyEngine,
    resource_monitor: Arc<ResourceMonitor>,
    stop_flag: Arc<AtomicBool>,
    temp_dir: tempfile::TempDir,
}

impl BenchmarkRunner {
    pub fn new(config_path: Option<&str>) -> Result<Self> {
        let config = match config_path {
            Some(p) => Config::load(p)?,
            None => Config::default(),
        };

        let temp_dir = tempfile::tempdir()?;
        let db_path = temp_dir.path().join("bench.db");
        let tantivy_path = temp_dir.path().join("tantivy");

        let db = Database::open(&db_path)?;
        let tantivy = TantivyEngine::open(&tantivy_path)?;
        let resource_monitor = Arc::new(ResourceMonitor::new());
        let stop_flag = Arc::new(AtomicBool::new(false));

        Ok(Self {
            config,
            db,
            tantivy,
            resource_monitor,
            stop_flag,
            temp_dir,
        })
    }

    pub fn temp_path(&self) -> &Path {
        self.temp_dir.path()
    }

    pub async fn run_all(&self) -> Vec<BenchResult> {
        let mut results = Vec::new();

        info!("Preparing test data...");
        if let Err(e) = self.prepare_test_data() {
            results.push(BenchResult {
                name: "Setup".to_string(),
                success: false,
                duration: Duration::ZERO,
                details: "Failed to prepare test data".to_string(),
                error: Some(format!("{:?}", e)),
                throughput: None,
            });
            return results;
        }

        results.push(self.bench_indexing().await);
        results.push(self.bench_search_bm25().await);
        results.push(self.bench_search_hybrid().await);
        results.push(self.bench_embedding().await);
        results.push(self.bench_incremental_reindex().await);
        results.push(self.bench_concurrent_search().await);
        results.push(self.bench_rest_stress().await);
        results.push(self.bench_chunking_throughput().await);

        results
    }

    fn prepare_test_data(&self) -> Result<usize> {
        let data_dir = self.temp_dir.path().join("test_data");
        fs::create_dir_all(&data_dir)?;

        let topics = [
            "Rust programming language",
            "Web development with TypeScript",
            "Machine learning fundamentals",
            "Database design patterns",
            "API architecture best practices",
            "DevOps and CI/CD pipelines",
            "Security and authentication",
            "Performance optimization",
            "Data structures and algorithms",
            "Cloud infrastructure",
        ];

        let mut count = 0;
        for i in 0..1000 {
            let path = data_dir.join(format!("doc_{:04}.md", i));
            let topic = topics[i % topics.len()];
            let content = format!(
                "# Document {}: {}\n\n\
                 ## Overview\n\n\
                 This document covers {} in detail.\n\n\
                 ## Key Concepts\n\n\
                 - Concept A: fundamental approach to {}\n\
                 - Concept B: advanced techniques\n\
                 - Concept C: real-world applications\n\n\
                 ## Code Example\n\n\
                 ```rust\n\
                 fn example_{}() {{\n\
                     println!(\"Hello from document {}\");\n\
                 }}\n\
                 ```\n\n\
                 ## Summary\n\n\
                 This concludes document {} about {}.",
                i, topic, topic, topic, i, i, i, topic
            );
            let mut f = fs::File::create(&path)?;
            writeln!(f, "{}", content)?;
            count += 1;
        }

        info!("Created {} test files", count);
        Ok(count)
    }

    pub async fn bench_indexing(&self) -> BenchResult {
        let name = "Indexing (1000 files)";
        let start = Instant::now();

        let data_dir = self.temp_dir.path().join("test_data");

        let result = (|| -> Result<(usize, usize)> {
            self.resource_monitor.throttle_if_needed();

            let mut files = Vec::new();
            for entry in walkdir::WalkDir::new(&data_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                files.push(entry.into_path());
            }

            let cfg = &self.config;
            let indexed: Vec<(String, Vec<Chunk>)> = files
                .par_iter()
                .filter_map(|path| {
                    let ps = path.to_string_lossy().to_string();
                    let content = fs::read_to_string(path).ok()?;
                    let chunks = aikd_chunker::chunk_file(
                        &ps,
                        &content,
                        cfg.chunk.max_tokens,
                        cfg.chunk.min_tokens,
                    );
                    Some((ps, chunks))
                })
                .collect();

            let tx = self.db.begin_transaction()?;
            for (ps, chunks) in &indexed {
                let size = fs::metadata(ps).map(|m| m.len()).unwrap_or(0);
                let now = chrono::Utc::now().to_rfc3339();
                tx.conn().execute(
                    "INSERT OR REPLACE INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1,?2,?3,?4,'active','')",
                    rusqlite::params![ps, size as i64, now, now],
                )?;
                let fid: i64 = tx.conn().query_row(
                    "SELECT id FROM files WHERE path=?1",
                    rusqlite::params![ps],
                    |r| r.get(0),
                )?;
                for c in chunks {
                    let hj = serde_json::to_string(&c.heading_hierarchy).unwrap_or_default();
                    let mj = serde_json::to_string(&c.metadata).unwrap_or_default();
                    tx.conn().execute(
                        "INSERT INTO chunks (id,file_id,chunk_index,heading_hierarchy,heading_level,heading_text,line_start,line_end,content,metadata_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                        rusqlite::params![c.id, fid, c.chunk_index as i64, hj, c.heading_level as i64, c.heading_text, c.line_start as i64, c.line_end as i64, c.content, mj, now, now],
                    )?;
                }
            }
            tx.commit()?;

            let tc: Vec<(String, String, String, String)> = indexed
                .iter()
                .flat_map(|(p, cs)| {
                    cs.iter().map(move |c| {
                        (
                            c.id.clone(),
                            p.clone(),
                            c.heading_hierarchy_str(),
                            c.content.clone(),
                        )
                    })
                })
                .collect();
            self.tantivy.clear()?;
            self.tantivy.index_chunks(&tc)?;

            let file_count = indexed.len();
            let chunk_count: usize = indexed.iter().map(|(_, c)| c.len()).sum();
            Ok((file_count, chunk_count))
        })();

        let elapsed = start.elapsed();
        match result {
            Ok((files, chunks)) => BenchResult {
                name: name.to_string(),
                success: true,
                duration: elapsed,
                details: format!("{} files, {} chunks", files, chunks),
                error: None,
                throughput: Some(files as f64 / elapsed.as_secs_f64()),
            },
            Err(e) => BenchResult {
                name: name.to_string(),
                success: false,
                duration: elapsed,
                details: "Indexing failed".to_string(),
                error: Some(format!("{:?}", e)),
                throughput: None,
            },
        }
    }

    pub async fn bench_search_bm25(&self) -> BenchResult {
        let name = "BM25 Search (100 queries)";
        let start = Instant::now();

        let queries: Vec<String> = (0..100)
            .map(|i| {
                let topics = [
                    "Rust",
                    "TypeScript",
                    "machine learning",
                    "database",
                    "API",
                    "DevOps",
                    "security",
                    "performance",
                    "algorithms",
                    "cloud",
                ];
                format!("{} {}", topics[i % topics.len()], i / topics.len())
            })
            .collect();

        let filters = SearchFilters::default();
        let mut errors = Vec::new();
        let mut total_latency = Duration::ZERO;
        let mut total_results = 0usize;

        for q in &queries {
            self.resource_monitor.throttle_if_needed();
            let t0 = Instant::now();
            match self.tantivy.search(q, 10, &filters) {
                Ok(results) => {
                    total_latency += t0.elapsed();
                    total_results += results.len();
                }
                Err(e) => {
                    errors.push(format!("'{}': {}", q, e));
                }
            }
        }

        let elapsed = start.elapsed();
        let avg_latency = if queries.is_empty() {
            Duration::ZERO
        } else {
            total_latency / queries.len() as u32
        };

        BenchResult {
            name: name.to_string(),
            success: errors.is_empty(),
            duration: elapsed,
            details: format!(
                "{} queries, avg {:.2}ms, {} total results, {} errors",
                queries.len(),
                avg_latency.as_secs_f64() * 1000.0,
                total_results,
                errors.len()
            ),
            error: if errors.is_empty() {
                None
            } else {
                Some(errors.join("; "))
            },
            throughput: Some(queries.len() as f64 / elapsed.as_secs_f64()),
        }
    }

    pub async fn bench_search_hybrid(&self) -> BenchResult {
        let name = "Hybrid Search (50 queries)";
        let start = Instant::now();

        let queries: Vec<String> = (0..50)
            .map(|i| format!("programming concept {}", i))
            .collect();

        let filters = SearchFilters::default();
        let mut errors = Vec::new();
        let mut total_latency = Duration::ZERO;

        for q in &queries {
            self.resource_monitor.throttle_if_needed();
            let t0 = Instant::now();

            let kw_results = match self.tantivy.search(q, 20, &filters) {
                Ok(r) => r,
                Err(e) => {
                    errors.push(format!("BM25 '{}': {}", q, e));
                    continue;
                }
            };

            let all_embs = match aikd_embedder::load_all_embeddings(self.db.conn()) {
                Ok(e) => e,
                Err(e) => {
                    errors.push(format!("Load embs: {}", e));
                    continue;
                }
            };

            if all_embs.is_empty() {
                total_latency += t0.elapsed();
                continue;
            }

            let kw_ids: Vec<String> = kw_results.iter().map(|r| r.chunk_id.clone()).collect();
            let q_emb = kw_results
                .first()
                .and_then(|r| {
                    all_embs
                        .iter()
                        .find(|(id, _)| id == &r.chunk_id)
                        .map(|(_, e)| e.clone())
                })
                .unwrap_or_else(|| vec![0.0; aikd_embedder::DIMENSIONS]);

            let vec_scored = aikd_embedder::vector_search(&q_emb, &all_embs, 20);
            let vec_ids: Vec<String> = vec_scored.iter().map(|(id, _)| id.clone()).collect();
            let _fused = aikd_embedder::reciprocal_rank_fusion(&kw_ids, &vec_ids, 60);

            total_latency += t0.elapsed();
        }

        let elapsed = start.elapsed();
        let avg_latency = if queries.is_empty() {
            Duration::ZERO
        } else {
            total_latency / queries.len() as u32
        };

        BenchResult {
            name: name.to_string(),
            success: errors.is_empty(),
            duration: elapsed,
            details: format!(
                "{} queries, avg {:.2}ms, {} errors",
                queries.len(),
                avg_latency.as_secs_f64() * 1000.0,
                errors.len()
            ),
            error: if errors.is_empty() {
                None
            } else {
                Some(errors.join("; "))
            },
            throughput: Some(queries.len() as f64 / elapsed.as_secs_f64()),
        }
    }

    pub async fn bench_incremental_reindex(&self) -> BenchResult {
        let name = "Incremental Re-index (100 modified files)";
        let start = Instant::now();

        let result = (|| -> Result<usize> {
            self.resource_monitor.throttle_if_needed();

            let data_dir = self.temp_dir.path().join("test_data");
            let mut modified = 0;

            for i in 0..100 {
                let path = data_dir.join(format!("doc_{:04}.md", i));
                let content = format!(
                    "# MODIFIED Document {}\n\nThis content has been updated for incremental indexing test.\n\n## New Section\n\nAdditional content here.",
                    i
                );
                fs::write(&path, content)?;
                modified += 1;
            }

            let now = chrono::Utc::now().to_rfc3339();
            let tx = self.db.begin_transaction()?;
            for i in 0..100 {
                let ps = data_dir
                    .join(format!("doc_{:04}.md", i))
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(&ps)?;
                let chunks = aikd_chunker::chunk_file(&ps, &content, 1000, 50);

                if let Ok(old_fid) = tx.conn().query_row::<i64, _, _>(
                    "SELECT id FROM files WHERE path=?1",
                    rusqlite::params![ps],
                    |r| r.get(0),
                ) {
                    let _ = tx.conn().execute("DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)", rusqlite::params![old_fid]);
                    let _ = tx.conn().execute(
                        "DELETE FROM chunks WHERE file_id=?1",
                        rusqlite::params![old_fid],
                    );
                    let _ = tx
                        .conn()
                        .execute("DELETE FROM files WHERE id=?1", rusqlite::params![old_fid]);
                }

                let size = fs::metadata(&ps).map(|m| m.len()).unwrap_or(0);
                tx.conn().execute(
                    "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1,?2,?3,?4,'active','')",
                    rusqlite::params![ps, size as i64, now, now],
                )?;
                let fid: i64 = tx.conn().query_row(
                    "SELECT id FROM files WHERE path=?1",
                    rusqlite::params![ps],
                    |r| r.get(0),
                )?;
                for c in &chunks {
                    let hj = serde_json::to_string(&c.heading_hierarchy).unwrap_or_default();
                    let mj = serde_json::to_string(&c.metadata).unwrap_or_default();
                    tx.conn().execute(
                        "INSERT INTO chunks (id,file_id,chunk_index,heading_hierarchy,heading_level,heading_text,line_start,line_end,content,metadata_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                        rusqlite::params![c.id, fid, c.chunk_index as i64, hj, c.heading_level as i64, c.heading_text, c.line_start as i64, c.line_end as i64, c.content, mj, now, now],
                    )?;
                }
            }
            tx.commit()?;

            let tc: Vec<(String, String, String, String)> = (0..100)
                .filter_map(|i| {
                    let ps = data_dir.join(format!("doc_{:04}.md", i));
                    let content = fs::read_to_string(&ps).ok()?;
                    let chunks = aikd_chunker::chunk_file(ps.to_str()?, &content, 1000, 50);
                    Some(chunks.into_iter().map(move |c| {
                        let p = ps.to_string_lossy().to_string();
                        (c.id.clone(), p, c.heading_hierarchy_str(), c.content)
                    }))
                })
                .flatten()
                .collect();
            self.tantivy.index_chunks(&tc)?;

            Ok(modified)
        })();

        let elapsed = start.elapsed();
        match result {
            Ok(count) => BenchResult {
                name: name.to_string(),
                success: true,
                duration: elapsed,
                details: format!("{} files re-indexed", count),
                error: None,
                throughput: Some(count as f64 / elapsed.as_secs_f64()),
            },
            Err(e) => BenchResult {
                name: name.to_string(),
                success: false,
                duration: elapsed,
                details: "Incremental re-index failed".to_string(),
                error: Some(format!("{:?}", e)),
                throughput: None,
            },
        }
    }

    pub async fn bench_concurrent_search(&self) -> BenchResult {
        let name = "Concurrent Search (10 threads x 50 queries)";
        let start = Instant::now();

        let queries: Vec<String> = (0..500).map(|i| format!("test query {}", i)).collect();

        let tantivy_ref = &self.tantivy;
        let filters = SearchFilters::default();

        let errors: Vec<String> = queries
            .par_iter()
            .filter_map(|q| match tantivy_ref.search(q, 5, &filters) {
                Ok(_) => None,
                Err(e) => Some(format!("'{}': {}", q, e)),
            })
            .collect();

        let elapsed = start.elapsed();
        let total = queries.len();
        let ok = total - errors.len();

        BenchResult {
            name: name.to_string(),
            success: errors.is_empty(),
            duration: elapsed,
            details: format!("{}/{} queries succeeded", ok, total),
            error: if errors.is_empty() {
                None
            } else {
                Some(format!("{} errors", errors.len()))
            },
            throughput: Some(total as f64 / elapsed.as_secs_f64()),
        }
    }

    pub async fn bench_chunking_throughput(&self) -> BenchResult {
        let name = "Chunking Throughput (1000 files)";
        let start = Instant::now();

        let data_dir = self.temp_dir.path().join("test_data");
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(&data_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            files.push(entry.into_path());
        }

        let cfg = &self.config;
        let chunk_count: usize = files
            .par_iter()
            .filter_map(|path| {
                let content = fs::read_to_string(path).ok()?;
                let chunks = aikd_chunker::chunk_file(
                    &path.to_string_lossy(),
                    &content,
                    cfg.chunk.max_tokens,
                    cfg.chunk.min_tokens,
                );
                Some(chunks.len())
            })
            .sum();

        let elapsed = start.elapsed();
        BenchResult {
            name: name.to_string(),
            success: true,
            duration: elapsed,
            details: format!("{} files → {} chunks", files.len(), chunk_count),
            error: None,
            throughput: Some(files.len() as f64 / elapsed.as_secs_f64()),
        }
    }

    pub fn resource_status(&self) -> ResourceStatus {
        self.resource_monitor.check().unwrap_or(ResourceStatus {
            cpu_percent: 0.0,
            mem_percent: 0.0,
            mem_used_mb: 0,
            mem_total_mb: 0,
        })
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    pub async fn bench_embedding(&self) -> BenchResult {
        let name = "Embedding (500 chunks)";
        let start = Instant::now();

        let model_dir = self.config.model_path();
        if !aikd_embedder::is_model_downloaded(&model_dir) {
            return BenchResult {
                name: name.to_string(),
                success: true,
                duration: Duration::ZERO,
                details: "Skipped — model not downloaded".to_string(),
                error: None,
                throughput: None,
            };
        }

        // Insert 500 test chunks into DB
        let tx = match self.db.begin_transaction() {
            Ok(tx) => tx,
            Err(e) => {
                return BenchResult {
                    name: name.to_string(),
                    success: false,
                    duration: start.elapsed(),
                    details: "Failed to start transaction".to_string(),
                    error: Some(format!("{:?}", e)),
                    throughput: None,
                }
            }
        };

        let now = chrono::Utc::now().to_rfc3339();
        let _ = tx.conn().execute(
            "INSERT OR IGNORE INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('__bench__', 0, ?1, ?1, 'active', '')",
            rusqlite::params![now],
        );
        let fid: i64 = tx
            .conn()
            .query_row("SELECT id FROM files WHERE path='__bench__'", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);

        for i in 0..500 {
            let content = format!("This is benchmark chunk number {} with enough content to be meaningful for embedding generation.", i);
            let id = format!("bench-{}", i);
            let _ = tx.conn().execute(
                "INSERT OR IGNORE INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, metadata_json, created_at, updated_at) VALUES (?1,?2,?3,'[]',0,'',0,0,?4,'{}',?5,?5)",
                rusqlite::params![id, fid, i, content, now],
            );
        }
        let _ = tx.commit();

        // Run embedding
        let batch_size = self
            .config
            .embedding
            .batch_size
            .parse::<usize>()
            .unwrap_or(32);
        let result = aikd_embedder::embed_and_store(self.db.conn(), &model_dir, batch_size);

        let elapsed = start.elapsed();
        match result {
            Ok(count) => BenchResult {
                name: name.to_string(),
                success: true,
                duration: elapsed,
                details: format!("{} chunks embedded", count),
                error: None,
                throughput: Some(count as f64 / elapsed.as_secs_f64()),
            },
            Err(e) => BenchResult {
                name: name.to_string(),
                success: false,
                duration: elapsed,
                details: "Embedding failed".to_string(),
                error: Some(format!("{:?}", e)),
                throughput: None,
            },
        }
    }

    pub async fn bench_rest_stress(&self) -> BenchResult {
        let name = "REST API Stress (100 requests)";
        let start = Instant::now();

        let port = self.config.server.rest_port;
        let base_url = format!("http://127.0.0.1:{}", port);
        let token = self.config.server.auth_token.clone().unwrap_or_default();

        // Check if server is running
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();

        let health_check = client
            .get(format!("{}/api/stats", base_url))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await;

        if health_check.is_err() {
            return BenchResult {
                name: name.to_string(),
                success: true,
                duration: start.elapsed(),
                details: "Skipped — REST server not running".to_string(),
                error: None,
                throughput: None,
            };
        }

        let mut handles = Vec::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(20));

        for i in 0..100 {
            let client = client.clone();
            let url = format!("{}/api/query?q=test+{}&limit=5", base_url, i);
            let token = token.clone();
            let sem = sem.clone();
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send()
                    .await
            }));
        }

        let mut ok = 0;
        let mut fails = Vec::new();
        for (i, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(resp)) if resp.status().is_success() => ok += 1,
                Ok(Ok(resp)) => fails.push(format!("{}: HTTP {}", i, resp.status())),
                Ok(Err(e)) => fails.push(format!("{}: {}", i, e)),
                Err(e) => fails.push(format!("{}: join error: {}", i, e)),
            }
        }

        let elapsed = start.elapsed();
        BenchResult {
            name: name.to_string(),
            success: fails.is_empty(),
            duration: elapsed,
            details: format!("{}/100 requests succeeded, {} failed", ok, fails.len()),
            error: if fails.is_empty() {
                None
            } else {
                Some(fails.join("; "))
            },
            throughput: Some(100.0 / elapsed.as_secs_f64()),
        }
    }
}

pub fn start_resource_monitor(stop_flag: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let monitor = ResourceMonitor::new();
        loop {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            match monitor.check() {
                Ok(status) => {
                    if status.cpu_percent > MAX_CPU_PERCENT
                        || status.mem_percent > MAX_MEM_PERCENT as f32
                    {
                        warn!(
                            "Resource high: CPU {:.1}%, RAM {:.1}%",
                            status.cpu_percent, status.mem_percent
                        );
                    }
                }
                Err(e) => {
                    warn!("Resource monitor error: {}", e);
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_monitor() {
        let monitor = ResourceMonitor::new();
        let status = monitor.check().unwrap();
        assert!(status.cpu_percent >= 0.0);
        assert!(status.mem_total_mb > 0);
    }

    #[test]
    fn test_bench_result_display() {
        let result = BenchResult {
            name: "Test".to_string(),
            success: true,
            duration: Duration::from_millis(150),
            details: "42 items".to_string(),
            error: None,
            throughput: Some(280.0),
        };
        let display = format!("{}", result);
        assert!(display.contains("PASS"));
        assert!(display.contains("Test"));
        assert!(display.contains("150.0ms"));
    }

    #[tokio::test]
    async fn test_runner_creation() {
        let runner = BenchmarkRunner::new(None);
        assert!(runner.is_ok());
    }

    #[tokio::test]
    async fn test_prepare_test_data() {
        let runner = BenchmarkRunner::new(None).unwrap();
        let count = runner.prepare_test_data().unwrap();
        assert_eq!(count, 1000);
    }

    #[tokio::test]
    async fn test_bench_chunking() {
        let runner = BenchmarkRunner::new(None).unwrap();
        runner.prepare_test_data().unwrap();
        let result = runner.bench_chunking_throughput().await;
        assert!(result.success, "Chunking failed: {:?}", result.error);
        assert!(result.duration > Duration::ZERO);
    }
}
