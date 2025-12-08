//! Command-line binary entrypoint for `pd`.
//!
//! This module contains the small CLI glue that parses arguments and
//! either runs a standalone download or sends commands to the daemon.
mod args;
mod client;
mod tui;

use anyhow::Result;
use args::{Args, Commands};
use clap::Parser;
use client::send_command_raw;
use futures_util::future::join_all;
use governor::{Quota, RateLimiter};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parallel_downloader::config::Settings;
use parallel_downloader::downloader;
use parallel_downloader::ipc::{Command, Response};
use parallel_downloader::observer::ConsoleObserver;
use parallel_downloader::utils;
use parallel_downloader::{ArcRateLimiter, download_chunk};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Connect to the local daemon and send a `Command`.
async fn send_command(cmd: Command) -> Result<()> {
    let response = send_command_raw(cmd).await?;
    match response {
        Response::Ok(msg) => println!("✅ {}", msg),
        Response::Err(msg) => println!("❌ Error: {}", msg),
        Response::StatusList(jobs) => {
            println!(
                "{:<5} {:<30} {:<10} {:<15}",
                "ID", "Filename", "Progress", "State"
            );
            println!("{}", "-".repeat(60));
            for job in jobs {
                println!(
                    "{:<5} {:<30} {}%       {:<15}",
                    job.id, job.filename, job.progress_percent, job.state
                );
            }
        }
    }

    Ok(())
}

