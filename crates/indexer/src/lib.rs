use aikd_core::fusion::reciprocal_rank_fusion;
use aikd_core::{SearchFilters, SearchResult};
use anndists::dist::distances::DistCosine;
use anyhow::Result;
use hnsw_rs::hnsw::Hnsw;
use parking_lot::{Mutex, RwLock};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub const HNSW_M: usize = 16;
pub const HNSW_EF_CONSTRUCTION: usize = 200;
pub const HNSW_EF_SEARCH: usize = 64;

#[allow(dead_code)]
struct HnswCache {
    hnsw: Hnsw<'static, f32, DistCosine>,
    built_at: Instant,
    entry_count: usize,
}

pub struct VectorIndex {
    data: RwLock<Vec<Vec<f32>>>,
    id_map: RwLock<Vec<String>>,
    dim: usize,
    hnsw_cache: Mutex<Option<HnswCache>>,
    dirty: AtomicBool,
    hnsw_m: usize,
    hnsw_ef_construction: usize,
    hnsw_ef_search: usize,
}

impl VectorIndex {
    pub fn new(dim: usize) -> Self {
        Self {
            data: RwLock::new(Vec::new()),
            id_map: RwLock::new(Vec::new()),
            dim,
            hnsw_cache: Mutex::new(None),
            dirty: AtomicBool::new(true),
            hnsw_m: HNSW_M,
            hnsw_ef_construction: HNSW_EF_CONSTRUCTION,
            hnsw_ef_search: HNSW_EF_SEARCH,
        }
    }

    /// Estimate memory usage in bytes.
    /// Returns (vector_data_bytes, id_map_bytes, total_bytes).
    pub fn memory_usage(&self) -> (usize, usize, usize) {
        let data = self.data.read();
        let map = self.id_map.read();

        let vector_bytes = data.len() * self.dim * std::mem::size_of::<f32>();
        let id_bytes: usize = map.iter().map(|s| s.len() + 24).sum();
        let total = vector_bytes + id_bytes;

        (vector_bytes, id_bytes, total)
    }

    /// Get human-readable memory usage string.
    pub fn memory_usage_str(&self) -> String {
        let (vec_bytes, id_bytes, total) = self.memory_usage();
        format!(
            "Vectors: {:.1} MB, IDs: {:.1} MB, Total: {:.1} MB",
            vec_bytes as f64 / (1024.0 * 1024.0),
            id_bytes as f64 / (1024.0 * 1024.0),
            total as f64 / (1024.0 * 1024.0)
        )
    }

    pub fn with_hnsw_params(
        dim: usize,
        m: usize,
        ef_construction: usize,
        ef_search: usize,
    ) -> Self {
        Self {
            data: RwLock::new(Vec::new()),
            id_map: RwLock::new(Vec::new()),
            dim,
            hnsw_cache: Mutex::new(None),
            dirty: AtomicBool::new(true),
            hnsw_m: m,
            hnsw_ef_construction: ef_construction,
            hnsw_ef_search: ef_search,
        }
    }

    pub fn insert(&self, id: &str, vector: &[f32]) {
        let mut map = self.id_map.write();
        let mut data = self.data.write();
        map.push(id.to_string());
        data.push(vector.to_vec());
        self.dirty.store(true, Ordering::Release);
    }

    fn rebuild_hnsw_cache(&self) {
        let data = self.data.read();
        if data.is_empty() {
            *self.hnsw_cache.lock() = None;
            return;
        }
        let hnsw: Hnsw<f32, DistCosine> = Hnsw::new(
            self.hnsw_m,
            data.len().max(100),
            16,
            self.hnsw_ef_construction,
            DistCosine,
        );
        for (i, v) in data.iter().enumerate() {
            hnsw.insert((v, i));
        }
        let cache = HnswCache {
            hnsw,
            built_at: Instant::now(),
            entry_count: data.len(),
        };
        *self.hnsw_cache.lock() = Some(cache);
    }

