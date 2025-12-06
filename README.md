# rfetch Downloader 🦀

A robust, concurrent file downloader built in Rust. It is designed to be resilient, supporting automatic retries, crash recovery, and download verification.

## 🚀 Features

* **Concurrency:** Downloads files in parallel chunks to maximize bandwidth.
* **Resiliency:** Automatically retries failed chunks on network timeouts.
* **Crash Recovery:** Saves progress to a state file (`.state.json`). If the program crashes or is interrupted, simply run the command again to resume exactly where it left off.
* **Rate Limiting:** Optional token-bucket throttling to limit bandwidth usage.
* **Integrity Check:** Verifies the final file against a SHA-256 hash.
* **Library Support:** Logic is separated from the CLI, allowing you to use the core downloader in your own Rust applications.

## 📦 Installation

Ensure you have Rust installed. Clone the repository and build:

```bash
git clone [https://github.com/velumuruganr/rfetch.git](https://github.com/velumuruganr/rfetch.git)
cd rfetch
cargo build --release