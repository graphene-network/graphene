//! Cache key computation for L1/L2/L3 hierarchy.
//!
//! The cache is organized into three layers:
//! - **L1 (Kernels)**: Pre-built unikernel binaries (~100% hit rate)
//! - **L2 (Dependencies)**: Kernel + requirements (~95% hit rate)
//! - **L3 (Full Build)**: Kernel + requirements + user code
//!
//! Each layer builds on the previous, enabling maximum cache reuse.

use blake3::Hash;

/// Compute L1 cache key from kernel specification.
///
/// L1 caches pre-built unikernel binaries. The kernel spec typically
/// includes runtime and version (e.g., "python:3.12", "node:20").
///
/// Expected hit rate: ~100% (kernels are pre-built)
pub fn l1_key(kernel_spec: &str) -> Hash {
    blake3::hash(kernel_spec.as_bytes())
}

/// Compute L2 cache key from L1 key and requirements.
///
/// L2 caches builds with dependencies installed. Requirements are
/// sorted for deterministic hashing.
///
/// Expected hit rate: ~95% (dependencies rarely change)
pub fn l2_key(l1: &Hash, requirements: &[String]) -> Hash {
    let mut sorted = requirements.to_vec();
    sorted.sort();

    let mut hasher = blake3::Hasher::new();
    hasher.update(l1.as_bytes());
    hasher.update(b"|");
    hasher.update(sorted.join("|").as_bytes());
    hasher.finalize()
}

/// Compute L3 cache key from L2 key and code hash.
///
/// L3 caches complete builds including user code. This is the
/// final cache layer checked before triggering a new build.
pub fn l3_key(l2: &Hash, code_hash: &Hash) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(l2.as_bytes());
    hasher.update(b"|");
    hasher.update(code_hash.as_bytes());
    hasher.finalize()
}

/// Compute L3 cache key directly from all inputs.
///
/// Convenience function that computes the full L3 key from raw inputs.
pub fn full_build_key(kernel_spec: &str, requirements: &[String], code_hash: &Hash) -> Hash {
    let l1 = l1_key(kernel_spec);
    let l2 = l2_key(&l1, requirements);
    l3_key(&l2, code_hash)
}

/// Hash file contents to produce a code hash.
pub fn hash_bytes(data: &[u8]) -> Hash {
    blake3::hash(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l1_key_deterministic() {
        let key1 = l1_key("python:3.12");
        let key2 = l1_key("python:3.12");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_l1_key_differs_by_spec() {
        let key1 = l1_key("python:3.12");
        let key2 = l1_key("node:20");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_l2_key_deterministic_with_order() {
        let l1 = l1_key("python:3.12");
        let key1 = l2_key(&l1, &["pandas".into(), "numpy".into()]);
        let key2 = l2_key(&l1, &["numpy".into(), "pandas".into()]);
        // Order shouldn't matter (sorted internally)
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_l2_key_differs_by_requirements() {
        let l1 = l1_key("python:3.12");
        let key1 = l2_key(&l1, &["pandas".into()]);
        let key2 = l2_key(&l1, &["numpy".into()]);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_l3_key_differs_by_code() {
        let l1 = l1_key("python:3.12");
        let l2 = l2_key(&l1, &["pandas".into()]);
        let code1 = hash_bytes(b"print('hello')");
        let code2 = hash_bytes(b"print('world')");

        let key1 = l3_key(&l2, &code1);
        let key2 = l3_key(&l2, &code2);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_full_build_key() {
        let code_hash = hash_bytes(b"test code");
        let key1 = full_build_key("python:3.12", &["pandas".into()], &code_hash);

        // Manually compute the same key
        let l1 = l1_key("python:3.12");
        let l2 = l2_key(&l1, &["pandas".into()]);
        let key2 = l3_key(&l2, &code_hash);

        assert_eq!(key1, key2);
    }
}
