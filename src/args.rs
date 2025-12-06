use clap::Parser;

/// A robust, concurrent file downloader.
/// 
/// This tool splits files into chunks and downloads them in parallel,
/// with support for resuming and rate limiting.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The URL of the file to download.
    #[arg(short, long)]
    pub url: String,

    /// The name of the output file. If not provided, defaults to "output.bin".
    #[arg(short, long)]
    pub output: Option<String>,

    /// The number of concurrent download threads to use.
    #[arg(short = 't', long, default_value_t = 4)]
    pub threads: u8,

    /// An optional SHA-256 hash to verify file integrity after download.
    #[arg(long)]
    pub verify_sha256: Option<String>,

    /// A rate limit in bytes per second (e.g., 1048576 for 1MB/s).
    #[arg(long)]
    pub rate_limit: Option<u32>,
}
