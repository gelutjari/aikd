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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let v = vec![1.0f32, -2.5, 0.0];
        assert_eq!(bytes_to_f32(&f32_to_bytes(&v)), v);
    }
}
