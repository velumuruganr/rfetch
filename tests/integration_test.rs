use indicatif::ProgressBar;
use parallel_downloader::observer::ConsoleObserver;
use parallel_downloader::state::{Chunk, DownloadState};
use parallel_downloader::worker::download_chunk;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;
use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_multipart_download_stitching() {
    // 1. Setup Mock Server
    let mock_server = MockServer::start().await;

    // We simulate a file "HelloWorld" split into two chunks: "Hello" (0-4) and "World" (5-9)
    Mock::given(method("GET"))
        .and(header("Range", "bytes=0-4"))
        .respond_with(ResponseTemplate::new(206).set_body_string("Hello"))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(header("Range", "bytes=5-9"))
        .respond_with(ResponseTemplate::new(206).set_body_string("World"))
        .mount(&mock_server)
        .await;

    // 2. Setup Files
    let temp_file = NamedTempFile::new().unwrap();
    let output_path = temp_file.path().to_str().unwrap().to_string();
    let state_path = format!("{}.state", output_path);

    // 3. Define Chunks
    let chunk1 = Chunk {
        index: 0,
        start: 0,
        end: 4,
        completed: false,
    };
    let chunk2 = Chunk {
        index: 1,
        start: 5,
        end: 9,
        completed: false,
    };

    let state = Arc::new(Mutex::new(DownloadState {
        url: mock_server.uri(),
        chunks: vec![chunk1.clone(), chunk2.clone()],
    }));

    let client = reqwest::Client::new();
    let pb = ProgressBar::hidden();
    let observer = Arc::new(ConsoleObserver { pb });

    // 4. Download Chunk 1
    download_chunk(
        chunk1,
        output_path.clone(),
        observer.clone(),
        state.clone(),
        state_path.clone(),
        None,
        client.clone(),
    )
    .await
    .expect("Chunk 1 failed");

    // 5. Download Chunk 2
    download_chunk(
        chunk2,
        output_path.clone(),
        observer.clone(),
        state.clone(),
        state_path.clone(),
        None,
        client.clone(),
    )
    .await
    .expect("Chunk 2 failed");

    // 6. Verify Content
    let content = tokio::fs::read_to_string(&output_path).await.unwrap();
    assert_eq!(content, "HelloWorld", "Chunks were not stitched correctly!");

    // 7. Verify State
    let locked_state = state.lock().await;
    assert!(locked_state.chunks[0].completed);
    assert!(locked_state.chunks[1].completed);
}
