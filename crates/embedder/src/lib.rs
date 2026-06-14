use aikd_core::ResourceProfile;
use anyhow::Result;
use fastembed::{InitOptionsUserDefined, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel};
use rusqlite::Connection;
use std::path::Path;

pub const MODEL_NAME: &str = "all-MiniLM-L6-v2";
pub const DIMENSIONS: usize = 384;

const HF_MODEL_URLS: &[(&str, &str)] = &[
    ("model.onnx", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx"),
    ("tokenizer.json", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json"),
    ("config.json", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/config.json"),
    ("special_tokens_map.json", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/special_tokens_map.json"),
    ("tokenizer_config.json", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer_config.json"),
];

pub fn download_model(model_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(model_dir)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    for (filename, url) in HF_MODEL_URLS {
        let dest = model_dir.join(filename);
        if dest.exists() {
            log::info!("{} already exists, skipping", filename);
            continue;
        }
        log::info!("Downloading {} ...", filename);
        let resp = client.get(*url).send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to download {}: HTTP {}", filename, resp.status());
        }
        let bytes = resp.bytes()?;
        std::fs::write(&dest, &bytes)?;
        log::info!("Downloaded {} ({} bytes)", filename, bytes.len());
    }
    Ok(())
}

pub fn is_model_downloaded(model_dir: &Path) -> bool {
    HF_MODEL_URLS
        .iter()
        .all(|(f, _)| model_dir.join(f).exists())
}

pub fn create_model(model_dir: &Path) -> Result<TextEmbedding> {
    if model_dir.join("model.onnx").exists() {
        let onnx_file = std::fs::read(model_dir.join("model.onnx"))?;
        let tokenizer_files = TokenizerFiles {
            tokenizer_file: std::fs::read(model_dir.join("tokenizer.json"))?,
            config_file: std::fs::read(model_dir.join("config.json"))?,
            special_tokens_map_file: std::fs::read(model_dir.join("special_tokens_map.json"))?,
            tokenizer_config_file: std::fs::read(model_dir.join("tokenizer_config.json"))?,
        };
        let user_model = UserDefinedEmbeddingModel::new(onnx_file, tokenizer_files);
        let model = TextEmbedding::try_new_from_user_defined(
            user_model,
            InitOptionsUserDefined::default(),
        )?;
        Ok(model)
    } else {
        anyhow::bail!("Model not found at {}. Download with: curl -L -o {}/model.onnx https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx", model_dir.display(), model_dir.display())
    }
}

pub fn embed_and_store(conn: &Connection, model_dir: &Path, batch_size: usize) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT id, content FROM chunks WHERE id NOT IN (SELECT chunk_id FROM embeddings WHERE model = ?1)")?;
    let rows: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![MODEL_NAME], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        return Ok(0);
    }

    let mut model = create_model(model_dir)?;
    let mut total = 0;

    for chunk in rows.chunks(batch_size) {
        let texts: Vec<&str> = chunk.iter().map(|(_, c)| c.as_str()).collect();
        let embeddings = model.embed(texts, None)?;

        for ((id, _), emb) in chunk.iter().zip(embeddings.iter()) {
            conn.execute(
                "INSERT OR REPLACE INTO embeddings (chunk_id, model, dimensions, vector) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, MODEL_NAME, DIMENSIONS as i64, f32_to_bytes(emb)],
            )?;
            total += 1;
        }
    }

    Ok(total)
}

pub fn embed_and_store_with_profile(
    conn: &Connection,
    model_dir: &Path,
    profile: &ResourceProfile,
) -> Result<usize> {
    if !profile.embedding_enabled {
        log::info!("Embedding disabled (low resource mode)");
        return Ok(0);
    }
    embed_and_store(conn, model_dir, profile.batch_size)
}

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

pub fn reciprocal_rank_fusion(kw: &[String], vec: &[String], k: u64) -> Vec<(String, f32)> {
    let mut scores: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    for (r, id) in kw.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k as f32 + r as f32 + 1.0);
    }
    for (r, id) in vec.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k as f32 + r as f32 + 1.0);
    }
    let mut fused: Vec<(String, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

pub fn load_all_embeddings(conn: &Connection) -> Result<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare("SELECT chunk_id, vector FROM embeddings WHERE model = ?1")?;
    let rows = stmt.query_map(rusqlite::params![MODEL_NAME], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (id, bytes) = row?;
        result.push((id, bytes_to_f32(&bytes)));
    }
    Ok(result)
}

