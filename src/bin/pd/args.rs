//! Command-line argument definitions for the `pd` binary.
//!
//! This module defines the parsed CLI structures using `clap`. The
//! `Args` and `Commands` types are exported by the crate so the
//! library binary can consume them when assembling behavior.
use clap::{Parser, Subcommand};

/// A robust, concurrent file downloader.
///
/// This tool splits files into chunks and downloads them in parallel,
/// with support for resuming and rate limiting.
#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the background daemon
    Start,
    /// Add a download to the running daemon
    Add {
        /// URL to download
        #[arg(short, long)]
        url: String,
        /// Directory path
        #[arg(short, long)]
        dir: String,
    },
    /// List active downloads
    Status,
    /// Stop the daemon
    Stop,
    /// Run in standalone mode
    Run {
        /// The URL of the file to download.
        #[arg(short, long)]
        url: String,

        /// The name of the output file. If not provided, defaults to "output.bin".
        #[arg(short, long)]
        output: Option<String>,

        /// The number of concurrent download threads to use.
        #[arg(short = 't', long)]
        threads: Option<u8>,

        /// An optional SHA-256 hash to verify file integrity after download.
        #[arg(long)]
        verify_sha256: Option<String>,

        /// A rate limit in bytes per second (e.g., 1048576 for 1MB/s).
        #[arg(long)]
        rate_limit: Option<u32>,

        /// The directory to save the file in. Defaults to the current directory.
        #[arg(short = 'd', long)]
        dir: Option<String>,

        /// File containing a list of URLs to download (one per line).
        #[arg(short = 'i', long)]
        input: Option<String>,

        /// Max number of files to download concurrently (Batch mode only).
        #[arg(short = 'c', long)]
        concurrent_files: Option<usize>,
    },
    /// Open the interactive TUI dashboard.
    Tui,
    /// Pause a specific download
    Pause { id: usize },
    /// Resume a specific download
    Resume { id: usize },
}
