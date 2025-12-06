use crate::state::Chunk;
use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::header::CONTENT_LENGTH;
use sha2::{Digest, Sha256};
use std::io::Read;

/// fetches the Content-Length of a file from a URL using a HEAD request.
///
/// # Errors
/// 
/// Returns an error if:
/// * The network request fails.
/// * The server returns a non-success status code.
/// * The server does not provide a `Content-Length` header.
pub async fn get_file_size(url: &str) -> Result<u64> {
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

/// Divides a total file size into equal-sized chunks for concurrent downloading.
/// 
/// The last chunk will automatically expand to cover any remainder bytes.
pub fn calculate_chunks(total_size: u64, num_threads: u64) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let chunk_size = total_size / num_threads;

    for i in 0..num_threads {
        let start = i * chunk_size;

        let end = if i == num_threads - 1 {
            total_size - 1
        } else {
            (start + chunk_size) - 1
        };

        chunks.push(Chunk {
            start,
            end,
            completed: false,
        })
    }

    chunks
}

/// Calculates the SHA-256 hash of a file and compares it to an expected hash.
/// 
/// # Arguments
/// 
/// * `path` - The path to the file on disk.
/// * `expected_hash` - The hex-encoded SHA-256 string to compare against.
/// 
/// # Returns
/// 
/// Returns `Ok(())` if the hashes match. Returns an `Err` if they do not match
/// or if the file cannot be read.
pub fn verify_file_integrity(path: &str, expected_hash: &str) -> Result<()> {
    println!("Verifying file integrity...");

    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();

    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::with_template("{msg} [{bar:40.yellow/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
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
