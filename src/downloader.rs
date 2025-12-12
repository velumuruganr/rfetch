//! Helpers for preparing and initializing download operations.
//!
//! This module contains functions to create or resume a download by
//! managing the persistent `*.state.json` file, pre-allocating the
//! destination file, and returning a `DownloadState` describing chunks.
//!
//! # Example
//!
//! ```no_run
//! # async {
//! let client = reqwest::Client::new();
//! let (state, state_filename, size) = parallel_downloader::downloader::prepare_download(
//!     "https://example.com/file.bin",
//!     "output.bin".to_string(),
//!     4,
//!     &client,
//! ).await.unwrap();
//! # };
//! ```
use anyhow::Result;
use futures_util::future::join_all;
use reqwest::Client;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::observer::ProgressObserver;
use crate::state::{self, DownloadState};
use crate::utils;
use crate::worker::ArcRateLimiter;

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

/// Performs a parallel download using the provided observer factory.
///
/// This function handles the common logic of preparing the download,
/// spawning chunk download tasks, and cleaning up the state file.
/// The observer_factory is called for each chunk with (chunk_index, chunk_size).
pub async fn perform_parallel_download<F>(
    url: &str,
    output_filename: String,
    threads: u8,
    client: &Client,
    observer_factory: F,
    rate_limiter: Option<ArcRateLimiter>,
    cancel_token: CancellationToken,
) -> Result<u64>
where
    F: Fn(usize, u64) -> Arc<dyn ProgressObserver>,
{
    let (state, state_filename, size) =
        prepare_download(url, output_filename.clone(), threads, client).await?;
    let shared_state = Arc::new(Mutex::new(state));
    let mut tasks = Vec::new();

    let chunks = shared_state.lock().await.chunks.clone();
    for chunk in chunks.into_iter() {
        if chunk.completed {
            continue;
        }

        let chunk_size = chunk.end - chunk.start + 1;
        let observer = observer_factory(chunk.index, chunk_size);
        let task = tokio::spawn(crate::download_chunk(
            chunk,
            output_filename.clone(),
            observer,
            shared_state.clone(),
            state_filename.clone(),
            rate_limiter.clone(),
            client.clone(),
            cancel_token.clone(),
        ));
        tasks.push(task);
    }

    if !tasks.is_empty() {
        let results = join_all(tasks).await;
        for result in results {
            result??;
        }
    }

    fs::remove_file(&state_filename).await?;
    Ok(size)
}
