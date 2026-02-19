use anyhow::Result;
use sha2::{Digest as ShaDigest, Sha256};

/// A chunk of file data ready to be stored as an email draft
pub struct ChunkData {
    pub index: u32,
    pub data: Vec<u8>,
    pub hash: String,
    pub size: u64,
}

/// Split data into chunks of specified size and compute per-chunk SHA256 hash
pub fn chunk_data(data: &[u8], chunk_size: u64) -> Vec<ChunkData> {
    let chunk_size = chunk_size as usize;
    let mut chunks = Vec::new();

    for (i, chunk_bytes) in data.chunks(chunk_size).enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(chunk_bytes);
        let hash = hex::encode(hasher.finalize());

        chunks.push(ChunkData {
            index: i as u32,
            data: chunk_bytes.to_vec(),
            hash,
            size: chunk_bytes.len() as u64,
        });
    }

    chunks
}

/// Split a file into chunks from a file path
pub async fn chunk_file(path: &std::path::Path, chunk_size: u64) -> Result<Vec<ChunkData>> {
    let data = tokio::fs::read(path).await?;
    Ok(chunk_data(&data, chunk_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunking() {
        let data = vec![0u8; 100];
        let chunks = chunk_data(&data, 30);
        assert_eq!(chunks.len(), 4); // 30+30+30+10
        assert_eq!(chunks[0].size, 30);
        assert_eq!(chunks[1].size, 30);
        assert_eq!(chunks[2].size, 30);
        assert_eq!(chunks[3].size, 10);
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[3].index, 3);
    }

    #[test]
    fn test_exact_chunk() {
        let data = vec![0u8; 60];
        let chunks = chunk_data(&data, 30);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].size, 30);
        assert_eq!(chunks[1].size, 30);
    }

    #[test]
    fn test_single_chunk() {
        let data = vec![1u8; 10];
        let chunks = chunk_data(&data, 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].size, 10);
    }

    #[test]
    fn test_chunk_hashes_differ() {
        let mut data = vec![0u8; 60];
        data[30] = 1; // make second chunk different
        let chunks = chunk_data(&data, 30);
        assert_ne!(chunks[0].hash, chunks[1].hash);
    }
}
