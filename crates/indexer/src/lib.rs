use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use anyhow::Result;
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy};
use hnsw_rs::hnsw::Hnsw;
use anndists::dist::distances::DistCosine;
use parking_lot::{RwLock, Mutex};
use aikd_core::{SearchFilters, SearchResult};

pub const HNSW_M: usize = 16;
pub const HNSW_EF_CONSTRUCTION: usize = 200;
pub const HNSW_EF_SEARCH: usize = 64;

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

    pub fn with_hnsw_params(dim: usize, m: usize, ef_construction: usize, ef_search: usize) -> Self {
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

        if self.dirty.load(Ordering::Acquire) {
            drop(map);
            self.rebuild_hnsw_cache();
            self.dirty.store(false, Ordering::Release);
            let map = self.id_map.read();
            return self.search_with_cache(query, limit, &map);
        }

        self.search_with_cache(query, limit, &map)
    }

    fn search_with_cache(&self, query: &[f32], limit: usize, map: &parking_lot::RwLockReadGuard<Vec<String>>) -> Vec<(String, f32)> {
        let cache_guard = self.hnsw_cache.lock();
        match cache_guard.as_ref() {
            Some(cache) => {
                let results = cache.hnsw.search(query, limit, self.hnsw_ef_search);
                results.into_iter()
                    .filter_map(|r| {
                        map.get(r.d_id).map(|id| (id.clone(), 1.0 - r.distance))
                    })
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
}

pub struct TantivyEngine {
    index: Index,
    reader: IndexReader,
    schema: Schema,
    field_chunk_id: Field,
    field_file_path: Field,
    field_heading: Field,
    field_content: Field,
}

impl TantivyEngine {
    pub fn open(index_path: &Path) -> Result<Self> {
        std::fs::create_dir_all(index_path)?;

        let mut schema_builder = Schema::builder();
        let field_chunk_id = schema_builder.add_text_field("chunk_id", STRING | STORED);
        let field_file_path = schema_builder.add_text_field("file_path", TEXT | STORED);
        let field_heading = schema_builder.add_text_field("heading", TEXT | STORED);
        let field_content = schema_builder.add_text_field("content", TEXT | STORED);
        let schema = schema_builder.build();

        let index = if index_path.join("meta.json").exists() {
            Index::open_in_dir(index_path)?
        } else {
            Index::create_in_dir(index_path, schema.clone())?
        };

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        Ok(Self {
            index,
            reader,
            schema,
            field_chunk_id,
            field_file_path,
            field_heading,
            field_content,
        })
    }

    pub fn index_chunks(&self, chunks: &[(String, String, String, String)]) -> Result<()> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;

        for (chunk_id, file_path, heading, content) in chunks {
            writer.add_document(doc!(
                self.field_chunk_id => chunk_id.as_str(),
                self.field_file_path => file_path.as_str(),
                self.field_heading => heading.as_str(),
                self.field_content => content.as_str(),
            ))?;
        }

        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        writer.delete_all_documents()?;
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn search(&self, query_str: &str, limit: usize, filters: &SearchFilters) -> Result<Vec<SearchResult>> {
        let searcher = self.reader.searcher();

        let query_parser = QueryParser::for_index(
            &self.index,
            vec![self.field_content, self.field_heading, self.field_file_path],
        );

        let query = query_parser.parse_query(query_str)?;
        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit * 2))?;

        let mut results = Vec::new();

        for (_score, doc_addr) in top_docs {
            let doc = searcher.doc::<tantivy::TantivyDocument>(doc_addr)?;
            let file_path = doc.get_first(self.field_file_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(ref pf) = filters.path_contains {
                if !file_path.contains(pf.as_str()) {
                    continue;
                }
            }

            if let Some(ref pe) = filters.path_exclude {
                if file_path.contains(pe.as_str()) {
                    continue;
                }
            }

            if let Some(ref ft) = filters.file_types {
                let has_ext = ft.iter().any(|ext| file_path.ends_with(&format!(".{}", ext)));
                if !has_ext {
                    continue;
                }
            }

            let heading = doc.get_first(self.field_heading)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(ref hc) = filters.heading_contains {
                if !heading.contains(hc.as_str()) {
                    continue;
                }
            }

            let chunk_id = doc.get_first(self.field_chunk_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let content = doc.get_first(self.field_content)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            results.push(SearchResult {
                chunk_id,
                file_path,
                heading_hierarchy: heading.clone(),
                heading_text: heading,
                content,
                line_start: 0,
                line_end: 0,
                score: _score,
            });

            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }
}

pub struct HybridSearcher {
    tantivy: TantivyEngine,
    vector_index: Arc<VectorIndex>,
}

impl HybridSearcher {
    pub fn new(tantivy: TantivyEngine, vector_index: Arc<VectorIndex>) -> Self {
        Self { tantivy, vector_index }
    }

    pub fn tantivy(&self) -> &TantivyEngine {
        &self.tantivy
    }

    pub fn vector_index(&self) -> &VectorIndex {
        &self.vector_index
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
        let bm25_ids: Vec<String> = bm25_results.iter().map(|r| r.chunk_id.clone()).collect();

        let ann_results = self.vector_index.search(query_embedding, limit * 2);
        let ann_ids: Vec<String> = ann_results.iter().map(|(id, _)| id.clone()).collect();

        let fused = reciprocal_rank_fusion(&bm25_ids, &ann_ids, k);
        let fused_ids: Vec<String> = fused.iter().take(limit).map(|(id, _)| id.clone()).collect();

        let mut results = Vec::new();
        for id in &fused_ids {
            if let Some(r) = bm25_results.iter().find(|r| &r.chunk_id == id) {
                results.push(r.clone());
            } else if let Some((_, score)) = ann_results.iter().find(|(aid, _)| aid == id) {
                results.push(SearchResult {
                    chunk_id: id.clone(),
                    file_path: String::new(),
                    heading_hierarchy: String::new(),
                    heading_text: String::new(),
                    content: String::new(),
                    line_start: 0,
                    line_end: 0,
                    score: *score,
                });
            }
        }

        Ok(results)
    }
}

fn reciprocal_rank_fusion(kw: &[String], ann: &[String], k: u64) -> Vec<(String, f32)> {
    let mut scores: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    for (r, id) in kw.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k as f32 + r as f32 + 1.0);
    }
    for (r, id) in ann.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k as f32 + r as f32 + 1.0);
    }
    let mut fused: Vec<(String, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tantivy_open_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TantivyEngine::open(dir.path()).unwrap();
        engine.index_chunks(&[
            ("c1".into(), "/test.md".into(), "Heading".into(), "hello world".into()),
            ("c2".into(), "/test2.md".into(), "Other".into(), "foo bar".into()),
        ]).unwrap();

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
        let fused = reciprocal_rank_fusion(&kw, &ann, 60);
        assert_eq!(fused.len(), 4);
        assert!(fused[0].1 > fused[2].1);
    }

    #[test]
    fn test_hybrid_searcher() {
        let dir = tempfile::tempdir().unwrap();
        let tantivy = TantivyEngine::open(dir.path()).unwrap();
        tantivy.index_chunks(&[
            ("c1".into(), "/a.md".into(), "H1".into(), "rust programming".into()),
            ("c2".into(), "/b.md".into(), "H2".into(), "python scripting".into()),
        ]).unwrap();

        let vector_idx = Arc::new(VectorIndex::new(3));
        vector_idx.insert("c1", &[1.0, 0.0, 0.0]);
        vector_idx.insert("c2", &[0.0, 1.0, 0.0]);

        let searcher = HybridSearcher::new(tantivy, vector_idx);
        let filters = SearchFilters::default();
        let results = searcher.hybrid_search("rust", &[1.0, 0.0, 0.0], 10, &filters, 60).unwrap();
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
        engine.index_chunks(&[
            ("c1".into(), "/test.md".into(), "H".into(), "content".into()),
        ]).unwrap();
        engine.clear().unwrap();
        let filters = SearchFilters::default();
        let results = engine.search("content", 10, &filters).unwrap();
        assert!(results.is_empty());
    }
}
