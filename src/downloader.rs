// src/downloader.rs
use crate::state::{self, DownloadState};
use crate::utils;
use anyhow::Result;
use reqwest::Client;
use tokio::fs;

/// Prepares a download by either loading an existing state or creating a new one.
///
/// This consolidates the logic for:
/// 1. Checking if a state file exists.
/// 2. If yes, validating the URL.
/// 3. If no (or mismatch), fetching file size, pre-allocating the file, and calculating chunks.
pub async fn prepare_download(
    url: &str,
    output_filename: String,
    threads: u8,
    client: &Client,
) -> Result<(DownloadState, String, u64)> {
    let state_filename = format!("{}.state.json", output_filename);

    // 1. Try to load existing state
    if let Ok(json) = fs::read_to_string(&state_filename).await
        && let Ok(state) = serde_json::from_str::<DownloadState>(&json)
        && state.url == url
    {
        // Determine total size from chunks (sum of all chunk sizes)
        let total_size = state.chunks.last().map(|c| c.end + 1).unwrap_or(0);
        return Ok((state, state_filename, total_size));
    }

    // 2. Start New Download
    let size = utils::get_file_size(url, client).await?;

    // Pre-allocate file (Optimization: prevents fragmentation)
    let file = fs::File::create(&output_filename).await?;
    file.set_len(size).await?;

    let chunks = utils::calculate_chunks(size, threads as u64);
    let state = DownloadState {
        url: url.to_string(),
        chunks,
    };

    state::save_state(&state, &state_filename).await?;

    Ok((state, state_filename, size))
}