    pub fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)> {
        let map = self.id_map.read();

        if map.is_empty() {
            return Vec::new();
        }

        if self
            .dirty
            .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            drop(map);
            self.rebuild_hnsw_cache();
            let map = self.id_map.read();
            return self.search_with_cache(query, limit, &map);
        }

        self.search_with_cache(query, limit, &map)
    }

    fn search_with_cache(
        &self,
        query: &[f32],
        limit: usize,
        map: &parking_lot::RwLockReadGuard<Vec<String>>,
    ) -> Vec<(String, f32)> {
        let cache_guard = self.hnsw_cache.lock();
        match cache_guard.as_ref() {
            Some(cache) => {
                let results = cache.hnsw.search(query, limit, self.hnsw_ef_search);
                results
                    .into_iter()
                    .filter_map(|r| map.get(r.d_id).map(|id| (id.clone(), 1.0 - r.distance)))
                    .collect()
            }
            None => Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.id_map.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.id_map.read().is_empty()
    }

    /// Load all embeddings from the database into the vector index.
    pub fn load_from_db(conn: &rusqlite::Connection, model_name: &str) -> Result<Self> {
        let index = Self::new(aikd_core::EMBEDDING_DIMENSIONS);
        let mut stmt = conn.prepare("SELECT chunk_id, vector FROM embeddings WHERE model = ?1")?;
        let rows = stmt.query_map(rusqlite::params![model_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        for row in rows {
            let (id, bytes) = row?;
            let vec = aikd_core::bytes_to_f32(&bytes);
            index.insert(&id, &vec);
        }
        Ok(index)
    }
}

/// Lexical BM25 search engine backed by SQLite FTS5 contentless `chunks_fts` (schema v14).
/// Keeps the `TantivyEngine` name and signature compatibility while eliminating external
/// multi-megabyte Tantivy segment directories. Also maintains an optional standalone
/// in-memory/SQLite fallback when used in isolated unit tests without an attached `Database`.
pub struct TantivyEngine {
    #[allow(dead_code)]
    index_path: PathBuf,
    standalone_db: Mutex<Option<Connection>>,
}

impl TantivyEngine {
    pub fn open(index_path: &Path) -> Result<Self> {
        Ok(Self {
            index_path: index_path.to_path_buf(),
            standalone_db: Mutex::new(None),
        })
    }

    fn ensure_standalone(&self) -> Result<()> {
        let mut guard = self.standalone_db.lock();
        if guard.is_none() {
            let conn = Connection::open_in_memory()?;
            aikd_storage::schema::run_migrations(&conn)?;
            *guard = Some(conn);
        }
        Ok(())
    }

    pub fn index_chunks(&self, chunks: &[(String, String, String, String)]) -> Result<()> {
        self.ensure_standalone()?;
        let guard = self.standalone_db.lock();
        if let Some(ref conn) = *guard {
            for (i, (chunk_id, file_path, heading, content)) in chunks.iter().enumerate() {
                conn.execute(
                    "INSERT OR IGNORE INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1, 0, 't', 't', 'active', '')",
                    rusqlite::params![file_path],
                )?;
                let fid: i64 = conn.query_row(
                    "SELECT id FROM files WHERE path = ?1",
                    rusqlite::params![file_path],
                    |r| r.get(0),
                )?;
                conn.execute(
                    "INSERT OR REPLACE INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, created_at, updated_at) VALUES (?1, ?2, ?3, '[]', 1, ?4, 1, 1, ?5, 't', 't')",
                    rusqlite::params![chunk_id, fid, i as i64, heading, content],
                )?;
                let rowid: i64 = conn.query_row(
                    "SELECT rowid FROM chunks WHERE id = ?1",
                    rusqlite::params![chunk_id],
                    |r| r.get(0),
                )?;
                let tokens = format!("{heading} {content}");
                aikd_storage::fts::index_chunk_fts(conn, rowid, file_path, heading, &tokens)?;
            }
        }
        Ok(())
    }

    pub fn index_chunks_with_heap(
        &self,
        chunks: &[(String, String, String, String)],
        _heap_bytes: usize,
    ) -> Result<()> {
        self.index_chunks(chunks)
    }

    pub fn delete_chunks(&self, chunk_ids: &[String]) -> Result<()> {
        let guard = self.standalone_db.lock();
        if let Some(ref conn) = *guard {
            aikd_storage::fts::delete_chunks_fts(conn, chunk_ids)?;
            for id in chunk_ids {
                let _ = conn.execute("DELETE FROM chunks WHERE id = ?1", rusqlite::params![id]);
            }
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let guard = self.standalone_db.lock();
        if let Some(ref conn) = *guard {
            aikd_storage::fts::clear_fts(conn)?;
            let _ = conn.execute("DELETE FROM chunks", []);
            let _ = conn.execute("DELETE FROM files", []);
        }
        Ok(())
    }

    pub fn clear_with_heap(&self, _heap_bytes: usize) -> Result<()> {
        self.clear()
    }

    /// Search against a provided SQLite connection (`chunks_fts` + live disk snippets).
    pub fn search_with_conn(
        &self,
        conn: &Connection,
        query_str: &str,
        limit: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<SearchResult>> {
        aikd_storage::fts::search_lexical(conn, query_str, limit, filters)
    }

    /// Search using the engine's standalone in-memory store (for unit tests / benchmarks).
    pub fn search(
        &self,
        query_str: &str,
        limit: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<SearchResult>> {
        let guard = self.standalone_db.lock();
        if let Some(ref conn) = *guard {
            return aikd_storage::fts::search_lexical(conn, query_str, limit, filters);
        }
        Ok(Vec::new())
    }
}

pub struct HybridSearcher {
    tantivy: Arc<TantivyEngine>,
    vector_index: Arc<VectorIndex>,
}

impl HybridSearcher {
    pub fn new(tantivy: Arc<TantivyEngine>, vector_index: Arc<VectorIndex>) -> Self {
        Self {
            tantivy,
            vector_index,
        }
    }

    pub fn tantivy(&self) -> &TantivyEngine {
        &self.tantivy
    }

    pub fn vector_index(&self) -> &VectorIndex {
        &self.vector_index
    }

    pub fn hybrid_search_with_conn(
        &self,
        conn: &Connection,
        query_str: &str,
        query_embedding: &[f32],
        limit: usize,
        filters: &SearchFilters,
        k: u64,
    ) -> Result<Vec<SearchResult>> {
        let bm25_results = self
            .tantivy
            .search_with_conn(conn, query_str, limit * 2, filters)?;
        self.fuse_results(bm25_results, query_embedding, limit, k)
    }

    pub fn hybrid_search(
        &self,
        query_str: &str,
        query_embedding: &[f32],
        limit: usize,
        filters: &SearchFilters,
        k: u64,
    ) -> Result<Vec<SearchResult>> {
        let bm25_results = self.tantivy.search(query_str, limit * 2, filters)?;
        self.fuse_results(bm25_results, query_embedding, limit, k)
    }

    fn fuse_results(
        &self,
        bm25_results: Vec<SearchResult>,
        query_embedding: &[f32],
        limit: usize,
        k: u64,
    ) -> Result<Vec<SearchResult>> {
        let ann_results = self.vector_index.search(query_embedding, limit * 2);

        let bm25_map: std::collections::HashMap<&str, &SearchResult> = bm25_results
            .iter()
            .map(|r| (r.chunk_id.as_str(), r))
            .collect();

        let bm25_ids: Vec<String> = bm25_results.iter().map(|r| r.chunk_id.clone()).collect();
        let ann_ids: Vec<String> = ann_results.iter().map(|(id, _)| id.clone()).collect();

        let fused = reciprocal_rank_fusion(&bm25_ids, &ann_ids, k);

        let mut results = Vec::with_capacity(limit);
        for (id, _score) in fused.iter().take(limit) {
            if let Some(r) = bm25_map.get(id.as_str()) {
                results.push((*r).clone());
            }
        }
        if results.is_empty() && !bm25_results.is_empty() {
            results = bm25_results.into_iter().take(limit).collect();
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tantivy_open_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TantivyEngine::open(dir.path()).unwrap();
        engine
            .index_chunks(&[
                (
                    "c1".into(),
                    "/test.md".into(),
                    "Heading".into(),
                    "hello world".into(),
                ),
                (
                    "c2".into(),
                    "/test2.md".into(),
                    "Other".into(),
                    "foo bar".into(),
                ),
            ])
            .unwrap();

        let filters = SearchFilters::default();
        let results = engine.search("hello", 10, &filters).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk_id, "c1");
    }

    #[test]
    fn test_vector_index_insert_and_search() {
        let idx = VectorIndex::new(3);
        idx.insert("a", &[1.0, 0.0, 0.0]);
        idx.insert("b", &[0.0, 1.0, 0.0]);
        idx.insert("c", &[0.7, 0.7, 0.0]);

        let results = idx.search(&[1.0, 0.0, 0.0], 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "a");
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn test_vector_index_empty() {
        let idx = VectorIndex::new(3);
        assert!(idx.is_empty());
        assert_eq!(idx.len(), 0);
    }

    #[test]
    fn test_reciprocal_rank_fusion() {
        let kw = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let ann = vec!["b".to_string(), "a".to_string(), "d".to_string()];
        let fused = reciprocal_rank_fusion(&kw, &ann, aikd_core::fusion::DEFAULT_RRF_K);
        assert_eq!(fused.len(), 4);
        assert!(fused[0].1 > fused[2].1);
    }

    #[test]
    fn test_hybrid_searcher() {
        let dir = tempfile::tempdir().unwrap();
        let tantivy = TantivyEngine::open(dir.path()).unwrap();
        tantivy
            .index_chunks(&[
                (
                    "c1".into(),
                    "/a.md".into(),
                    "H1".into(),
                    "rust programming".into(),
                ),
                (
                    "c2".into(),
                    "/b.md".into(),
                    "H2".into(),
                    "python scripting".into(),
                ),
            ])
            .unwrap();

        let vector_idx = Arc::new(VectorIndex::new(3));
        vector_idx.insert("c1", &[1.0, 0.0, 0.0]);
        vector_idx.insert("c2", &[0.0, 1.0, 0.0]);

        let searcher = HybridSearcher::new(Arc::new(tantivy), vector_idx);
        let filters = SearchFilters::default();
        let results = searcher
            .hybrid_search(
                "rust",
                &[1.0, 0.0, 0.0],
                10,
                &filters,
                aikd_core::fusion::DEFAULT_RRF_K,
            )
            .unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_vector_index_dirty_flag() {
        let idx = VectorIndex::new(3);
        assert!(idx.dirty.load(std::sync::atomic::Ordering::Acquire));
        idx.insert("a", &[1.0, 0.0, 0.0]);
        assert!(idx.dirty.load(std::sync::atomic::Ordering::Acquire));
        idx.search(&[1.0, 0.0, 0.0], 1);
        assert!(!idx.dirty.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn test_vector_index_with_params() {
        let idx = VectorIndex::with_hnsw_params(3, 8, 100, 32);
        assert_eq!(idx.hnsw_m, 8);
        assert_eq!(idx.hnsw_ef_construction, 100);
        assert_eq!(idx.hnsw_ef_search, 32);
    }

    #[test]
    fn test_tantivy_clear() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TantivyEngine::open(dir.path()).unwrap();
        engine
            .index_chunks(&[("c1".into(), "/test.md".into(), "H".into(), "content".into())])
            .unwrap();

        let filters = SearchFilters::default();
        assert!(!engine.search("content", 10, &filters).unwrap().is_empty());

        engine.clear().unwrap();
        assert!(engine.search("content", 10, &filters).unwrap().is_empty());
    }

    #[test]
    fn test_search_with_path_filter() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TantivyEngine::open(dir.path()).unwrap();
        engine
            .index_chunks(&[
                (
                    "c1".into(),
                    "/src/auth.rs".into(),
                    "Auth".into(),
                    "login token".into(),
                ),
                (
                    "c2".into(),
                    "/docs/auth.md".into(),
                    "Auth".into(),
                    "login token".into(),
                ),
            ])
            .unwrap();

        let filters = SearchFilters {
            path_contains: Some("src".into()),
            ..Default::default()
        };
        let results = engine.search("login", 10, &filters).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, "c1");
    }

    #[test]
    fn test_search_with_heading_filter() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TantivyEngine::open(dir.path()).unwrap();
        engine
            .index_chunks(&[
                (
                    "c1".into(),
                    "/a.md".into(),
                    "Installation".into(),
                    "install rust".into(),
                ),
                (
                    "c2".into(),
                    "/b.md".into(),
                    "Usage".into(),
                    "install cargo".into(),
                ),
            ])
            .unwrap();

        let filters = SearchFilters {
            heading_contains: Some("Install".into()),
            ..Default::default()
        };
        let results = engine.search("install", 10, &filters).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, "c1");
    }

    #[test]
    fn test_code_punctuation_query_does_not_fail() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TantivyEngine::open(dir.path()).unwrap();
        engine
            .index_chunks(&[(
                "c1".into(),
                "/src/lib.rs".into(),
                "fn run_scan".into(),
                "pub fn run_scan(cfg: &Config)".into(),
            )])
            .unwrap();

        let filters = SearchFilters::default();
        for q in ["fn(", "foo::bar", "[aikd]", "cfg: &Config", "():"] {
            let res = engine.search(q, 10, &filters);
            assert!(res.is_ok(), "query `{q}` must not fail: {:?}", res.err());
        }
    }
}
