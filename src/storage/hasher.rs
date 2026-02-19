use md5::{Digest as Md5Digest, Md5};
use sha2::{Digest as ShaDigest, Sha256};

/// Streaming hash result
pub struct HashResult {
    /// MD5 hex digest (used for S3 ETag)
    pub md5: String,
    /// SHA256 hex digest (used for integrity checks)
    pub sha256: String,
}

/// Compute both MD5 and SHA256 hashes of data in a single pass
pub fn compute_hashes(data: &[u8]) -> HashResult {
    let mut md5_hasher = Md5::new();
    let mut sha256_hasher = Sha256::new();

    md5_hasher.update(data);
    sha256_hasher.update(data);

    HashResult {
        md5: hex::encode(md5_hasher.finalize()),
        sha256: hex::encode(sha256_hasher.finalize()),
    }
}

/// Compute MD5 hash only (for ETag)
pub fn compute_md5(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Compute SHA256 hash (for x-amz-content-sha256)
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_computation() {
        let data = b"hello world";
        let result = compute_hashes(data);
        assert_eq!(result.md5, "5eb63bbbe01eeed093cb22bb8f5acdc3");
        assert_eq!(
            result.sha256,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_empty_hash() {
        let data = b"";
        let result = compute_hashes(data);
        assert_eq!(result.md5, "d41d8cd98f00b204e9800998ecf8427e");
    }
}
