use clap::Parser;

/// A fast, concurrent file downloader built in Rust.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The URL of the file to download
    #[arg(short, long)]
    pub url: String,

    /// The output file name
    #[arg(short, long)]
    pub output: Option<String>,

    /// Number of threads to use
    #[arg(short = 't', long, default_value_t = 4)]
    pub threads: u8,

    #[arg(long)]
    pub verify_sha256: Option<String>,

    #[arg(long)]
    pub rate_limit: Option<u32>,
}
