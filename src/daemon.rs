//! Daemon mode implementation.
//!
//! The daemon provides a small TCP-based control plane for managing
//! background downloads. It accepts JSON `Command`s and returns
//! `Response`s defined in `ipc.rs`.
use crate::config::Settings;
use crate::ipc::{Command, JobStatus, Request, Response};
use crate::observer::DaemonObserver;
use crate::{downloader, perform_parallel_download};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

/// Runtime status for an active job inside the daemon.
///
/// This structure is intentionally light-weight and contains atomics
/// so it can be updated from multiple worker tasks without heavy locking.
pub struct ActiveJobData {
    /// Job id assigned by the daemon.
    pub id: usize,
    /// Output filename for the job.
    pub filename: String,
    /// Total size (in bytes) for the job, updated after metadata fetch.
    pub total_bytes: AtomicU64,
    /// Running counter of downloaded bytes.
    pub downloaded_bytes: AtomicU64,
    /// Short textual state (e.g. "Starting...", "Downloading...", "Done").
    pub state: Mutex<String>,
    /// Cancellation token used to pause/stop the job.
    pub cancel_token: Mutex<CancellationToken>,
    /// Source URL for this job.
    pub url: String,
    /// Directory where the output file is being written.
    pub dir: String,
}

struct DaemonState {
    /// Map of active job id -> job metadata used by the daemon.
    jobs: HashMap<usize, Arc<ActiveJobData>>,
}

struct JobRequest {
    /// URL to download.
    url: String,
    /// Target directory for the download.
    dir: String,
    /// Shared job metadata tracked by the daemon.
    job_data: Arc<ActiveJobData>,
}

