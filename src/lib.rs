//! # rfetch Download Library
//! 
//! `rfetch` is a library for performing robust, concurrent file downloads.
//! It supports features like:
//! - Multi-threaded downloading
//! - Resuming interrupted downloads
//! - Automatic retries on network failure
//! - Rate limiting (throttling)
//! - SHA-256 integrity verification
//! 
//! ## Example Usage
//! 
//! Note: This library is primarily designed to be used by the binary, but the internal
//! components are exposed for custom implementations.

pub mod args;
pub mod state;
pub mod utils;
pub mod worker;

pub use args::Args;
pub use state::DownloadState;
pub use worker::{ArcRateLimiter, download_chunk};
