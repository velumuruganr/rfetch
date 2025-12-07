# Parallel Downloader (pd) 🦀

A robust, concurrent file downloader built in Rust. It is designed to be resilient, supporting automatic retries, crash recovery, and download verification.

[![Build Status](https://github.com/velumuruganr/parallel_downloader/actions/workflows/rust.yml/badge.svg)](https://github.com/velumuruganr/parallel_downloader/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## 🚀 Features

* **Concurrency:** Downloads files in parallel chunks to maximize bandwidth.
* **Resiliency:** Automatically retries failed chunks on network timeouts.
* **Crash Recovery:** Saves progress to a state file (`.state.json`). If the program crashes or is interrupted, simply run the command again to resume exactly where it left off.
* **Rate Limiting:** Optional token-bucket throttling to limit bandwidth usage.
* **Integrity Check:** Verifies the final file against a SHA-256 hash.
* **Library Support:** Logic is separated from the CLI, allowing you to use the core downloader in your own Rust applications.
* **🧠 Batch Processing:**
    * Supports a Daemon mode (`pd start`) for background management.
    * Accepts input files (`-i list.txt`) for bulk downloads.
---

## 📦 Installation

### Pre-built Binaries
Download the latest release for Windows, Linux, or macOS from the [Releases Page](https://github.com/velumuruganr/parallel_downloader/releases).

### Build from Source
```bash
git clone https://github.com/yourusername/parallel_downloader.git
cd parallel_downloader
cargo install --path .
```

## 🛠 Usage

**1. Standalone Mode (CLI)**

Best for quick, one-off downloads.

```bash
# Simple download (defaults to 4 threads)
pd run --url "https://example.com/large.iso"

# Advanced Usage
pd run \
  --url "https://example.com/movie.mp4" \
  --output "holiday.mp4" \
  --threads 8 \
  --rate-limit 1048576 \
  --dir ./downloads
```

**2. Batch Mode**

Download a list of URLs (one per line).

```bash
# Download files from list.txt, 3 files at a time
pd run -i list.txt -c 3
```

**3. Daemon Mode (Background Service)**

Best for long-running servers or managing queues.

```bash
# 1. Start the daemon in a separate terminal
pd start

# 2. Add downloads from any terminal
pd add --url "https://example.com/file1.zip"
pd add --url "https://example.com/file2.zip"

# 3. Check status
pd status

# 4. Shutdown
pd stop
```

## Options
| Flag                       | Description                                       | Default       |
|----------------------------|---------------------------------------------------|---------------|
| `--url`, `-u`              | The URL to download                               | Required      |
| `--output`, `-o`           | Output filename                                   | `output.bin`  |
| `--threads`, `-t`          | Number of concurrent threads                      | `4`           |
| `--rate-limit`             | Max speed in bytes/sec (e.g., 1048576 = 1MB/s)    | Unlimited     |
| `--verify-sha256`          | Hash string to verify file integrity              | None          |
| `--dir`, `-d`              | Directory to store downloads                      | Current Dir   |
| `--input`, `-i`            | Input file with list of URLs (one per line)       | None          |
| `--concurrent_files`, `-c` | Number of concurrent downloads in batch mode      | `2`           |

## 📚 Library Usage

You can use `parallel_downloader` as a library in your own project.

Add to your `Cargo.toml`:

```ini,toml
[dependencies]
parallel_downloader = { version = "0.2", default-features = false }
```
Use the modules in your code:
```rust
use parallel_downloader::downloader::prepare_download;
use parallel_downloader::worker::download_chunk;
```

## 🧪 Testing

We use `wiremock` to simulate HTTP failures and chunk ranges without hitting real servers.
```bash
cargo test
```

## 📂 Examples

The repository includes runnable examples in the `examples/` folder to demonstrate how to use `parallel_downloader` programmatically in your own applications.

### Simple Download
This example demonstrates a complete workflow: creating a client, verifying file size, and downloading chunks with a progress bar.

To run the example:
```bash
cargo run --example simple_download
```

You can view the full source code in [examples/simple_download.rs](https://github.com/velumuruganr/parallel_downloader/blob/master/examples/simple_download.rs).