//! Persistent download state and helpers.
//!
//! The structures in this module are serialized to disk as JSON to enable
//! crash recovery and resuming partially completed downloads.
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Represents a specific range of bytes within a file to be downloaded.
///
/// The range is inclusive, meaning `start` and `end` are both part of the chunk.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Chunk {
    /// Index of the chunk
    pub index: usize,
    /// The starting byte index (0-based).
    pub start: u64,
    /// The ending byte index.
    pub end: u64,
    /// Whether this specific chunk has been fully downloaded and written to disk.
    pub completed: bool,
    /// Number of bytes already downloaded and written for this chunk.
    #[serde(default)]
    pub current_offset: u64,
}

/// Represents the persistent state of a download operation.
///
/// This struct is serialized to JSON to allow resuming downloads after a crash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    /// The source URL of the file.
    pub url: String,
    /// A list of all chunks and their current status.
    pub chunks: Vec<Chunk>,
}

/// Saves the current download state to a JSON file.
///
/// # Errors
///
/// This function will return an error if the file cannot be created or written to.
pub async fn save_state(state: &DownloadState, filename: &str) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    tokio::fs::write(filename, json).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test] // Logic uses async fs, so we need the tokio test attribute
    async fn test_save_and_load_state() -> Result<()> {
        let dir = tempdir()?; // Create temp folder
        let file_path = dir.path().join("test.state.json");
        let path_str = file_path.to_str().unwrap();

        // 1. Create a dummy state
        let state = DownloadState {
            url: "http://example.com".to_string(),
            chunks: vec![
                Chunk {
                    index: 0,
                    start: 0,
                    end: 10,
                    completed: true,
                    current_offset: 0,
                },
                Chunk {
                    index: 1,
                    start: 11,
                    end: 20,
                    completed: false,
                    current_offset: 0,
                },
            ],
        };

        // 2. Save it
        save_state(&state, path_str).await?;

        // 3. Read it back manually to verify JSON structure
        let content = tokio::fs::read_to_string(path_str).await?;
        let loaded_state: DownloadState = serde_json::from_str(&content)?;

        assert_eq!(loaded_state.url, "http://example.com");
        assert_eq!(loaded_state.chunks.len(), 2);
        assert!(loaded_state.chunks[0].completed);
        assert!(!loaded_state.chunks[1].completed);

        Ok(())
    }
}
