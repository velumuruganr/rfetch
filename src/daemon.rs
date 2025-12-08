//! Daemon mode implementation.
//!
//! The daemon provides a small TCP-based control plane for managing
//! background downloads. It accepts JSON `Command`s and returns
//! `Response`s defined in `ipc.rs`.
use crate::ipc::{Command, JobStatus, Request, Response};
use crate::observer::DaemonObserver;
use crate::{download_chunk, downloader};
use anyhow::Result;
use futures_util::future::join_all;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
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
    pub cancel_token: Mutex<CancellationToken>,
    pub url: String,
    pub dir: String,
}

struct DaemonState {
    jobs: HashMap<usize, Arc<ActiveJobData>>,
}

/// Start the daemon listening on `127.0.0.1:port`.
///
/// This function runs an event loop accepting simple JSON commands
/// to add jobs, query status, or shutdown the daemon.
pub async fn start_daemon(port: u16, secret: Option<String>, bind_ip: String) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", bind_ip, port)).await?;
    println!("Daemon started on {}:{}", bind_ip, port);

    let client = reqwest::Client::builder()
        .user_agent("ParallelDownloader/0.2")
        .timeout(Duration::from_secs(30))
        .build()?;

    let global_state = Arc::new(Mutex::new(DaemonState {
        jobs: HashMap::new(),
    }));
    let next_id = Arc::new(AtomicUsize::new(1));

    loop {
        let (mut socket, addr) = listener.accept().await?;
        let secret_check = secret.clone();

        let next_id_ref = next_id.clone();
        let state_ref = global_state.clone();
        let client_ref = client.clone();

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

            let command = request.command;

            match command {
                Command::Shutdown => {
                    let _ =
                        send_response(&mut socket, Response::Ok("Shutting down...".into())).await;
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

                        let new_token = CancellationToken::new();

                        *cancel_token_ref = new_token
                    } else {
                        let _ =
                            send_response(&mut socket, Response::Err("Job ID not found".into()))
                                .await;
                    }
                }
                Command::Add { url, dir } => {
                    let id = next_id_ref.fetch_add(1, Ordering::SeqCst);
                    let filename = crate::utils::get_filename_from_url(&url);

                    let job_data = Arc::new(ActiveJobData {
                        id,
                        filename: filename.clone(),
                        total_bytes: AtomicU64::new(0), // We'll set this after we fetch file size
                        downloaded_bytes: AtomicU64::new(0),
                        state: Mutex::new("Starting...".into()),
                        cancel_token: Mutex::new(CancellationToken::new()),
                        url: url.clone(),
                        dir: dir.clone(),
                    });
                    {
                        let mut locked = state_ref.lock().await;
                        locked.jobs.insert(id, job_data.clone());
                    }

                    let _ = send_response(
                        &mut socket,
                        Response::Ok(format!("Added job #{}: {}", id, filename)),
                    )
                    .await;

                    tokio::spawn(async move {
                        if let Err(e) =
                            perform_background_download(url, dir, job_data.clone(), client_ref)
                                .await
                        {
                            let mut state = job_data.state.lock().await;
                            *state = format!("Failed: {}", e);
                        }
                    });
                }
            }
        });
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

    // 2. Recon (Get Size)
    {
        let mut s = job_data.state.lock().await;
        *s = "Fetching Metadata...".into();
    }

    let (state, state_filename, size) =
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

    let shared_state = Arc::new(Mutex::new(state));
    let mut tasks = Vec::new();
    let chunks_to_process = shared_state.lock().await.chunks.clone();

    let cancel_token = job_data.cancel_token.lock().await;

    for chunk in chunks_to_process.into_iter() {
        if chunk.completed {
            continue;
        }

        let filename = output_filename.clone();
        let state_ref = shared_state.clone();
        let state_file_ref = state_filename.clone();
        let client_ref = client.clone();
        let cancel_token_ref = cancel_token.clone();

        let observer = Arc::new(DaemonObserver {
            job_data: job_data.clone(),
        });
        let task = tokio::spawn(async move {
            download_chunk(
                chunk,
                filename,
                observer,
                state_ref,
                state_file_ref,
                None,
                client_ref,
                cancel_token_ref,
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

    let _ = tokio::fs::remove_file(&state_filename).await;
    {
        let mut s = job_data.state.lock().await;
        *s = "Done".into();
    }
    // Ensure 100% on finish
    job_data.downloaded_bytes.store(size, Ordering::SeqCst);

    Ok(())
}

/// Serialize and send a `Response` back to the connected socket.
async fn send_response(socket: &mut tokio::net::TcpStream, resp: Response) -> Result<()> {
    let json = serde_json::to_string(&resp)?;
    socket.write_all(json.as_bytes()).await?;
    Ok(())
}
