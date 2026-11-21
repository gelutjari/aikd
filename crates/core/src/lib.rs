pub mod agents;
pub mod config;
pub mod error;
pub mod fusion;
pub mod platform;
pub mod resource;
pub mod security;
pub mod vector;
pub mod types;

pub use config::Config;
pub use error::AikdError;
pub use resource::ResourceProfile;
pub use vector::{bytes_to_f32, cosine_similarity, export_chunks_for_embedding, f32_to_bytes, import_embeddings_json, load_all_embeddings, vector_search, EmbeddingCache, SemanticProvider, EMBEDDING_DIMENSIONS, EMBEDDING_MODEL};
pub use types::*;
