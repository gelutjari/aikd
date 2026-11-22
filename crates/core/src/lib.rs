pub mod agents;
pub mod config;
pub mod error;
pub mod fusion;
pub mod platform;
pub mod resource;
pub mod security;
pub mod types;
pub mod vector;

pub use config::{Config, RetentionConfig};
pub use error::AikdError;
pub use resource::ResourceProfile;
pub use types::*;
pub use vector::{
    bytes_to_f32, cosine_similarity, export_chunks_for_embedding, f32_to_bytes,
    import_embeddings_json, load_all_embeddings, vector_search, EmbeddingCache, SemanticProvider,
    EMBEDDING_DIMENSIONS, EMBEDDING_MODEL,
};