pub fn store_embeddings(conn: &Connection, ids: &[String], embeddings: &[Vec<f32>]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO embeddings (chunk_id, model, dimensions, vector) VALUES (?1, ?2, ?3, ?4)"
    )?;
    for (id, emb) in ids.iter().zip(embeddings.iter()) {
        stmt.execute(rusqlite::params![
            id,
            MODEL_NAME,
            DIMENSIONS as i64,
            f32_to_bytes(emb)
        ])?;
    }
    Ok(())
}

pub fn delete_embeddings_for_file(conn: &Connection, file_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id = ?1)",
        rusqlite::params![file_id],
    )?;
    Ok(())
}

pub fn import_embeddings_json(conn: &Connection, json_path: &str) -> Result<usize> {
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
            rusqlite::params![chunk_id, MODEL_NAME, DIMENSIONS as i64, f32_to_bytes(&embedding)],
        )?;
        imported += 1;
    }
    Ok(imported)
}

pub fn export_chunks_for_embedding(conn: &Connection, output_path: &str) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT id, content FROM chunks")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut items = Vec::new();
    for row in rows {
        let (id, content) = row?;
        items.push(serde_json::json!({"chunk_id": id, "content": content}));
    }

    let json = serde_json::to_string_pretty(&items)?;
    std::fs::write(output_path, json)?;
    Ok(items.len())
}

pub fn f32_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for &f in v {
        b.extend_from_slice(&f.to_le_bytes());
    }
    b
}

pub fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub struct EmbeddingCache {
    cache: parking_lot::Mutex<lru::LruCache<String, Vec<f32>>>,
}

impl EmbeddingCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: parking_lot::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(capacity.max(1000)).unwrap(),
            )),
        }
    }

    pub fn from_resource(profile: &aikd_core::ResourceProfile) -> Self {
        let capacity = (profile.total_ram_bytes as usize / (DIMENSIONS * 4)) / 5;
        Self::new(capacity.max(1000))
    }

    pub fn get(&self, chunk_id: &str) -> Option<Vec<f32>> {
        self.cache.lock().get(chunk_id).cloned()
    }

    pub fn put(&self, chunk_id: String, vector: Vec<f32>) {
        self.cache.lock().put(chunk_id, vector);
    }

    pub fn preload_for_ids(&self, chunk_ids: &[String], conn: &Connection) -> Result<()> {
        let mut cache = self.cache.lock();
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
            if let Ok(bytes) = stmt.query_row(rusqlite::params![MODEL_NAME, id.as_str()], |row| {
                row.get::<_, Vec<u8>>(1)
            }) {
                cache.put(id.clone(), bytes_to_f32(&bytes));
            }
        }

        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cache.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);

        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_vector_search() {
        let all = vec![
            ("a".to_string(), vec![1.0, 0.0, 0.0]),
            ("b".to_string(), vec![0.0, 1.0, 0.0]),
            ("c".to_string(), vec![0.7, 0.7, 0.0]),
        ];
        let query = vec![1.0, 0.0, 0.0];
        let results = vector_search(&query, &all, 2);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "a");
    }

    #[test]
    fn test_reciprocal_rank_fusion() {
        let kw = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let vec = vec!["b".to_string(), "a".to_string(), "d".to_string()];
        let fused = reciprocal_rank_fusion(&kw, &vec, 60);
        assert_eq!(fused.len(), 4);
        assert!(fused[0].1 > fused[2].1);
    }

    #[test]
    fn test_f32_roundtrip() {
        let original = vec![1.0_f32, 2.5, -3.7, 0.0, 100.123];
        let bytes = f32_to_bytes(&original);
        let recovered = bytes_to_f32(&bytes);
        for (a, b) in original.iter().zip(recovered.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn test_embedding_cache_new() {
        let cache = EmbeddingCache::new(1000);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_embedding_cache_put_get() {
        let cache = EmbeddingCache::new(100);
        cache.put("c1".into(), vec![1.0, 0.0, 0.0]);
        assert_eq!(cache.len(), 1);
        let v = cache.get("c1");
        assert!(v.is_some());
        assert_eq!(v.unwrap(), vec![1.0, 0.0, 0.0]);
    }

    #[test]
    fn test_embedding_cache_miss() {
        let cache = EmbeddingCache::new(100);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_f32_empty() {
        let bytes = f32_to_bytes(&[]);
        assert!(bytes.is_empty());
        let recovered = bytes_to_f32(&bytes);
        assert!(recovered.is_empty());
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }
}