/// Start the daemon listening on the requested `bind_ip:port`.
///
/// This function runs the main event loop that accepts JSON `Request`s
/// over a local TCP socket and responds with `Response`s. It supports
/// adding jobs, querying status, pausing/resuming, and graceful shutdown.
///
/// Parameters:
/// - `port`: TCP port to bind the control socket to.
/// - `secret`: optional string that must match incoming requests' secret.
/// - `bind_ip`: IP address to bind to (commonly `127.0.0.1`).
pub async fn start_daemon(port: u16, secret: Option<String>, bind_ip: String) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", bind_ip, port)).await?;
    println!("Daemon started on {}:{}", bind_ip, port);

    let settings = Settings::load().unwrap_or_default();
    let max_concurrent_downloads = settings.concurrent_files.unwrap_or(3);

    let client = reqwest::Client::builder()
        .user_agent("ParallelDownloader/0.2")
        .timeout(Duration::from_secs(30))
        .build()?;

    let global_state = Arc::new(Mutex::new(DaemonState {
        jobs: HashMap::new(),
    }));
    let next_id = Arc::new(AtomicUsize::new(1));

    let shutdown_token = CancellationToken::new();
    let shutdown_token_ref = shutdown_token.clone();

    let semaphore = Arc::new(Semaphore::new(max_concurrent_downloads));

    let (job_tx, mut job_rx) = mpsc::channel::<JobRequest>(100);

    let client_for_scheduler = client.clone();
    let shutdown_for_scheduler = shutdown_token.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_for_scheduler.cancelled() => {
                    break;
                }
                Some(req) = job_rx.recv() => {
                    let semaphore_clone = semaphore.clone();
                    let client_clone = client_for_scheduler.clone();

                    tokio::spawn(async move {
                        let _permit = match semaphore_clone.acquire().await {
                            Ok(p) => p,
                            Err(_) => return,
                        };

                        let job_token = req.job_data.cancel_token.lock().await;
                        if job_token.is_cancelled() { return; }

                        {
                            let mut state = req.job_data.state.lock().await;
                            *state = "Starting...".to_string();
                        }

                        if let Err(e) = perform_background_download(
                            req.url,
                            req.dir,
                            req.job_data.clone(),
                            client_clone,
                        ).await {
                            let mut state = req.job_data.state.lock().await;
                            *state = format!("Failed: {}", e);
                        }
                    });
                }
            }
        }
    });

    tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            println!("\nReceived Ctrl+C. Pausing all jobs and shutting down...");
            shutdown_token_ref.cancel();
        }
    });

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                println!("Daemon shutting down. Goodbye!");
                // Wait briefly for tasks to detect cancellation and save state
                tokio::time::sleep(Duration::from_millis(500)).await;
                return Ok(());
            }
            accept_result = listener.accept() => {
                let (mut socket, addr) = accept_result?;
                let secret_check = secret.clone();
                let next_id_ref = next_id.clone();
                let state_ref = global_state.clone();
                let shutdown_token_inner = shutdown_token.clone();
                let job_tx_clone = job_tx.clone();

                tokio::spawn(async move {
                    let mut buf = [0; 4096];

                    let n = match socket.read(&mut buf).await {
                        Ok(0) => return,
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("Socket read error: {}", e);
                            return;
                        }
                    };

                    let req_str = String::from_utf8_lossy(&buf[..n]);
                    let request: Request = match serde_json::from_str(&req_str) {
                        Ok(r) => r,
                        Err(e) => {
                            let _ =
                                send_response(&mut socket, Response::Err(format!("Invalid JSON: {}", e)))
                                    .await;
                            return;
                        }
                    };

                    if let Some(ref server_pass) = secret_check
                        && request.secret.as_ref() != Some(server_pass)
                    {
                        println!("⚠️ Unauthorized attempt from {}", addr);
                        let _ = send_response(
                            &mut socket,
                            Response::Err("Unauthorized: Invalid secret".into()),
                        )
                        .await;
                        return;
                    }

                    match request.command {
                        Command::Shutdown => {
                            let _ =
                                send_response(&mut socket, Response::Ok("Shutting down...".into())).await;

                            shutdown_token_inner.cancel();
                            std::process::exit(0);
                        }
                        Command::Status => {
                            let locked = state_ref.lock().await;
                            let mut list = Vec::new();

                            for job in locked.jobs.values() {
                                let current = job.downloaded_bytes.load(Ordering::Relaxed);
                                let total = job.total_bytes.load(Ordering::Relaxed);
                                let percent = if total > 0 {
                                    (current * 100) / total
                                } else {
                                    0
                                };
                                list.push(JobStatus {
                                    id: job.id,
                                    filename: job.filename.clone(),
                                    progress_percent: percent,
                                    state: job.state.lock().await.clone(),
                                });
                            }
                            list.sort_by_key(|j| j.id);
                            let _ = send_response(&mut socket, Response::StatusList(list)).await;
                        }
                        Command::Pause { id } => {
                            let locked = state_ref.lock().await;
                            if let Some(job) = locked.jobs.get(&id) {
                                let cancel_token_ref = job.cancel_token.lock().await;
                                cancel_token_ref.cancel();
                                let mut state_str = job.state.lock().await;
                                *state_str = "Pausing...".into();
                                let _ =
                                    send_response(&mut socket, Response::Ok(format!("Paused job #{}", id)))
                                        .await;
                            } else {
                                let _ =
                                    send_response(&mut socket, Response::Err("Job ID not found".into()))
                                        .await;
                            }
                        }
                        Command::Resume { id } => {
                            let locked = state_ref.lock().await;
                            if let Some(job) = locked.jobs.get(&id) {
                                let mut cancel_token_ref = job.cancel_token.lock().await;
                                if !cancel_token_ref.is_cancelled() {
                                    let _ = send_response(
                                        &mut socket,
                                        Response::Err("Job is already running".into()),
                                    )
                                    .await;
                                    return;
                                }

                                let new_token = shutdown_token_inner.child_token();
                                *cancel_token_ref = new_token;

                                let _ = job_tx_clone.send(JobRequest {
                                    url: job.url.clone(),
                                    dir: job.dir.clone(),
                                    job_data: job.clone(),
                                }).await;

                                let _ = send_response(&mut socket, Response::Ok(format!("Resumed job #{}", id))).await;
                            } else {
                                let _ =
                                    send_response(&mut socket, Response::Err("Job ID not found".into()))
                                        .await;
                            }
                        }
                        Command::Add { url, dir } => {
                            let id = next_id_ref.fetch_add(1, Ordering::SeqCst);
                            let filename = crate::utils::get_filename_from_url(&url);
                            let child_cancel_token = shutdown_token_inner.child_token();

                            let job_data = Arc::new(ActiveJobData {
                                id,
                                filename: filename.clone(),
                                total_bytes: AtomicU64::new(0), // We'll set this after we fetch file size
                                downloaded_bytes: AtomicU64::new(0),
                                state: Mutex::new("Queued".into()),
                                cancel_token: Mutex::new(child_cancel_token),
                                url: url.clone(),
                                dir: dir.clone(),
                            });
                            {
                                let mut locked = state_ref.lock().await;
                                locked.jobs.insert(id, job_data.clone());
                            }

                            // Send to Scheduler Channel
                            if let Err(e) = job_tx_clone.send(JobRequest {
                                url: url.clone(),
                                dir: dir.clone(),
                                job_data: job_data.clone(),
                            }).await {
                                let _ = send_response(&mut socket, Response::Err(format!("Scheduler Error: {}", e))).await;
                                return;
                            }

                            let _ = send_response(
                                &mut socket,
                                Response::Ok(format!("Queued job #{}: {}", id, filename)),
                            )
                            .await;
                        }
                    }
                });
            }
        }
    }
}

