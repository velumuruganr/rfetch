use crate::ipc::{Command, JobStatus, Response};
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

pub struct ActiveJobData {
    pub id: usize,
    pub filename: String,
    pub total_bytes: AtomicU64,
    pub downloaded_bytes: AtomicU64,
    pub state: Mutex<String>,
}

struct DaemonState {
    jobs: HashMap<usize, Arc<ActiveJobData>>,
}

pub async fn start_daemon(port: u16) -> Result<()> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    println!("Daemon started on port {}", port);

    let client = reqwest::Client::builder()
        .user_agent("ParallelDownloader/0.2")
        .timeout(Duration::from_secs(30))
        .build()?;

    let global_state = Arc::new(Mutex::new(DaemonState {
        jobs: HashMap::new(),
    }));
    let next_id = Arc::new(AtomicUsize::new(1));

    loop {
        let (mut socket, _) = listener.accept().await?;

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
            let command: Command = match serde_json::from_str(&req_str) {
                Ok(c) => c,
                Err(e) => {
                    let _ =
                        send_response(&mut socket, Response::Err(format!("Invalid JSON: {}", e)))
                            .await;
                    return;
                }
            };

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
                Command::Add { url, dir } => {
                    let id = next_id_ref.fetch_add(1, Ordering::SeqCst);
                    let filename = crate::utils::get_filename_from_url(&url);

                    let job_data = Arc::new(ActiveJobData {
                        id,
                        filename: filename.clone(),
                        total_bytes: AtomicU64::new(0), // We'll set this after we fetch file size
                        downloaded_bytes: AtomicU64::new(0),
                        state: Mutex::new("Starting...".into()),
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

    for chunk in chunks_to_process.into_iter() {
        if chunk.completed {
            continue;
        }

        let filename = output_filename.clone();
        let state_ref = shared_state.clone();
        let state_file_ref = state_filename.clone();
        let client_ref = client.clone();

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

async fn send_response(socket: &mut tokio::net::TcpStream, resp: Response) -> Result<()> {
    let json = serde_json::to_string(&resp)?;
    socket.write_all(json.as_bytes()).await?;
    Ok(())
}
