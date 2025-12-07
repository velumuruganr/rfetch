use futures_util::future::join_all;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parallel_downloader::observer::ConsoleObserver;
use parallel_downloader::state::DownloadState;
use parallel_downloader::utils;
use parallel_downloader::worker::download_chunk;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration
    let url = "https://proof.ovh.net/files/10Mb.dat";
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
    let state_file = format!("{}.state.json", output_file);

    // 3. Get File Size (The Recon Phase)
    let size = utils::get_file_size(url, &client).await?;
    println!("File Size: {} bytes", size);

    // 4. Pre-allocate the File on Disk
    let file = tokio::fs::File::create(&output_file).await?;
    file.set_len(size).await?;

    // 5. Calculate Chunks (The Logic)
    let chunks = utils::calculate_chunks(size, threads);

    // 6. Setup Shared State
    let state = DownloadState {
        url: url.to_string(),
        chunks: chunks.clone(),
    };
    let shared_state = Arc::new(Mutex::new(state));

    // 7. Setup UI (Progress Bars)
    let multi_progress = MultiProgress::new();
    let style = ProgressStyle::with_template(
        "{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
    )
    .unwrap()
    .progress_chars("=>-");

    // 8. Spawn Download Tasks
    let mut tasks = Vec::new();

    for (i, chunk) in chunks.into_iter().enumerate() {
        // Clone variables for the async task
        let task_client = client.clone();
        let task_output = output_file.clone();
        let task_state = shared_state.clone();
        let task_state_file = state_file.clone();

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