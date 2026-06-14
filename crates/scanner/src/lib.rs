use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use anyhow::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use indicatif::{ProgressBar, ProgressStyle};

use aikd_core::{Config, Chunk};
use aikd_storage::{Database, compute_blake3};
use aikd_indexer::TantivyEngine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub files_found: usize,
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub chunks_created: usize,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub override_path: Option<PathBuf>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self { override_path: None }
    }
}

pub fn discover_files(cfg: &Config, opts: &ScanOptions) -> Vec<PathBuf> {
    let scan_paths: Vec<String> = opts.override_path
        .as_ref()
        .map(|p| vec![p.to_string_lossy().to_string()])
        .unwrap_or(cfg.scan.include_paths.clone());

    let mut files = Vec::new();
    for sp in &scan_paths {
        let expanded = shellexpand::tilde(sp);
        let root = Path::new(expanded.as_ref());
        if !root.exists() {
            log::warn!("{} not found, skipping", sp);
            continue;
        }
        for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.path().ancestors().any(|a| {
                a.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| cfg.should_exclude_dir(n))
                    .unwrap_or(false)
            }) {
                continue;
            }
            let fname = entry.file_name().to_str().unwrap_or("");
            if cfg.should_exclude_file(fname) || !cfg.matches_filename_filter(fname) {
                continue;
            }
            if !cfg.scan.include_extensions.iter().any(|ext| {
                entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s == ext.as_str())
                    .unwrap_or(false)
            }) {
                continue;
            }
            if let Ok(m) = entry.metadata() {
                if !cfg.check_file_size(m.len()) {
                    continue;
                }
            }
            files.push(entry.into_path());
        }
    }
    files
}

pub fn filter_changed(files: Vec<PathBuf>, db: &Database) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter(|path| {
            let ps = path.to_string_lossy().to_string();
            let new_hash = match compute_blake3(path) {
                Ok(h) => h,
                Err(_) => return true,
            };
            if let Ok(old_hash) = db.conn().query_row::<String, _, _>(
                "SELECT blake3_hash FROM files WHERE path=?1",
                rusqlite::params![ps],
                |r| r.get(0),
            ) {
                old_hash != new_hash
            } else {
                true
            }
        })
        .collect()
}

pub fn chunk_files(files: &[PathBuf], cfg: &Config) -> Vec<(String, Vec<Chunk>)> {
    files
        .par_iter()
        .filter_map(|path| {
            let ps = path.to_string_lossy().to_string();
            let content = std::fs::read_to_string(path).ok()?;
            if !cfg.matches_content_filter(&content) {
                return None;
            }
            let chunks = aikd_chunker::chunk_file(&ps, &content, cfg.max_chunk_tokens(), cfg.min_chunk_tokens());
            Some((ps, chunks))
        })
        .collect()
}

pub fn store_chunks(indexed: &[(String, Vec<Chunk>)], db: &Database) -> Result<()> {
    let tx = db.begin_transaction()?;
    for (ps, chunks) in indexed {
        let size = std::fs::metadata(ps).map(|m| m.len()).unwrap_or(0);
        let now = chrono::Utc::now().to_rfc3339();
        let hash = compute_blake3(Path::new(ps)).unwrap_or_default();

        if let Ok(old_fid) = tx.conn().query_row::<i64, _, _>(
            "SELECT id FROM files WHERE path=?1",
            rusqlite::params![ps],
            |r| r.get(0),
        ) {
            let _ = tx.conn().execute(
                "DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id=?1)",
                rusqlite::params![old_fid],
            );
            let _ = tx
                .conn()
                .execute("DELETE FROM chunks WHERE file_id=?1", rusqlite::params![old_fid]);
            let _ = tx
                .conn()
                .execute("DELETE FROM files WHERE id=?1", rusqlite::params![old_fid]);
        }

        tx.conn().execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1,?2,?3,?4,'active',?5)",
            rusqlite::params![ps, size as i64, now, now, hash],
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
    Ok(())
}

pub fn update_tantivy(indexed: &[(String, Vec<Chunk>)], tantivy: &TantivyEngine) -> Result<()> {
    tantivy.clear()?;
    let tc: Vec<(String, String, String, String)> = indexed
        .iter()
        .flat_map(|(p, cs)| {
            cs.iter()
                .map(move |c| (c.id.clone(), p.clone(), c.heading_hierarchy_str(), c.content.clone()))
        })
        .collect();
    tantivy.index_chunks(&tc)?;
    Ok(())
}

pub fn run_scan(cfg: &Config, db: &Database, tantivy: &TantivyEngine, opts: &ScanOptions) -> Result<ScanProgress> {
    let start = Instant::now();

    eprint!("[aikd] Discovering files...");
    let files = discover_files(cfg, opts);
    eprintln!(" found {} files", files.len());

    eprint!("[aikd] Checking for changes...");
    let files_to_index = filter_changed(files, db);
    let files_found = files_to_index.len();
    eprintln!(" {} to index", files_found);

    if files_found == 0 {
        return Ok(ScanProgress {
            files_found: 0,
            files_indexed: 0,
            files_skipped: 0,
            chunks_created: 0,
            elapsed: start.elapsed(),
        });
    }

    let pb = ProgressBar::new(files_found as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("[aikd] {spinner:.green} Chunking [{bar:40.cyan/blue}] {pos}/{len} files | {per_sec} | ETA: {eta}")
        .unwrap()
        .progress_chars("█░"));

    let indexed: Vec<(String, Vec<Chunk>)> = files_to_index
        .par_iter()
        .filter_map(|path| {
            let ps = path.to_string_lossy().to_string();
            let content = std::fs::read_to_string(path).ok()?;
            if !cfg.matches_content_filter(&content) {
                pb.inc(1);
                return None;
            }
            let chunks = aikd_chunker::chunk_file(&ps, &content, cfg.max_chunk_tokens(), cfg.min_chunk_tokens());
            pb.inc(1);
            Some((ps, chunks))
        })
        .collect();

    pb.finish_with_message("done");

    let chunks_created: usize = indexed.iter().map(|(_, c)| c.len()).sum();
    eprintln!("[aikd] Storing {} chunks from {} files...", chunks_created, indexed.len());
    store_chunks(&indexed, db)?;

    eprint!("[aikd] Updating search index...");
    update_tantivy(&indexed, tantivy)?;
    eprintln!(" done");

    let files_indexed = indexed.len();
    let files_skipped = files_found - files_indexed;

    Ok(ScanProgress {
        files_found,
        files_indexed,
        files_skipped,
        chunks_created,
        elapsed: start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_files() {
        let cfg = Config::default();
        let opts = ScanOptions::default();
        let files = discover_files(&cfg, &opts);
        assert!(files.len() > 0);
    }
}