/// Internal helper: performs downloading for a job in background.
///
/// This is responsible for preparing the download, spawning chunk tasks,
/// and updating the `ActiveJobData` as progress occurs.
async fn perform_background_download(
    url: String,
    dir: String,
    job_data: Arc<ActiveJobData>,
    client: reqwest::Client,
) -> Result<()> {
    let filename = &job_data.filename;
    let mut output_path = std::path::PathBuf::from(&dir);
    output_path.push(filename);

    if dir != "." {
        tokio::fs::create_dir_all(&dir).await?;
    }

    let output_filename = output_path.to_string_lossy().to_string();

    {
        let mut s = job_data.state.lock().await;
        *s = "Fetching Metadata...".into();
    }

    let (state, _state_filename, size) =
        downloader::prepare_download(&url, output_filename.clone(), 4, &client).await?;
    job_data.total_bytes.store(size, Ordering::SeqCst);
    let downloaded_bytes: u64 = state
        .chunks
        .iter()
        .filter(|c| c.completed)
        .map(|c| c.end - c.start + 1)
        .sum();
    job_data
        .downloaded_bytes
        .store(downloaded_bytes, Ordering::SeqCst);

    {
        let mut s = job_data.state.lock().await;
        *s = "Downloading...".into();
    }

    let token_clone = {
        let guard = job_data.cancel_token.lock().await;
        guard.clone()
    };

    let _ = perform_parallel_download(
        &url,
        output_filename.clone(),
        4,
        &client,
        |_, _| {
            Arc::new(DaemonObserver {
                job_data: job_data.clone(),
            })
        },
        None,
        token_clone.clone(),
    )
    .await?;

    if !token_clone.is_cancelled() {
        let mut s = job_data.state.lock().await;
        *s = "Done".into();
        job_data.downloaded_bytes.store(size, Ordering::SeqCst);
    } else {
        let mut s = job_data.state.lock().await;
        *s = "Paused".into();
    }

    Ok(())
}

/// Serialize and send a `Response` back to the connected socket.
async fn send_response(socket: &mut tokio::net::TcpStream, resp: Response) -> Result<()> {
    let json = serde_json::to_string(&resp)?;
    socket.write_all(json.as_bytes()).await?;
    Ok(())
}
