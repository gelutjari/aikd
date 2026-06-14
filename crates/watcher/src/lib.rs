use std::path::Path;
use anyhow::Result;
use notify::Watcher;
use aikd_core::Config;
use aikd_storage::{Database, compute_blake3};
use aikd_indexer::TantivyEngine;
use rusqlite;

pub async fn run_watcher(config_path: &str, debounce_ms: u64) -> Result<()> {
    let cfg = Config::load(config_path).unwrap_or_default();
    println!("Starting AIKD file watcher (debounce: {}ms)...", debounce_ms);
    println!("Watching paths: {:?}", cfg.scan.include_paths);
    println!("Press Ctrl+C to stop.\n");

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    let debounce_duration = std::time::Duration::from_millis(debounce_ms);

    for sp in &cfg.scan.include_paths {
        let expanded = shellexpand::tilde(sp);
        let path = Path::new(expanded.as_ref());
        if path.exists() {
            watcher.watch(path, notify::RecursiveMode::Recursive)?;
            println!("  Watching: {}", expanded);
        }
    }

    let database = Database::open(&cfg.db_path())?;
    let tantivy = TantivyEngine::open(&cfg.tantivy_path())?;

    let mut pending_events: std::collections::HashMap<std::path::PathBuf, notify::EventKind> = std::collections::HashMap::new();
    let mut last_event_time = std::time::Instant::now();

    loop {
        match rx.recv_timeout(debounce_duration) {
            Ok(Ok(event)) => {
                match event.kind {
                    notify::EventKind::Create(_) |
                    notify::EventKind::Modify(_) |
                    notify::EventKind::Remove(_) => {
                        let filtered: Vec<_> = event.paths.into_iter().filter(|p| {
                            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                cfg.scan.include_extensions.iter().any(|e2| e2 == ext)
                            } else {
                                false
                            }
                        }).collect();
                        for path in filtered {
                            pending_events.insert(path, event.kind.clone());
                        }
                        last_event_time = std::time::Instant::now();
                    }
                    _ => {}
                }
            }
            Ok(Err(e)) => {
                log::warn!("Watch error: {}", e);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if pending_events.is_empty() || last_event_time.elapsed() < debounce_duration {
                    continue;
                }

                let events: Vec<_> = pending_events.drain().collect();
                let now = chrono::Utc::now().to_rfc3339();
                let mut changed = 0;
                let mut created = 0;
                let mut removed = 0;

                for (path, kind) in &events {
                    let ps = path.to_string_lossy().to_string();

                    // Skip if hash unchanged (incremental check)
                    if matches!(kind, notify::EventKind::Modify(_)) && path.exists() {
                        if let Ok(new_hash) = compute_blake3(path) {
                            if let Ok(old_hash) = database.conn().query_row::<String, _, _>(
                                "SELECT blake3_hash FROM files WHERE path=?1",
                                rusqlite::params![ps],
                                |r| r.get(0),
                            ) {
                                if old_hash == new_hash {
                                    continue; // File unchanged
                                }
                            }
                        }
                    }

                    match kind {
                        notify::EventKind::Create(_) | notify::EventKind::Modify(_) => {
                            if path.exists() {
                                if let Ok(content) = std::fs::read_to_string(path) {
                                    if cfg.matches_content_filter(&content) {
                                        let chunks = aikd_chunker::chunk_file(&ps, &content, cfg.max_chunk_tokens(), cfg.min_chunk_tokens());
                                        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                                        let hash = compute_blake3(path).unwrap_or_default();
                                        let tx = database.begin_transaction()?;
                                        if let Ok(old_fid) = tx.conn().query_row::<i64, _, _>("SELECT id FROM files WHERE path=?1", rusqlite::params![ps], |r| r.get(0)) {
                                            let _ = tx.conn().execute("DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)", rusqlite::params![old_fid]);
                                            let _ = tx.conn().execute("DELETE FROM chunks WHERE file_id=?1", rusqlite::params![old_fid]);
                                            let _ = tx.conn().execute("DELETE FROM files WHERE id=?1", rusqlite::params![old_fid]);
                                        }
                                        let _ = tx.conn().execute("INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1,?2,?3,?4,'active',?5)", rusqlite::params![ps, size as i64, now, now, hash]);
                                        if let Ok(fid) = tx.conn().query_row("SELECT id FROM files WHERE path=?1", rusqlite::params![ps], |r| r.get::<_, i64>(0)) {
                                            for c in &chunks {
                                                let hj = serde_json::to_string(&c.heading_hierarchy).unwrap_or_default();
                                                let mj = serde_json::to_string(&c.metadata).unwrap_or_default();
                                                let _ = tx.conn().execute(
                                                    "INSERT INTO chunks (id,file_id,chunk_index,heading_hierarchy,heading_level,heading_text,line_start,line_end,content,metadata_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                                                    rusqlite::params![c.id, fid, c.chunk_index as i64, hj, c.heading_level as i64, c.heading_text, c.line_start as i64, c.line_end as i64, c.content, mj, now, now],
                                                );
                                            }
                                        }
                                        tx.commit()?;
                                        let tc: Vec<(String, String, String, String)> = chunks.iter()
                                            .map(|c| (c.id.clone(), ps.clone(), c.heading_hierarchy_str(), c.content.clone()))
                                            .collect();
                                        tantivy.index_chunks(&tc)?;
                                        if matches!(kind, notify::EventKind::Create(_)) {
                                            created += 1;
                                            println!("[{}] + Created: {}", chrono::Local::now().format("%H:%M:%S"), ps);
                                        } else {
                                            changed += 1;
                                            println!("[{}] ~ Modified: {}", chrono::Local::now().format("%H:%M:%S"), ps);
                                        }
                                    }
                                }
                            }
                        }
                        notify::EventKind::Remove(_) => {
                            let tx = database.begin_transaction()?;
                            if let Ok(old_fid) = tx.conn().query_row::<i64, _, _>("SELECT id FROM files WHERE path=?1", rusqlite::params![ps], |r| r.get(0)) {
                                let _ = tx.conn().execute("DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)", rusqlite::params![old_fid]);
                                let _ = tx.conn().execute("DELETE FROM chunks WHERE file_id=?1", rusqlite::params![old_fid]);
                                let _ = tx.conn().execute("DELETE FROM files WHERE id=?1", rusqlite::params![old_fid]);
                            }
                            tx.commit()?;
                            removed += 1;
                            println!("[{}] - Removed: {}", chrono::Local::now().format("%H:%M:%S"), ps);
                        }
                        _ => {}
                    }
                }

                if changed + created + removed > 0 {
                    println!("[{}] Summary: +{} created, ~{} changed, -{} removed\n",
                        chrono::Local::now().format("%H:%M:%S"), created, changed, removed);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    Ok(())
}
