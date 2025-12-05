use anyhow::{Result, anyhow, Context};
use clap::Parser;
use futures_util::future::join_all;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::{io::{AsyncSeekExt, AsyncWriteExt}, time::sleep};
use std::io::SeekFrom;
use std::time::Duration;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use reqwest::header::{CONTENT_LENGTH, RANGE};
use sha2::{Digest, Sha256};
use std::io::Read;


#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Chunk {
    start: u64,
    end: u64,
    completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadState {
    url: String,
    chunks: Vec<Chunk>,
}

/// A fast, concurrent file downloader built in Rust.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The URL of the file to download
    #[arg(short, long)]
    url: String,

    /// The output file name
    #[arg(short, long)]
    output: Option<String>,

    /// Number of threads to use
    #[arg(short = 't', long, default_value_t = 4)]
    threads: u8,

    #[arg(long)]
    verify_sha256: Option<String>,
}

fn verify_file_integrity(path: &str, expected_hash: &str) -> Result<()> {
    println!("Verifying file integrity...");

    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();

    let pb = ProgressBar::new(file_size);
    pb.set_style(ProgressStyle::with_template(
        "{msg} [{bar:40.yellow/blue}] {bytes}/{total_bytes} ({eta})"
    ).unwrap().progress_chars("#>-"));
    pb.set_message("Hashing");

    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }

        hasher.update(&buffer[..count]);
        pb.inc(count as u64);
    }

    pb.finish_with_message("Hashing complete");

    let result = hasher.finalize();
    let actual_hash = hex::encode(result);

    if actual_hash == expected_hash.to_lowercase() {
        println!("✅ Integrity Check PASSED!");
        Ok(())
    } else {
        println!("❌ Integrity Check FAILED!");
        println!("Expected: {}", expected_hash);
        println!("Actual:   {}", actual_hash);
        Err(anyhow!("File corruption detected: Hash mismatch"))
    }

}

fn calculate_chunks(total_size: u64, num_threads: u64) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let chunk_size = total_size / num_threads;

    for i in 0..num_threads {
        let start = i * chunk_size;

        let end = if i == num_threads - 1 {
            total_size - 1
        } else {
            (start + chunk_size) - 1
        };

        chunks.push(Chunk { start, end, completed: false })
    }

    chunks
}

async fn save_state(state: &DownloadState, filename: &str) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    tokio::fs::write(filename, json).await?;
    Ok(())
}

async fn get_file_size(url: &str) -> Result<u64> {
    let client = reqwest::Client::new();

    let response = client.head(url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Request failed. Status Code: {}",
            response.status()
        ));
    }

    let headers = response.headers();
    let content_length = headers
        .get(CONTENT_LENGTH)
        .ok_or(anyhow!("Content Length not found in response header."))?
        .to_str()?
        .parse::<u64>()?;

    Ok(content_length)
}

async fn download_chunk(chunk_index: usize, chunk: Chunk, output_file: String, pb: ProgressBar, state: Arc<Mutex<DownloadState>>, state_filename: String) -> Result<()> {
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
            
            let mut response = client.get(&state.lock().await.url).header(RANGE, range_header).send().await?;

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
                pb.inc(response_bytes.len() as u64);
                file.write_all(&response_bytes).await?;
            }

            Ok::<(), anyhow::Error>(())
        }.await;

        match result {
            Ok(_) => {
                let mut locked_state = state.lock().await;
                locked_state.chunks[chunk_index].completed = true;

                save_state(&locked_state, &state_filename).await?;

                pb.finish_with_message("Done!");
                return Ok(());
            },
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
        },
        Err(_) => {
            println!("Starting download for: {}", args.url);

            let file_size: u64 = get_file_size(&args.url).await?;
            println!("File Size: {}", file_size);

            let file = tokio::fs::File::create(&output_filename).await?;
            file.set_len(file_size).await?;

            let chunks = calculate_chunks(file_size, args.threads as u64);

            let state = DownloadState {
                url: args.url.clone(),
                chunks,
            };

            save_state(&state, &state_filename).await?;
            state
        } 
    };

    let shared_state = Arc::new(Mutex::new(download_state));

    let multi_progress = MultiProgress::new();

    let style = ProgressStyle::with_template(
        "{msg} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
    ).unwrap().progress_chars("=>-");

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

        let pb = multi_progress.add(ProgressBar::new(chunk.end - chunk.start + 1));
        pb.set_style(style.clone());
        pb.set_message(format!("Thread: {}", i+1));
        let task = tokio::spawn(async move {
            download_chunk(i, chunk, filename, pb, state_ref, state_file_ref).await
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
            verify_file_integrity(&output_filename, &expected_hash)
        }).await??;
    }

    Ok(())
}
