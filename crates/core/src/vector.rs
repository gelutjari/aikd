//! Vector encoding primitives shared by core consumers.
//!
//! The canonical embedding model constants live here so crates like
//! `aikd-indexer` can decode stored vectors without depending on the
//! (heavy, optional) `aikd-embedder` crate (ADR-0001).

/// Canonical embedding model id used for stored vectors.
pub const EMBEDDING_MODEL: &str = "all-MiniLM-L6-v2";
/// Canonical embedding vector dimensionality.
pub const EMBEDDING_DIMENSIONS: usize = 384;

/// Encode an f32 slice as little-endian bytes for SQLite BLOB storage.
pub fn f32_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Decode little-endian bytes back into an f32 vector.
pub fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Cosine similarity between two vectors. Pure math, no model needed.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let d = na.sqrt() * nb.sqrt();
    if d == 0.0 {
        0.0
    } else {
        dot / d
    }
}

/// Brute-force top-K vector search over in-memory vectors.
/// For small/medium DB this is the default; HNSW in `aikd-indexer` is an
/// optional acceleration for the same canonical data.
pub fn vector_search(
    query: &[f32],
    all: &[(String, Vec<f32>)],
    limit: usize,
) -> Vec<(String, f32)> {
    let mut scored: Vec<(String, f32)> = all
        .iter()
        .map(|(id, e)| (id.clone(), cosine_similarity(query, e)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored
}

/// The stable boundary every semantic provider must implement.
/// AIKD core owns this contract: canonical schemas, provenance, lifecycle.
/// A provider (local ONNX build, external service, future `aikd-semantic`)
/// only fills in inference + model lifecycle behind this trait.
pub trait SemanticProvider {
    /// Human-readable provider name, e.g. `onnx-minilm`, `external-openai`.
    fn name(&self) -> &str;
    /// Model id whose vectors are stored (`embeddings.model` column).
    fn model_id(&self) -> &str;
    /// Vector dimensionality produced by this provider.
    fn dimensions(&self) -> usize;
    /// True when the provider can serve inference right now
    /// (model files present, service reachable, key configured).
    fn is_available(&self) -> bool;
    /// Embed a batch of texts. Fails honestly when unavailable.
    fn embed_texts(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;
}

/// Load all stored vectors for the canonical model. Reading stored vectors
/// never requires ONNX — only inference (creating new vectors) does.
pub fn load_all_embeddings(conn: &rusqlite::Connection) -> anyhow::Result<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare("SELECT chunk_id, vector FROM embeddings WHERE model = ?1")?;
    let rows = stmt.query_map(rusqlite::params![EMBEDDING_MODEL], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (id, bytes) = row?;
        result.push((id, bytes_to_f32(&bytes)));
    }
    Ok(result)
}

/// Import vectors produced by an EXTERNAL provider (JSON array of
/// `{chunk_id, embedding}`). This is the zero-ONNX path to semantic data:
/// vectors come from outside, AIKD stores and serves them.
pub fn import_embeddings_json(
    conn: &rusqlite::Connection,
    json_path: &str,
) -> anyhow::Result<usize> {
    let content = std::fs::read_to_string(json_path)?;
    let data: serde_json::Value = serde_json::from_str(&content)?;
    let arr = data
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Expected JSON array"))?;
    let mut imported = 0;
    for item in arr {
        let chunk_id = item["chunk_id"].as_str().unwrap_or_default().to_string();
        let embedding: Vec<f32> = item["embedding"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect()
            })
            .unwrap_or_default();
        if chunk_id.is_empty() || embedding.is_empty() {
            continue;
        }
        conn.execute(
            "INSERT OR REPLACE INTO embeddings (chunk_id, model, dimensions, vector) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![chunk_id, EMBEDDING_MODEL, EMBEDDING_DIMENSIONS as i64, f32_to_bytes(&embedding)],
        )?;
        imported += 1;
    }
    Ok(imported)
}

/// Export chunk contents for an EXTERNAL provider to embed.
/// Zero-ONNX workflow: `aikd export` → provider embeds → `aikd import`.
/// In lean mode (empty `chunks.content`), reads lines live from disk.
pub fn export_chunks_for_embedding(
    conn: &rusqlite::Connection,
    output_path: &str,
) -> anyhow::Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT c.id, f.path, c.line_start, c.line_end, c.content FROM chunks c JOIN files f ON f.id = c.file_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? as usize,
            row.get::<_, i64>(3)? as usize,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut items = Vec::new();
    for row in rows {
        let (id, fp, ls, le, content) = row?;
        let text = if !content.is_empty() {
            content
        } else if let Ok(raw) = std::fs::read_to_string(&fp) {
            let lines: Vec<&str> = raw.lines().collect();
            if lines.is_empty() {
                String::new()
            } else {
                let s = ls.saturating_sub(1).min(lines.len().saturating_sub(1));
                let e = le.max(s + 1).min(lines.len());
                lines[s..e].join("\n")
            }
        } else {
            String::new()
        };
        items.push(serde_json::json!({"chunk_id": id, "content": text}));
    }
    let json = serde_json::to_string_pretty(&items)?;
    std::fs::write(output_path, json)?;
    Ok(items.len())
}

