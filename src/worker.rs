use crate::state::{Chunk, DownloadState, save_state};
use anyhow::{Context, Result};
use governor::state::InMemoryState;
use governor::{RateLimiter, clock::DefaultClock, state::direct::NotKeyed};
use indicatif::ProgressBar;
use reqwest::header::RANGE;
use std::io::SeekFrom;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::{
    io::{AsyncSeekExt, AsyncWriteExt},
    time::sleep,
};

pub type ArcRateLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

/// Downloads a single chunk of a file, writing it to the specified position in the output file.
///
/// This function handles:
/// 1. **Seeking** to the correct position in the file.
/// 2. **Downloading** the bytes using HTTP Range headers.
/// 3. **Updating** the progress bar.
/// 4. **Rate Limiting** if a limiter is provided.
/// 5. **Retrying** on network failures (up to 5 times).
/// 6. **Saving State** upon successful completion.
///
/// # Arguments
///
/// * `chunk_index` - The index of this chunk in the state vector (used for updating status).
/// * `chunk` - The definition of the byte range to download.
/// * `output_file` - The path to the file being written to.
/// * `pb` - The progress bar associated with this thread.
/// * `state` - The shared state mutex for marking completion.
/// * `state_filename` - The path to save the state JSON file.
/// * `limiter` - An optional rate limiter to throttle download speed.
pub async fn download_chunk(
    chunk_index: usize,
    chunk: Chunk,
    output_file: String,
    pb: ProgressBar,
    state: Arc<Mutex<DownloadState>>,
    state_filename: String,
    limiter: Option<ArcRateLimiter>,
) -> Result<()> {
    const MAX_RETRIES: u32 = 5;
    let mut attempt = 0;

    if chunk.completed {
        pb.finish_with_message("Already downloaded...");
        return Ok(());
    }

    loop {
        attempt += 1;

        let result = async {
            let client = reqwest::Client::new();
            let range_header = format!("bytes={}-{}", chunk.start, chunk.end);

            let mut response = client
                .get(&state.lock().await.url)
                .header(RANGE, range_header)
                .send()
                .await?;

            if attempt > 1 {
                println!("Retry #{}...", attempt);
            }

            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&output_file)
                .await
                .context("Failed to open file")?;

            file.seek(SeekFrom::Start(chunk.start)).await?;

            while let Some(response_bytes) = response.chunk().await? {
                let len = response_bytes.len() as u32;

                if let Some(ref l) = limiter {
                    if let Some(n) = std::num::NonZeroU32::new(len) {
                        l.until_n_ready(n).await.unwrap();
                    }
                }

                pb.inc(response_bytes.len() as u64);
                file.write_all(&response_bytes).await?;
            }

            Ok::<(), anyhow::Error>(())
        }
        .await;

        match result {
            Ok(_) => {
                let mut locked_state = state.lock().await;
                locked_state.chunks[chunk_index].completed = true;

                save_state(&locked_state, &state_filename).await?;

                pb.finish_with_message("Done!");
                return Ok(());
            }
            Err(e) => {
                if attempt >= MAX_RETRIES {
                    pb.finish_with_message(format!("Failed..{}", e));
                    return Err(e);
                }

                pb.set_message(format!("Error: {}, Retrying in 2s...", e));
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
