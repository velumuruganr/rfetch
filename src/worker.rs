use crate::observer::ProgressObserver;
use crate::state::{Chunk, DownloadState, save_state};
use anyhow::{Context, Result};
use governor::state::InMemoryState;
use governor::{RateLimiter, clock::DefaultClock, state::direct::NotKeyed};
use reqwest::header::RANGE;
use std::io::SeekFrom;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter}; // Import BufWriter
use tokio::sync::Mutex;
use tokio::time::sleep;

pub type ArcRateLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

/// Downloads a single chunk with buffered writing and robust error handling.
pub async fn download_chunk(
    chunk: Chunk,
    output_file: String,
    observer: Arc<dyn ProgressObserver>,
    state: Arc<Mutex<DownloadState>>,
    state_filename: String,
    limiter: Option<ArcRateLimiter>,
    client: reqwest::Client,
) -> Result<()> {
    const MAX_RETRIES: u32 = 5;
    let mut attempt = 0;

    if chunk.completed {
        return Ok(());
    }

    loop {
        attempt += 1;

        let result = async {
            let range_header = format!("bytes={}-{}", chunk.start, chunk.end);

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

            while let Some(chunk_bytes) = response.chunk().await? {
                let len = chunk_bytes.len() as u32;

                if let Some(ref l) = limiter
                    && let Some(n) = std::num::NonZeroU32::new(len)
                {
                    l.until_n_ready(n).await.unwrap();
                }

                observer.inc(len as u64);
                writer.write_all(&chunk_bytes).await?;
            }

            // Ensure all bytes are flushed to disk before marking complete
            writer.flush().await?;

            Ok::<(), anyhow::Error>(())
        }
        .await;

        match result {
            Ok(_) => {
                let mut locked_state = state.lock().await;
                // Find chunk by index to be safe, though index usually matches position
                if let Some(c) = locked_state.chunks.get_mut(chunk.index) {
                    c.completed = true;
                }
                save_state(&locked_state, &state_filename).await?;
                observer.finish();
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
