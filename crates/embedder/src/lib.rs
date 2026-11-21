use aikd_core::ResourceProfile;
use anyhow::Result;
#[cfg(feature = "semantic")]
use fastembed::{InitOptionsUserDefined, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel};
use rusqlite::Connection;
use std::path::Path;

// Pure vector math, storage IO and cache live in `aikd_core::vector` so the
// default build uses them without linking ONNX. Re-exported below (single block).

/// True only in builds compiled with the `semantic` feature (ONNX linked).
/// Default builds are zero-ONNX: semantic inference is unavailable and every
/// semantic entry point must fail honestly instead of downloading or stubbing.
pub const SEMANTIC_COMPILED: bool = cfg!(feature = "semantic");

/// True when the `semantic-ext` HTTP provider backend is compiled in.
/// Zero-ONNX: inference happens in an external process behind HTTP.
pub const SEMANTIC_EXT_COMPILED: bool = cfg!(feature = "semantic-ext");

/// External embedding provider (OpenAI-compatible HTTP endpoint).
/// Implements the core [`aikd_core::SemanticProvider`] contract: AIKD owns
/// schemas/provenance/lifecycle, the provider only fills inference.
#[cfg(feature = "semantic-ext")]
pub struct ExternalProvider {
    pub name: String,
    pub model_id: String,
    pub dimensions: usize,
    pub endpoint: String,
    pub api_key: String,
}

#[cfg(feature = "semantic-ext")]
impl ExternalProvider {
    pub fn from_env(endpoint: &str, model: &str, api_key_env: &str) -> Result<Self> {
        let api_key = std::env::var(api_key_env)
            .map_err(|_| anyhow::anyhow!("env {api_key_env} is not set (key never lives in config)"))?;
        if endpoint.is_empty() {
            anyhow::bail!("embedding.endpoint is empty; set it to an OpenAI-compatible /embeddings URL");
        }
        Ok(Self { name: "external-http".into(), model_id: model.into(), dimensions: 0, endpoint: endpoint.into(), api_key })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        // Never use reqwest::blocking inside a tokio runtime (panics on drop).
        // Run the async fetch on a plain thread with its own runtime.
        let endpoint = self.endpoint.clone();
        let api_key = self.api_key.clone();
        let model_id = self.model_id.clone();
        let owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
        let job = move || -> Result<Vec<Vec<f32>>> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(Self::embed_batch_async(&endpoint, &api_key, &model_id, &owned))
        };
        match tokio::runtime::Handle::try_current() {
            // Inside async context: escape to a plain OS thread and run the
            // fetch there (no nesting: this thread drives no runtime).
            Ok(_) => std::thread::spawn(job)
                .join()
                .map_err(|_| anyhow::anyhow!("embed thread panicked"))?,
            Err(_) => job(),
        }
    }

    async fn embed_batch_async(endpoint: &str, api_key: &str, model_id: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        let resp = client
            .post(endpoint)
            .bearer_auth(api_key)
            .json(&serde_json::json!({"model": model_id, "input": texts}))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("embedding endpoint {endpoint} returned HTTP {}", resp.status());
        }
        let v: serde_json::Value = resp.json().await?;
        let arr = v["data"].as_array().ok_or_else(|| anyhow::anyhow!("bad embeddings response: no data[]"))?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let emb: Vec<f32> = item["embedding"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect())
                .unwrap_or_default();
            if emb.is_empty() {
                anyhow::bail!("bad embeddings response: empty vector");
            }
            out.push(emb);
        }
        Ok(out)
    }
}

#[cfg(feature = "semantic-ext")]
impl aikd_core::SemanticProvider for ExternalProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn model_id(&self) -> &str {
        &self.model_id
    }
    fn dimensions(&self) -> usize {
        self.dimensions
    }
    fn is_available(&self) -> bool {
        !self.endpoint.is_empty() && !self.api_key.is_empty()
    }
    fn embed_texts(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.embed_batch(texts)
    }
}

/// Store vectors from an external provider for chunks missing them.
/// Dimensions are learned from the first response (provider-declared).
#[cfg(feature = "semantic-ext")]
pub fn embed_and_store_external(conn: &Connection, provider: &ExternalProvider, batch_size: usize) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT id, content FROM chunks WHERE id NOT IN (SELECT chunk_id FROM embeddings WHERE model = ?1)")?;
    let rows: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![provider.model_id()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    if rows.is_empty() {
        return Ok(0);
    }
    let mut total = 0;
    for chunk in rows.chunks(batch_size.max(1)) {
        let texts: Vec<&str> = chunk.iter().map(|(_, c)| c.as_str()).collect();
        let embeddings = provider.embed_batch(&texts)?;
        for ((id, _), emb) in chunk.iter().zip(embeddings.iter()) {
            conn.execute(
                "INSERT OR REPLACE INTO embeddings (chunk_id, model, dimensions, vector) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, provider.model_id(), emb.len() as i64, aikd_core::f32_to_bytes(emb)],
            )?;
            total += 1;
        }
    }
    Ok(total)
}

