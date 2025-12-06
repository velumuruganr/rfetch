use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Represents a specific range of bytes within a file to be downloaded.
/// 
/// The range is inclusive, meaning `start` and `end` are both part of the chunk.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Chunk {
    /// The starting byte index (0-based).
    pub start: u64,
    /// The ending byte index.
    pub end: u64,
    /// Whether this specific chunk has been fully downloaded and written to disk.
    pub completed: bool,
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
