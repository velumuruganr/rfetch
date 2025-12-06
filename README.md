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
git clone https://github.com/velumuruganr/rfetch.git
cd rfetch
cargo build --release
```
The binary will be located in ./target/release/rfetch

## 🛠 Usage

### Basic Download

Download a file with default settings (4 threads).
```bash
cargo run -- --url "https://proof.ovh.net/files/100Mb.dat"
```

### Advanced Usage

Combine flags for a specific use case:
```bash
cargo run -- \
  --url "https://proof.ovh.net/files/100Mb.dat" \
  --output "my_video.dat" \
  --threads 8 \
  --rate-limit 512000 \
  --verify-sha256 "a1b2c3d4..."
```

## Options
| Flag              | Description                                       | Default       |
|-------------------|---------------------------------------------------|---------------|
| `--url`           | The URL to download                               | Required      |
| `--output`, `-o`  | Output filename                                   | `output.bin`  |
| `--threads`, `-t` | Number of concurrent threads                      | `4`           |
| `--rate-limit`    | Max speed in bytes/sec (e.g., 1048576 = 1MB/s)    | Unlimited     |
| `--verify-sha256` | Hash string to verify file integrity              | None          |

## 📚 Library Usage

You can use rfetch as a library in your own project.

Add to your `Cargo.toml`:
```
[dependencies]
rfetch = { path = "../path/to/rfetch" }
```

Use the modules in your code:
```Rust
use rfetch::utils::get_file_size;
use rfetch::worker::download_chunk;

#[tokio::main]
async fn main() {
    let size = get_file_size("https://example.com/file.zip").await.unwrap();
    println!("File size: {}", size);
}
```

## 🧪 Testing
Run the test suite, which includes unit tests for math/hashing and integration tests using a mock HTTP server.
```Bash
cargo test
```

## 📄 License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
