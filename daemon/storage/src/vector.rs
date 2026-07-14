//! Pure vector helpers backing the `note_embeddings` table (migration v4):
//! BLOB encoding for embedding vectors and brute-force cosine similarity.
//!
//! Per the Etappe-3 context decision (`docs/ROADMAP.md`), MerkWerk does not
//! depend on the `sqlite-vec` C extension — it is fragile to cross-compile
//! for Windows. Instead, embeddings are stored as a plain BLOB
//! (`note_embeddings.vector`) and similarity search
//! (`crate::Store::search_notes_semantic`) scores every row in Rust with
//! [`cosine_similarity`] instead of using a vector index. That brute-force
//! scan is fast enough for a personal, single-user note collection.
//!
//! All three functions here are free of I/O and side effects, so they are
//! exercised with plain native unit tests (no database needed) and
//! re-exported from the crate root for `merkwerk-daemon` to use directly
//! when it produces/consumes embeddings.

/// Encode `vector` as a little-endian `f32` BLOB: 4 bytes per element, in
/// order — the exact encoding stored in `note_embeddings.vector` (see
/// migration v4 in `crate::migrations`). Inverse of [`decode_vector`].
pub fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Decode a little-endian `f32` BLOB back into a vector. Inverse of
/// [`encode_vector`].
///
/// Robustness: `bytes` is read in 4-byte chunks via
/// [`slice::chunks_exact`], so a trailing partial element — `bytes.len()`
/// not a multiple of 4, which should never happen for a BLOB written by
/// [`encode_vector`], but could for a hand-crafted or corrupted row — is
/// silently ignored rather than causing a panic or an error. Callers that
/// need to detect malformed input should check `bytes.len() % 4 == 0`
/// themselves before decoding.
pub fn decode_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let array: [u8; 4] = chunk
                .try_into()
                .expect("chunks_exact(4) always yields 4-byte slices");
            f32::from_le_bytes(array)
        })
        .collect()
}

/// Cosine similarity between `a` and `b`: `dot(a, b) / (‖a‖ * ‖b‖)`, in
/// `[-1.0, 1.0]` for non-degenerate inputs.
///
/// Returns `0.0` — instead of erroring, panicking, or producing NaN — when:
///   - `a` and `b` have different lengths (not comparable), or
///   - either vector has zero magnitude (a true "zero vector", or the
///     degenerate case of two empty slices), which would otherwise divide
///     by zero.
///
/// This makes `0.0` a safe "no/lowest similarity" sentinel that
/// [`crate::Store::search_notes_semantic`] can sort on without special-casing
/// its callers.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- encode_vector / decode_vector ---------------------------------

    #[test]
    fn encode_decode_roundtrip() {
        let original = vec![1.0_f32, -2.5, 0.0, 3.14159, f32::MIN, f32::MAX];
        let encoded = encode_vector(&original);
        assert_eq!(encoded.len(), original.len() * 4);
        assert_eq!(decode_vector(&encoded), original);
    }

    #[test]
    fn encode_decode_roundtrip_empty_vector() {
        let original: Vec<f32> = Vec::new();
        let encoded = encode_vector(&original);
        assert!(encoded.is_empty());
        assert!(decode_vector(&encoded).is_empty());
    }

    #[test]
    fn encode_vector_is_little_endian() {
        // 1.0f32 in little-endian IEEE-754 bytes is 00 00 80 3F.
        assert_eq!(encode_vector(&[1.0_f32]), vec![0x00, 0x00, 0x80, 0x3F]);
    }

    #[test]
    fn decode_vector_of_empty_bytes_is_empty() {
        assert_eq!(decode_vector(&[]), Vec::<f32>::new());
    }

    #[test]
    fn decode_vector_ignores_trailing_partial_bytes() {
        // Two valid f32s (8 bytes) plus one dangling extra byte: the
        // trailing byte does not form a full f32, so it is dropped instead
        // of panicking or erroring (see decode_vector's doc comment).
        let mut bytes = encode_vector(&[1.0_f32, 2.0_f32]);
        bytes.push(0xFF);
        assert_eq!(decode_vector(&bytes), vec![1.0_f32, 2.0_f32]);
    }

    // ---- cosine_similarity ----------------------------------------------

    #[test]
    fn cosine_similarity_identical_vectors_is_near_one() {
        let v = [1.0_f32, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6, "expected ~1.0, got {sim}");
    }

    #[test]
    fn cosine_similarity_orthogonal_vectors_is_near_zero() {
        let a = [1.0_f32, 0.0];
        let b = [0.0_f32, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6, "expected ~0.0, got {sim}");
    }

    #[test]
    fn cosine_similarity_opposite_vectors_is_near_negative_one() {
        let a = [1.0_f32, 0.0];
        let b = [-1.0_f32, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6, "expected ~-1.0, got {sim}");
    }

    #[test]
    fn cosine_similarity_different_length_is_zero() {
        let a = [1.0_f32, 2.0, 3.0];
        let b = [1.0_f32, 2.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn cosine_similarity_zero_vector_is_zero() {
        let zero = [0.0_f32, 0.0, 0.0];
        let other = [1.0_f32, 2.0, 3.0];
        assert_eq!(cosine_similarity(&zero, &other), 0.0);
        assert_eq!(cosine_similarity(&other, &zero), 0.0);
        assert_eq!(cosine_similarity(&zero, &zero), 0.0);
    }

    #[test]
    fn cosine_similarity_empty_vectors_is_zero() {
        let a: [f32; 0] = [];
        let b: [f32; 0] = [];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }
}