/// Download a single URL using the downloader library and progress bars.
///
/// This helper prepares the download workspace, spawns chunk tasks and
/// optionally runs a SHA-256 integrity check when `verify_hash` is provided.
async fn process_url(
    url: String,
    output_dir: String,
    output_filename: Option<String>,
    threads: u8,
    rate_limiter: Option<ArcRateLimiter>,
    verify_hash: Option<String>,
    multi_progress: MultiProgress,
    cancel_token: CancellationToken,
) -> Result<()> {
    let filename = output_filename.unwrap_or(utils::get_filename_from_url(&url));
    let mut output_path = PathBuf::from(&output_dir);
    output_path.push(&filename);

    if output_dir != "." {
        tokio::fs::create_dir_all(&output_dir).await?;
    }

    let client = reqwest::Client::builder()
        .user_agent("ParallelDownloader/0.2")
        .timeout(Duration::from_secs(30))
        .build()?;

    let output_filename = output_path.to_string_lossy().to_string();
    let (state, state_filename, _) =
        downloader::prepare_download(&url, output_filename.clone(), threads, &client).await?;
    let shared_state = Arc::new(Mutex::new(state));
    let mut tasks = Vec::new();

    let chunks_to_process = shared_state.lock().await.chunks.clone();
    for (i, chunk) in chunks_to_process.into_iter().enumerate() {
        if chunk.completed {
            continue;
        }

        let filename = output_filename.clone();
        let state_ref = shared_state.clone();
        let state_file_ref = state_filename.clone();
        let limiter_ref = rate_limiter.clone();
        let client_ref = client.clone();
        let token_ref = cancel_token.clone();

        let pb = multi_progress.add(ProgressBar::new(chunk.end - chunk.start + 1));
        pb.set_style(
            ProgressStyle::with_template("{msg} {bar:40.cyan/blue} {bytes}/{total_bytes}")
                .unwrap()
                .progress_chars("=>-"),
        );
        pb.set_message(format!("{} [Part {}]", filename, i + 1));
        let observer = Arc::new(ConsoleObserver { pb });
        let task = tokio::spawn(async move {
            download_chunk(
                chunk,
                filename,
                observer,
                state_ref,
                state_file_ref,
                limiter_ref,
                client_ref,
                token_ref,
            )
            .await
        });

        tasks.push(task);
    }

    if !tasks.is_empty() {
        let results = join_all(tasks).await;
        for result in results {
            result??;
        }
    }

    tokio::fs::remove_file(&state_filename).await?;

    if let Some(expected_hash) = verify_hash {
        let output_filename = output_filename.clone();

        tokio::task::spawn_blocking(move || {
            utils::verify_file_integrity(&output_filename, &expected_hash)
        })
        .await??;
    }

    let _ = multi_progress.println(format!("✅ Finished {}", filename));
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let settings = Settings::load().unwrap_or_default();

    let secret = settings.server_secret.clone();
    let bind_ip = settings
        .server_addr
        .unwrap_or_else(|| "127.0.0.1".to_string());

    match args.command {
        Some(Commands::Start) => {
            parallel_downloader::daemon::start_daemon(9090, secret, bind_ip).await?;
        }
        Some(Commands::Add { url, dir }) => {
            send_command(Command::Add { url, dir }).await?;
        }
        Some(Commands::Status) => {
            send_command(Command::Status).await?;
        }
        Some(Commands::Tui) => {
            tui::start_tui().await?;
        }
        Some(Commands::Stop) => {
            send_command(Command::Shutdown).await?;
        }
        Some(Commands::Pause { id }) => {
            send_command(Command::Pause { id }).await?;
        }
        Some(Commands::Resume { id }) => {
            send_command(Command::Resume { id }).await?;
        }
        Some(Commands::Run {
            url,
            output,
            threads,
            verify_sha256,
            rate_limit,
            dir,
            input,
            concurrent_files,
        }) => {
            let threads = threads.or(settings.threads).unwrap_or(4);
            let rate_limit_val = rate_limit.or(settings.rate_limit);
            let default_dir = dir
                .or(settings.default_dir)
                .unwrap_or_else(|| ".".to_string());
            let concurrent_files = concurrent_files.or(settings.concurrent_files).unwrap_or(3);

            let rate_limiter: Option<ArcRateLimiter> = if let Some(bytes_per_sec) = rate_limit_val {
                if bytes_per_sec > 0 {
                    let quota =
                        Quota::per_second(std::num::NonZeroU32::new(bytes_per_sec).unwrap());
                    Some(Arc::new(RateLimiter::direct(quota)))
                } else {
                    None
                }
            } else {
                None
            };
            let cancel_token = CancellationToken::new();
            let signal_token = cancel_token.clone();

            tokio::spawn(async move {
                if let Ok(_) = tokio::signal::ctrl_c().await {
                    println!("\n🛑 Received Ctrl+C. Pausing downloads gracefully...");
                    signal_token.cancel();
                }
            });

            let multi_progress = MultiProgress::new();

            if let Some(input_file) = input {
                let mut warnings = Vec::new();

                if output.is_some() {
                    warnings.push("⚠️  Warning: The --output / -o flag is ignored in batch mode.\n   Files will be named automatically based on their URLs.");
                }

                if verify_sha256.is_some() {
                    warnings.push("⚠️  Warning: The --verify-sha256 flag is ignored in batch mode.\n   Cannot verify multiple different files against a single hash.");
                }

                if !warnings.is_empty() {
                    for w in warnings {
                        eprintln!("{}", w);
                    }
                    eprintln!("   (Downloads will proceed without these settings)\n");
                }

                println!("🚀 Starting Batch Download from {}", input_file);

                let content = tokio::fs::read_to_string(&input_file).await?;
                let urls: Vec<String> = content
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();

                // SEMAPHORE: Limit global concurrent files
                // If args.concurrent_files is 3, only 3 files download at once.
                let semaphore = Arc::new(Semaphore::new(concurrent_files));
                let mut file_tasks = Vec::new();

                for url in urls {
                    if cancel_token.is_cancelled() {
                        println!("Batch download stopped by user.");
                        break;
                    }
                    let task_token = cancel_token.clone();
                    let sem_clone = semaphore.clone();
                    let mp_clone = multi_progress.clone();

                    let dir = default_dir.clone();
                    let limiter = rate_limiter.clone();

                    let task = tokio::spawn(async move {
                        let _permit = sem_clone.acquire().await.unwrap();

                        if let Err(e) = process_url(
                            url.clone(),
                            dir,
                            None,
                            threads,
                            limiter,
                            None,
                            mp_clone,
                            task_token,
                        )
                        .await
                        {
                            eprintln!("❌ Failed to download {}: {}", url, e);
                        }
                    });

                    file_tasks.push(task);
                }

                join_all(file_tasks).await;
            } else {
                let url = url.clone();

                process_url(
                    url,
                    default_dir,
                    output,
                    threads,
                    rate_limiter,
                    verify_sha256,
                    multi_progress,
                    cancel_token.clone(),
                )
                .await?;

                println!("Download Completed!");
            }
        }
        None => {
            println!("Please use a subcommand: start, add, status, or run.");
        }
    }

    Ok(())
}
