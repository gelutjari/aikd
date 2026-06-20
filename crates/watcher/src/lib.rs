use aikd_core::{Chunk, Config};
use aikd_indexer::TantivyEngine;
use aikd_storage::{compute_blake3, Database};
use anyhow::Result;
use notify::Watcher;
use std::path::Path;
use std::sync::Mutex;

pub async fn run_watcher(config_path: &str, debounce_ms: u64) -> Result<()> {
    let cfg = Config::load(config_path).unwrap_or_default();
    println!("Starting AIKD file watcher (debounce: {debounce_ms}ms)...");
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
            println!("  Watching: {expanded}");
        }
    }

    let database = Database::open(&cfg.db_path())?;
    let tantivy = TantivyEngine::open(&cfg.tantivy_path())?;

    // Use Mutex for thread-safe access to pending events
    let pending_events: Mutex<std::collections::HashMap<std::path::PathBuf, notify::EventKind>> =
        Mutex::new(std::collections::HashMap::new());
    let last_event_time = Mutex::new(std::time::Instant::now());

    loop {
        match rx.recv_timeout(debounce_duration) {
            Ok(Ok(event)) => match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(_)
                | notify::EventKind::Remove(_) => {
                    let filtered: Vec<_> = event
                        .paths
                        .into_iter()
                        .filter(|p| {
                            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                cfg.scan.include_extensions.iter().any(|e2| e2 == ext)
                            } else {
                                false
                            }
                        })
                        .collect();
                    // Lock mutex for thread-safe access
                    let mut events = pending_events.lock().unwrap();
                    for path in filtered {
                        events.insert(path, event.kind);
                    }
                    *last_event_time.lock().unwrap() = std::time::Instant::now();
                }
                _ => {}
            },
            Ok(Err(e)) => {
                log::warn!("Watch error: {e}");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Check if we have pending events and debounce time has passed
                let should_process = {
                    let events = pending_events.lock().unwrap();
                    let last_time = last_event_time.lock().unwrap();
                    !events.is_empty() && last_time.elapsed() >= debounce_duration
                };

                if !should_process {
                    continue;
                }

                // Drain events under lock
                let events: Vec<_> = {
                    let mut events = pending_events.lock().unwrap();
                    events.drain().collect()
                };

                let mut changed = 0;
                let mut created = 0;
                let mut removed = 0;

                // Collect file changes for batch processing
                let mut to_index: Vec<(String, Vec<Chunk>)> = Vec::new();
                let mut to_remove: Vec<String> = Vec::new();

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
                                    continue;
                                }
                            }
                        }
                    }

                    match kind {
                        notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                            if path.exists() =>
                        {
                            if let Ok(content) = std::fs::read_to_string(path) {
                                if cfg.matches_content_filter(&content) {
                                    let chunks = aikd_chunker::chunk_file(
                                        &ps,
                                        &content,
                                        cfg.max_chunk_tokens(),
                                        cfg.min_chunk_tokens(),
                                    );
                                    to_index.push((ps.clone(), chunks));
                                    if matches!(kind, notify::EventKind::Create(_)) {
                                        created += 1;
                                        println!(
                                            "[{}] + Created: {}",
                                            chrono::Local::now().format("%H:%M:%S"),
                                            ps
                                        );
                                    } else {
                                        changed += 1;
                                        println!(
                                            "[{}] ~ Modified: {}",
                                            chrono::Local::now().format("%H:%M:%S"),
                                            ps
                                        );
                                    }
                                }
                            }
                        }
                        notify::EventKind::Remove(_) => {
                            to_remove.push(ps.clone());
                            removed += 1;
                            println!(
                                "[{}] - Removed: {}",
                                chrono::Local::now().format("%H:%M:%S"),
                                ps
                            );
                        }
                        _ => {}
                    }
                }

                // Batch store new/changed files using scanner
                if !to_index.is_empty() {
                    if let Err(e) = aikd_scanner::store_chunks(&to_index, &database) {
                        log::warn!("Failed to store chunks: {e}");
                    }
                    // Update Tantivy index for changed files
                    if let Err(e) = aikd_scanner::update_tantivy(&to_index, &tantivy) {
                        log::warn!("Failed to update tantivy: {e}");
                    }
                }

                // Remove deleted files
                for ps in &to_remove {
                    let tx = database.begin_transaction()?;
                    if let Ok(old_fid) = tx.conn().query_row::<i64, _, _>(
                        "SELECT id FROM files WHERE path=?1",
                        rusqlite::params![ps],
                        |r| r.get(0),
                    ) {
                        if let Err(e) = tx.conn().execute("DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)", rusqlite::params![old_fid]) {
                            log::warn!("Failed to delete embeddings for {ps}: {e}");
                        }
                        if let Err(e) = tx.conn().execute(
                            "DELETE FROM chunks WHERE file_id=?1",
                            rusqlite::params![old_fid],
                        ) {
                            log::warn!("Failed to delete chunks for {ps}: {e}");
                        }
                        if let Err(e) = tx
                            .conn()
                            .execute("DELETE FROM files WHERE id=?1", rusqlite::params![old_fid])
                        {
                            log::warn!("Failed to delete file {ps}: {e}");
                        }
                    }
                    tx.commit()?;
                }

                if changed + created + removed > 0 {
                    println!(
                        "[{}] Summary: +{} created, ~{} changed, -{} removed\n",
                        chrono::Local::now().format("%H:%M:%S"),
                        created,
                        changed,
                        removed
                    );
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    Ok(())
}