const HF_MODEL_URLS: &[(&str, &str)] = &[
    ("model.onnx", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx"),
    ("tokenizer.json", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json"),
    ("config.json", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/config.json"),
    ("special_tokens_map.json", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/special_tokens_map.json"),
    ("tokenizer_config.json", "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer_config.json"),
];

/// Download the embedding model files from HuggingFace.
/// Uses async reqwest to avoid blocking the tokio runtime.
/// Falls back to blocking if called outside async context.
#[cfg(feature = "semantic")]
pub fn download_model(model_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(model_dir)?;

    // Clone the path to owned PathBuf so it can be moved into threads
    let model_dir_owned = model_dir.to_path_buf();

    // Try to get the current tokio runtime handle
    let rt_handle = tokio::runtime::Handle::try_current();

    match rt_handle {
        Ok(handle) => {
            // We're in an async context — spawn on a blocking thread
            // to avoid freezing the tokio runtime
            std::thread::spawn(move || {
                handle.block_on(async { download_model_async(&model_dir_owned).await })
            })
            .join()
            .map_err(|_| anyhow::anyhow!("Download thread panicked"))?
        }
        Err(_) => {
            // Not in async context — use blocking directly
            download_model_blocking(model_dir)
        }
    }
}

#[cfg(not(feature = "semantic"))]
/// Zero-ONNX builds never download models. Use an external provider via `aikd import`.
pub fn download_model(_model_dir: &Path) -> Result<()> {
    anyhow::bail!("semantic unavailable: default builds are zero-ONNX and never download models; rebuild with --features semantic, or import vectors from an external provider via `aikd import`")
}

/// Async model download implementation.
#[cfg(feature = "semantic")]
async fn download_model_async(model_dir: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    for (filename, url) in HF_MODEL_URLS {
        let dest = model_dir.join(filename);
        if dest.exists() {
            log::info!("{filename} already exists, skipping");
            continue;
        }
        log::info!("Downloading {filename} ...");
        let resp = client.get(*url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to download {}: HTTP {}", filename, resp.status());
        }
        let bytes = resp.bytes().await?;
        tokio::fs::write(&dest, &bytes).await?;
        log::info!("Downloaded {} ({} bytes)", filename, bytes.len());
    }
    Ok(())
}

/// Blocking model download for non-async contexts.
#[cfg(feature = "semantic")]
fn download_model_blocking(model_dir: &Path) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    for (filename, url) in HF_MODEL_URLS {
        let dest = model_dir.join(filename);
        if dest.exists() {
            log::info!("{filename} already exists, skipping");
            continue;
        }
        log::info!("Downloading {filename} ...");
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

#[cfg(feature = "semantic")]
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
        TextEmbedding::try_new_from_user_defined(user_model, InitOptionsUserDefined::default())
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else {
        anyhow::bail!("Model not found at {}. Download with: curl -L -o {}/model.onnx https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx", model_dir.display(), model_dir.display())
    }
}

/// Zero-ONNX builds: semantic inference is unavailable. Fail honestly.
#[cfg(not(feature = "semantic"))]
pub fn create_model(_model_dir: &Path) -> Result<()> {
    anyhow::bail!("semantic unavailable: rebuild with --features semantic to enable ONNX inference")
}

#[cfg(feature = "semantic")]
pub fn embed_and_store(conn: &Connection, model_dir: &Path, batch_size: usize) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT id, content FROM chunks WHERE id NOT IN (SELECT chunk_id FROM embeddings WHERE model = ?1)")?;
    let rows: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![MODEL_NAME], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| match r {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("Failed to read chunk row: {e}");
                None
            }
        })
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

#[cfg(not(feature = "semantic"))]
pub fn embed_and_store(_conn: &Connection, _model_dir: &Path, _batch_size: usize) -> Result<usize> {
    anyhow::bail!("semantic unavailable: default builds are zero-ONNX; rebuild with --features semantic for local embeddings, or import vectors from an external provider via `aikd import`")
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

// Pure vector math, storage IO and cache live in `aikd_core::vector` so the
// default build uses them without linking ONNX. Re-exported here for
// backward compatibility of the `aikd_embedder::` paths.
pub use aikd_core::{
    bytes_to_f32, cosine_similarity, export_chunks_for_embedding, f32_to_bytes,
    import_embeddings_json, load_all_embeddings, vector_search, EmbeddingCache, SemanticProvider,
    EMBEDDING_DIMENSIONS as DIMENSIONS, EMBEDDING_MODEL as MODEL_NAME,
};
pub use aikd_core::fusion::reciprocal_rank_fusion;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_onnx_default_has_no_semantic() {
        // Default build: pure helpers work, inference entry points fail honestly.
        assert!(!SEMANTIC_COMPILED);
        assert!(create_model(std::path::Path::new("nope")).is_err());
        assert!(download_model(std::path::Path::new("nope")).is_err());
        assert!(embed_and_store_with_profile(
            &rusqlite::Connection::open_in_memory().unwrap(),
            std::path::Path::new("nope"),
            &aikd_core::ResourceProfile::detect(),
        )
        .is_err());
    }

    #[test]
    fn pure_helpers_come_from_core() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        let hits = vector_search(&[1.0, 0.0], &[("a".to_string(), vec![1.0, 0.0])], 1);
        assert_eq!(hits.len(), 1);
    }
}

#[cfg(test)]
mod ext_tests {
    #[test]
    #[cfg(feature = "semantic-ext")]
    fn external_provider_contract_holds() {
        use aikd_core::SemanticProvider;
        // Missing env → honest error, never a silent stub.
        assert!(super::ExternalProvider::from_env("http://127.0.0.1:9/embed", "m", "AIKD_TEST_MISSING_KEY_XYZ").is_err());
        // Empty endpoint → honest error even with key present.
        std::env::set_var("AIKD_TEST_KEY_TMP", "k");
        assert!(super::ExternalProvider::from_env("", "m", "AIKD_TEST_KEY_TMP").is_err());
        std::env::remove_var("AIKD_TEST_KEY_TMP");
        assert!(super::SEMANTIC_EXT_COMPILED);
        assert!(!super::SEMANTIC_COMPILED);
    }

    #[test]
    #[cfg(not(any(feature = "semantic-ext", feature = "semantic")))]
    fn zero_onnx_stays_zero() {
        assert!(!super::SEMANTIC_COMPILED);
        assert!(!super::SEMANTIC_EXT_COMPILED);
    }
}
