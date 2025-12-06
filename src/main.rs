mod args;
mod state;
mod utils;
mod worker;

use anyhow::{Result, anyhow};
use clap::Parser;
use futures_util::future::join_all;
use governor::{Quota, RateLimiter};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::Arc;
use tokio::sync::Mutex;

use args::Args;
use state::DownloadState;
use worker::download_chunk;

use crate::worker::ArcRateLimiter;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let output_filename = args.output.unwrap_or_else(|| "output.bin".to_string());
    let state_filename = format!("{}.state.json", output_filename);

    let state_result = tokio::fs::read_to_string(&state_filename).await;

    let download_state = match state_result {
        Ok(json_dats) => {
            println!("Resuming download...");
            let state: DownloadState = serde_json::from_str(&json_dats)?;

            if state.url != args.url {
                return Err(anyhow!("State file URL does not match argument URL!"));
            }
            state
        }
        Err(_) => {
            println!("Starting download for: {}", args.url);

            let file_size: u64 = utils::get_file_size(&args.url).await?;
            println!("File Size: {}", file_size);

            let file = tokio::fs::File::create(&output_filename).await?;
            file.set_len(file_size).await?;

            let chunks = utils::calculate_chunks(file_size, args.threads as u64);

            let state = DownloadState {
                url: args.url.clone(),
                chunks,
            };

            state::save_state(&state, &state_filename).await?;
            state
        }
    };

    let shared_state = Arc::new(Mutex::new(download_state));

    let rate_limiter: Option<ArcRateLimiter> = if let Some(bytes_per_sec) = args.rate_limit {
        if bytes_per_sec > 0 {
            println!("Rate Limiting enabled: {} bytes/sec", bytes_per_sec);

            let quota = Quota::per_second(std::num::NonZeroU32::new(bytes_per_sec).unwrap());
            Some(Arc::new(RateLimiter::direct(quota)))
        } else {
            None
        }
    } else {
        None
    };

    let multi_progress = MultiProgress::new();

    let style = ProgressStyle::with_template(
        "{msg} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
    )
    .unwrap()
    .progress_chars("=>-");

    let mut tasks = Vec::new();

    let chunks_to_process = shared_state.lock().await.chunks.clone();
    for (i, chunk) in chunks_to_process.into_iter().enumerate() {
        if chunk.completed {
            println!("Skipping chunk {} (already done)", i + 1);
            continue;
        }

        let filename = output_filename.clone();
        let state_ref = shared_state.clone();
        let state_file_ref = state_filename.clone();
        let limiter_ref = rate_limiter.clone();

        let pb = multi_progress.add(ProgressBar::new(chunk.end - chunk.start + 1));
        pb.set_style(style.clone());
        pb.set_message(format!("Thread: {}", i + 1));
        let task = tokio::spawn(async move {
            download_chunk(
                i,
                chunk,
                filename,
                pb,
                state_ref,
                state_file_ref,
                limiter_ref,
            )
            .await
        });

        tasks.push(task);
    }

    if tasks.is_empty() {
        println!("All chunks already complete!");
        return Ok(());
    }

    let results = join_all(tasks).await;

    for result in results {
        result??;
    }

    println!("Download completed.");
    tokio::fs::remove_file(&state_filename).await?;

    if let Some(expected_hash) = args.verify_sha256 {
        let output_filename = output_filename.clone();

        tokio::task::spawn_blocking(move || {
            utils::verify_file_integrity(&output_filename, &expected_hash)
        })
        .await??;
    }

    Ok(())
}
