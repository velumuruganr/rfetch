//! IPC types used by the daemon and CLI for local control.
//!
//! The daemon listens on a local TCP socket and accepts simple JSON
//! commands. These enums and structs define the request/response
//! messages exchanged between the CLI (`pd`) and the background daemon.
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct Request {
    pub secret: Option<String>,
    pub command: Command,
}

/// Commands that can be sent to the running daemon.
///
/// Sent from the CLI to control the background service (add jobs,
/// query status, or ask it to shutdown).
#[derive(Deserialize, Serialize, Debug)]
pub enum Command {
    /// Add a new download job with a `url` and target `dir`.
    Add {
        url: String,
        dir: String,
    },
    /// Request current status of all jobs.
    Status,
    /// Ask the daemon to gracefully shutdown.
    Shutdown,
    Pause {
        id: usize,
    },
    Resume {
        id: usize,
    },
}

/// Responses returned by the daemon for incoming `Command`s.
#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    /// Generic OK message with a short text payload.
    Ok(String),
    /// A list of job statuses returned by the `Status` command.
    StatusList(Vec<JobStatus>),
    /// Error with human-readable message.
    Err(String),
}

/// A lightweight representation of a job returned to the caller.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JobStatus {
    /// Numeric job id assigned by the daemon.
    pub id: usize,
    /// Filename being written for the job.
    pub filename: String,
    /// Download progress as a percentage (0-100).
    pub progress_percent: u64,
    /// Short human-facing state message (e.g. "Downloading...", "Done").
    pub state: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_serialize_deserialize_roundtrip() {
        let cmd = Command::Add {
            url: "https://example.com/file".into(),
            dir: "./".into(),
        };
        let json = serde_json::to_string(&cmd).expect("serialize");
        let parsed: Command = serde_json::from_str(&json).expect("deserialize");

        match parsed {
            Command::Add { url, dir } => {
                assert_eq!(url, "https://example.com/file");
                assert_eq!(dir, "./");
            }
            _ => panic!("Unexpected variant"),
        }
    }

    #[test]
    fn response_statuslist_serialization() {
        let job = JobStatus {
            id: 1,
            filename: "foo.bin".into(),
            progress_percent: 50,
            state: "Half".into(),
        };
        let resp = Response::StatusList(vec![job.clone()]);
        let json = serde_json::to_string(&resp).expect("serialize");
        let parsed: Response = serde_json::from_str(&json).expect("deserialize");

        match parsed {
            Response::StatusList(list) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].id, 1);
                assert_eq!(list[0].filename, "foo.bin");
            }
            _ => panic!("Unexpected variant"),
        }
    }
}
