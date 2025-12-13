# Parallel Downloader (pd) 🦀

A robust, concurrent file downloader built in Rust. It is designed to be resilient, supporting automatic retries, crash recovery, and download verification.

[![Build Status](https://github.com/velumuruganr/parallel_downloader/actions/workflows/rust.yml/badge.svg)](https://github.com/velumuruganr/parallel_downloader/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Crates.io](https://img.shields.io/crates/v/parallel_downloader.svg)](https://crates.io/crates/parallel_downloader)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

## 📋 Table of Contents

- [🚀 Features](#🚀-features)
- [🔧 How It Works](#🔧-how-it-works)
- [📦 Installation](#📦-installation)
  - [Pre-built Binaries](#pre-built-binaries)
  - [Build from Source](#build-from-source)
- [🛠 Usage](#🛠-usage)
- [Options](#options)
- [Commands](#commands)
- [Configuration](#configuration)
- [📚 Library Usage](#📚-library-usage)
- [🧪 Testing](#🧪-testing)
- [📂 Examples](#📂-examples)
  - [Simple Download](#simple-download)
- [🔧 Troubleshooting](#🔧-troubleshooting)
- [🤝 Contributing](#🤝-contributing)
- [📄 License](#📄-license)

## 🚀 Features

* **Concurrency:** Downloads files in parallel chunks to maximize bandwidth.
* **Resiliency:** Automatically retries failed chunks on network timeouts.
* **Crash Recovery:** Saves progress to a state file (`.state.json`). If the program crashes or is interrupted, simply run the command again to resume exactly where it left off.
* **Rate Limiting:** Optional token-bucket throttling to limit bandwidth usage.
* **Integrity Check:** Verifies the final file against a SHA-256 hash.
* **Library Support:** Logic is separated from the CLI, allowing you to use the core downloader in your own Rust applications.
* **Batch Processing:** Supports input files (`-i list.txt`) for bulk downloads.
* **Background Daemon:** Run downloads in the background with job management.
* **Interactive TUI:** Terminal-based dashboard for monitoring downloads.
* **Job Control:** Pause and resume individual downloads in daemon mode.
* **Authentication:** Optional server secret for securing daemon access.
---

## 🔧 How It Works

Parallel Downloader uses a **chunk-based parallel downloading** approach:

1. **File Analysis**: First, it determines the total file size using HTTP HEAD requests
2. **Chunk Division**: The file is divided into equal-sized chunks (typically based on thread count)
3. **Parallel Download**: Each chunk is downloaded concurrently using HTTP Range requests
4. **Progress Tracking**: Individual progress bars show download status for each chunk
5. **State Persistence**: Download progress is saved to `*.state.json` files for resumability
6. **Integrity Verification**: Optional SHA-256 verification ensures file correctness

This approach maximizes bandwidth utilization while providing robust error recovery and resumability.

---

## 📦 Installation

### Pre-built Binaries
Download the latest release for Windows, Linux, or macOS from the [Releases Page](https://github.com/velumuruganr/parallel_downloader/releases).

### Build from Source
```bash
git clone https://github.com/velumuruganr/parallel_downloader.git
cd parallel_downloader
cargo install --path .
```

Note: The CLI binary is provided in `src/bin/pd/main.rs` (the CLI glue is in `src/bin/pd/`). When developing from source you can run the binary directly with:

```bash
# Run the library's CLI binary from source
cargo run --bin pd -- run --url "https://example.com/large.iso"
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

# 4. Control downloads
pd pause 1    # Pause download with ID 1
pd resume 1   # Resume download with ID 1

# 5. Interactive dashboard
pd tui        # Open terminal user interface

# 6. Shutdown
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
| `--concurrent_files`, `-c` | Number of concurrent downloads in batch mode      | `3`           |

## Commands
| Command    | Description                                       |
|------------|---------------------------------------------------|
| `pd run`   | Download a single file or batch of files          |
| `pd start` | Start the background daemon                       |
| `pd add`   | Add a download to the running daemon              |
| `pd status`| List active downloads                             |
| `pd pause` | Pause a specific download by ID                   |
| `pd resume`| Resume a specific download by ID                  |
| `pd tui`   | Open interactive terminal dashboard               |
| `pd stop`  | Stop the daemon                                   |

## Configuration

This project supports reading settings from a `config.toml` file or from
environment variables prefixed with `PD`.

- File locations (platform-specific):
  - Linux:   `~/.config/pd/config.toml`
  - macOS:   `~/Library/Application Support/pd/config.toml`
  - Windows: `%APPDATA%\pd\config.toml`

- Example: copy `config.example.toml` to one of the locations above and edit
  values such as `threads`, `rate_limit`, `default_dir`, `concurrent_files`,
  `server_secret`, and `server_addr`.

- Environment variables: you can override values via environment variables using
  `__` as a separator. For example:

```bash
# set threads to 8
export PD__threads=8

# set global rate limit to 1 MiB/s
export PD__rate_limit=1048576

# set server secret for daemon authentication
export PD__server_secret="my-secret-key"

# set server bind address
export PD__server_addr="0.0.0.0"
```

Environment variables take precedence over values found in `config.toml`.

## 📚 Library Usage

You can use `parallel_downloader` as a library in your own project.

Add to your `Cargo.toml`:

```ini,toml
[dependencies]
parallel_downloader = { version = "0.3.0", default-features = false }
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

## 🔧 Troubleshooting

### Common Issues

**Download fails with "Connection reset" errors**
- Try reducing the number of threads: `pd run --threads 2 --url <URL>`
- Check your internet connection stability
- Some servers may not support concurrent connections

**State file corruption**
- Delete the `.state.json` file and restart the download
- The state file is created alongside your download with the same name

**Daemon connection refused**
- Ensure the daemon is running: `pd start`
- Check if the port is available (default: 127.0.0.1:9090)
- Verify server address in config if changed

**High CPU usage**
- Reduce thread count or add rate limiting
- This is normal for parallel downloads

**Permission denied errors**
- Check write permissions in the target directory
- Use `sudo` if downloading to system directories

### Getting Help

- Check existing [GitHub Issues](https://github.com/velumuruganr/parallel_downloader/issues)
- Create a new issue with:
  - Your OS and Rust version
  - Command used and full error output
  - URL (anonymized if needed)

## 🤝 Contributing

We welcome contributions! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

### Development Setup

1. Fork the repository
2. Clone your fork: `git clone https://github.com/velumuruganr/parallel_downloader.git`
3. Create a feature branch: `git checkout -b feature/amazing-feature`
4. Make your changes and add tests
5. Run tests: `cargo test`
6. Commit your changes: `git commit -m 'Add amazing feature'`
7. Push to the branch: `git push origin feature/amazing-feature`
8. Open a Pull Request

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.