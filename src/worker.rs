//! Worker utilities that perform chunk downloads.
//!
//! This module contains the low-level code that fetches byte ranges and
//! writes them into the pre-allocated output file. Workers perform retries,
//! optionally throttle using a shared rate limiter, and update a shared
//! `DownloadState` so progress can be resumed.
use crate::observer::ProgressObserver;
use crate::state::{Chunk, DownloadState, save_state};
use anyhow::{Context, Result};
use governor::state::InMemoryState;
use governor::{RateLimiter, clock::DefaultClock, state::direct::NotKeyed};
use reqwest::header::RANGE;
use std::io::SeekFrom;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

/// A shared (Arc-wrapped) rate limiter type used by workers.
pub type ArcRateLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

/// Downloads a single chunk with buffered writing and robust error handling.
///
/// The function performs retries, optional rate limiting and updates the
/// shared `DownloadState` on success so progress can be resumed later.
pub async fn download_chunk(
    mut chunk: Chunk,
    output_file: String,
    observer: Arc<dyn ProgressObserver>,
    state: Arc<Mutex<DownloadState>>,
    state_filename: String,
    limiter: Option<ArcRateLimiter>,
    client: reqwest::Client,
    cancel_token: CancellationToken,
) -> Result<()> {
    const MAX_RETRIES: u32 = 5;
    let mut attempt = 0;

    if chunk.completed {
        return Ok(());
    }

    loop {
        attempt += 1;

        if cancel_token.is_cancelled() {
            return Ok(());
        }

        let result = async {
            let current_start = chunk.start + chunk.current_offset;

            if current_start > chunk.end {
                return Ok(());
            }

            let range_header = format!("bytes={}-{}", current_start, chunk.end);

            let mut response = client
                .get(&state.lock().await.url)
                .header(RANGE, range_header)
                .send()
                .await?;

            if attempt > 1 {
                observer.message(format!("Retry #{}...", attempt));
            }

            let file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&output_file)
                .await
                .context("Failed to open file")?;

            let mut writer = BufWriter::new(file);

            writer.get_mut().seek(SeekFrom::Start(chunk.start)).await?;

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        writer.flush().await?;

                        let mut locked_state = state.lock().await;
                        if let Some(c) = locked_state.chunks.get_mut(chunk.index) {
                            c.current_offset = chunk.current_offset;
                        }
                        save_state(&locked_state, &state_filename).await?;

                        observer.message("Paused".into());
                        return Ok(());
                    }

                    item = response.chunk() => {
                        match item {
                            Ok(Some(chunk_bytes)) => {
                                let len = chunk_bytes.len() as u32;

                                if let Some(ref l) = limiter
                                    && let Some(n) = std::num::NonZeroU32::new(len)
                                {
                                    l.until_n_ready(n).await.unwrap();
                                }

                                observer.inc(len as u64);
                                writer.write_all(&chunk_bytes).await?;

                                chunk.current_offset += len as u64;
                            }
                            Ok(None) => break,
                            Err(e) => return Err(anyhow::anyhow!(e)),
                        }
                    }
                }
            }

            // Ensure all bytes are flushed to disk before marking complete
            writer.flush().await?;

            Ok(())
        }
        .await;

        match result {
            Ok(_) => {
                if !cancel_token.is_cancelled() {
                    let mut locked_state = state.lock().await;
                    // Find chunk by index to be safe, though index usually matches position
                    if let Some(c) = locked_state.chunks.get_mut(chunk.index) {
                        c.completed = true;
                        c.current_offset = chunk.end - chunk.start + 1;
                    }
                    save_state(&locked_state, &state_filename).await?;
                    observer.finish();
                }
                return Ok(());
            }
            Err(e) => {
                if attempt >= MAX_RETRIES {
                    observer.message(format!("Failed: {}", e));
                    return Err(e);
                }
                observer.message(format!("Error: {}. Retrying in 2s...", e));
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
