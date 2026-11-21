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
