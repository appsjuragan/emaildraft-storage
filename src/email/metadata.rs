use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

const SUBJECT_PREFIX: &str = "OBJMAIL:";

/// Metadata stored in the email subject line (base64-encoded JSON).
/// This ensures chunk metadata survives even if the database is lost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Schema version for forward compatibility
    pub v: u32,
    /// Bucket name
    pub bucket: String,
    /// Object key
    pub key: String,
    /// 0-based chunk index
    pub chunk_idx: u32,
    /// Total number of chunks for this object
    pub total_chunks: u32,
    /// Object UUID
    pub object_id: String,
    /// SHA256 hash of this chunk's data
    pub chunk_hash: String,
    /// Total object size in bytes
    pub total_size: u64,
    /// Content type of the object
    pub content_type: String,
}

impl ChunkMetadata {
    /// Encode metadata into a subject line: "OBJMAIL:<base64url_json>"
    pub fn encode_subject(&self) -> Result<String> {
        let json = serde_json::to_string(self).context("Failed to serialize chunk metadata")?;
        let encoded = URL_SAFE_NO_PAD.encode(json.as_bytes());
        Ok(format!("{}{}", SUBJECT_PREFIX, encoded))
    }

    /// Decode metadata from a subject line
    pub fn decode_subject(subject: &str) -> Result<Self> {
        let encoded = subject
            .strip_prefix(SUBJECT_PREFIX)
            .context("Subject does not have OBJMAIL: prefix")?;
        let json_bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .context("Failed to base64-decode subject")?;
        let metadata: Self = serde_json::from_slice(&json_bytes)
            .context("Failed to deserialize chunk metadata from JSON")?;
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let meta = ChunkMetadata {
            v: 1,
            bucket: "test-bucket".to_string(),
            key: "path/to/file.dat".to_string(),
            chunk_idx: 0,
            total_chunks: 5,
            object_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            chunk_hash: "abcdef1234567890".to_string(),
            total_size: 104857600,
            content_type: "application/octet-stream".to_string(),
        };

        let subject = meta.encode_subject().unwrap();
        assert!(subject.starts_with("OBJMAIL:"));

        let decoded = ChunkMetadata::decode_subject(&subject).unwrap();
        assert_eq!(decoded.bucket, "test-bucket");
        assert_eq!(decoded.key, "path/to/file.dat");
        assert_eq!(decoded.chunk_idx, 0);
        assert_eq!(decoded.total_chunks, 5);
        assert_eq!(decoded.total_size, 104857600);
    }
}