/// Small LRU cache over stored vectors. std Mutex: lookups are not hot-path.
pub struct EmbeddingCache {
    cache: std::sync::Mutex<lru::LruCache<String, Vec<f32>>>,
}

impl EmbeddingCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: std::sync::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(capacity.max(1000)).unwrap(),
            )),
        }
    }

    pub fn get(&self, chunk_id: &str) -> Option<Vec<f32>> {
        self.cache.lock().ok()?.get(chunk_id).cloned()
    }

    pub fn put(&self, chunk_id: String, vector: Vec<f32>) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.put(chunk_id, vector);
        }
    }

    pub fn preload_for_ids(
        &self,
        chunk_ids: &[String],
        conn: &rusqlite::Connection,
    ) -> anyhow::Result<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| anyhow::anyhow!("cache lock poisoned"))?;
        let ids_to_load: Vec<&String> = chunk_ids
            .iter()
            .filter(|id| cache.get(id.as_str()).is_none())
            .collect();
        if ids_to_load.is_empty() {
            return Ok(());
        }
        let mut stmt = conn.prepare(
            "SELECT chunk_id, vector FROM embeddings WHERE model = ?1 AND chunk_id = ?2",
        )?;
        for id in ids_to_load {
            if let Ok(bytes) = stmt
                .query_row(rusqlite::params![EMBEDDING_MODEL, id.as_str()], |row| {
                    row.get::<_, Vec<u8>>(1)
                })
            {
                cache.put(id.clone(), bytes_to_f32(&bytes));
            }
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cache.lock().map(|c| c.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let v = vec![1.0f32, -2.5, 0.0];
        assert_eq!(bytes_to_f32(&f32_to_bytes(&v)), v);
    }

    #[test]
    fn cosine_basics() {
        assert!((cosine_similarity(&[1.0, 0.0, 0.0], &[1.0, 0.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]) - 0.0).abs() < 1e-6);
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) - (-1.0)).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn search_topk() {
        let all = vec![
            ("a".to_string(), vec![1.0, 0.0, 0.0]),
            ("b".to_string(), vec![0.0, 1.0, 0.0]),
            ("c".to_string(), vec![0.7, 0.7, 0.0]),
        ];
        let results = vector_search(&[1.0, 0.0, 0.0], &all, 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "a");
    }

    #[test]
    fn cache_put_get_miss() {
        let cache = EmbeddingCache::new(100);
        assert!(cache.is_empty());
        cache.put("c1".into(), vec![1.0, 0.0, 0.0]);
        assert_eq!(cache.get("c1"), Some(vec![1.0, 0.0, 0.0]));
        assert!(cache.get("nope").is_none());
    }

    #[test]
    fn f32_empty_roundtrip() {
        assert!(f32_to_bytes(&[]).is_empty());
        assert!(bytes_to_f32(&[]).is_empty());
    }
}
