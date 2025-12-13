//! parallel_downloader — download library
//!
//! `parallel_downloader` provides a small, composable API for performing
//! robust concurrent downloads with support for resuming, retries,
//! optional rate limiting and SHA-256 verification.
//!
//! The library is written so the CLI can reuse the same primitives; you can
//! also embed the downloader in your own programs by calling the helpers
//! exposed below.
//!
//! # Example
//!
//! ```no_run
//! use parallel_downloader::downloader::prepare_download;
//! use parallel_downloader::worker::download_chunk;
//! // Build a `reqwest::Client`, call `prepare_download` to get a `DownloadState`,
//! // then spawn `download_chunk` tasks for each chunk.
//! ```

pub mod config;
pub mod daemon;
pub mod downloader;
pub mod ipc;
pub mod observer;
pub mod state;
pub mod utils;
pub mod worker;

pub use downloader::{perform_parallel_download, prepare_download};
pub use state::DownloadState;
pub use worker::{ArcRateLimiter, download_chunk};
