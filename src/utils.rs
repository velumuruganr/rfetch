//! Utility helpers used across the crate.
//!
//! Small convenience functions for HTTP metadata, filename extraction,
//! chunk calculation, and file integrity verification.
use crate::state::Chunk;
use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use percent_encoding::percent_decode_str;
use reqwest::header::CONTENT_LENGTH;
use sanitize_filename::sanitize;
use sha2::{Digest, Sha256};
use std::io::Read;
use url::Url;

/// fetches the Content-Length of a file from a URL using a HEAD request.
///
/// # Errors
///
/// Returns an error if:
/// * The network request fails.
/// * The server returns a non-success status code.
/// * The server does not provide a `Content-Length` header.
pub async fn get_file_size(url: &str, client: &reqwest::Client) -> Result<u64> {
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
            index: i as usize,
            start,
            end,
            completed: false,
            current_offset: 0,
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

/// Extracts a clean filename from a URL.
///
/// 1. Parses the URL.
/// 2. Extracts the last segment of the path.
/// 3. URL-decodes it (converts %20 to space, etc.).
/// 4. Sanitizes it to remove characters invalid for the OS.
/// 5. Falls back to "output.bin" if no valid filename is found.
pub fn get_filename_from_url(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .map(|mut s| s.next_back().unwrap_or("").to_string())
        })
        .map(|s| percent_decode_str(&s).decode_utf8_lossy().to_string())
        .map(sanitize)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "output.bin".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_calculate_chunks_even_split() {
        // 100 bytes, 4 threads -> should be 25 bytes each
        let chunks = calculate_chunks(100, 4);
        assert_eq!(chunks.len(), 4);

        // Chunk 0: 0-24 (25 bytes)
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[0].end, 24);

        // Chunk 3: 75-99 (25 bytes)
        assert_eq!(chunks[3].start, 75);
        assert_eq!(chunks[3].end, 99);
    }

    #[test]
    fn test_calculate_chunks_remainder() {
        // 100 bytes, 3 threads -> 33, 33, 34
        let chunks = calculate_chunks(100, 3);
        assert_eq!(chunks.len(), 3);

        // Chunk 0: 0-32 (33 bytes)
        assert_eq!(chunks[0].end - chunks[0].start + 1, 33);

        // Chunk 2 (Last one): 66-99 (34 bytes)
        assert_eq!(chunks[2].end - chunks[2].start + 1, 34);
        assert_eq!(chunks[2].end, 99);
    }

    #[test]
    fn test_verify_integrity() -> Result<()> {
        // 1. Create a temp file with known content
        let mut temp_file = NamedTempFile::new()?;
        write!(temp_file, "Hello Rust")?;

        // "Hello Rust" SHA-256 hash
        let expected_hash = "DC5D63134FB696626C4BF28E1232434AB040ACC10A66CFEE55DACDD70DAE82A3";

        // 2. Verify it passes
        let path = temp_file.path().to_str().unwrap();
        let result = verify_file_integrity(path, expected_hash);
        assert!(result.is_ok());

        // 3. Verify it fails with wrong hash
        let result = verify_file_integrity(path, "badhash123");
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_filename_extraction() {
        // Simple case
        assert_eq!(
            get_filename_from_url("https://example.com/archive.zip"),
            "archive.zip"
        );

        // With query parameters (should ignore ?id=123)
        assert_eq!(
            get_filename_from_url("https://example.com/image.png?id=123&quality=high"),
            "image.png"
        );

        // With URL encoding (%20)
        assert_eq!(
            get_filename_from_url("https://example.com/my%20vacation%20photo.jpg"),
            "my vacation photo.jpg"
        );

        // Edge case: No filename (ends in slash)
        assert_eq!(get_filename_from_url("https://example.com/"), "output.bin");
    }
}
