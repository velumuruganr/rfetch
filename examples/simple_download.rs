use futures_util::future::join_all;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parallel_downloader::downloader;
use parallel_downloader::observer::ConsoleObserver;
use parallel_downloader::utils;
use parallel_downloader::worker::download_chunk;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration
    let url = "https://proof.ovh.net/files/1Mb.dat";
    let threads = 4;
    let output_dir = ".";

    println!("Starting example download...");
    println!("URL: {}", url);

    // 1. Setup a robust HTTP Client
    let client = reqwest::Client::builder()
        .user_agent("ParallelDownloader-Example/0.1")
        .timeout(Duration::from_secs(30))
        .build()?;

    // 2. Prepare Paths and Filename
    let filename = utils::get_filename_from_url(url);
    let mut output_path = PathBuf::from(output_dir);
    output_path.push(&filename);
    let output_file = output_path.to_string_lossy().to_string();

    // 3. Prepare download (fetch metadata, pre-allocate file, create state)
    let (state, state_file, _size) =
        downloader::prepare_download(url, output_file.clone(), threads, &client).await?;

    // 4. Setup Shared State
    let shared_state = Arc::new(Mutex::new(state));

    // Extract chunk list from the shared state for task spawning
    let chunks = shared_state.lock().await.chunks.clone();

    // 7. Setup UI (Progress Bars)
    let multi_progress = MultiProgress::new();
    let style =
        ProgressStyle::with_template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("=>-");

    // 8. Spawn Download Tasks
    let mut tasks = Vec::new();

    let cancel_token = CancellationToken::new();

    for (i, chunk) in chunks.into_iter().enumerate() {
        // Clone variables for the async task
        let task_client = client.clone();
        let task_output = output_file.clone();
        let task_state = shared_state.clone();
        let task_state_file = state_file.clone();
        let token_ref = cancel_token.clone();

        // Setup individual progress bar
        let pb = multi_progress.add(ProgressBar::new(chunk.end - chunk.start + 1));
        pb.set_style(style.clone());
        pb.set_message(format!("Part {}", i + 1));

        // Wrap the progress bar in the library's Observer trait
        let observer = Arc::new(ConsoleObserver { pb });

        // Spawn the worker
        tasks.push(tokio::spawn(async move {
            download_chunk(
                chunk,
                task_output,
                observer,
                task_state,
                task_state_file,
                None, // No rate limiter for this example
                task_client,
                token_ref,
            )
            .await
        }));
    }

    // 9. Wait for all threads to complete
    let results = join_all(tasks).await;
    for res in results {
        // Handle task panic (res?) and download error (res??)
        if let Err(e) = res? {
            eprintln!("A chunk failed to download: {}", e);
            return Ok(());
        }
    }

    // 10. Cleanup state file
    tokio::fs::remove_file(&state_file).await?;

    println!("✅ Download completed successfully: {}", output_file);
    Ok(())
}
