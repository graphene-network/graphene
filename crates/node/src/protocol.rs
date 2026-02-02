use serde::{Deserialize, Serialize};

pub const TALOS_ALPN: &[u8] = b"monad/job/1";

fn default_runtime() -> String {
    "python-3.11".to_string()
}

/// The message Node A sends to Node B
#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    /// "Please run this job for me"
    JobRequest {
        job_id: String,
        code: String,
        requirements: Vec<String>,
        /// Runtime specification, e.g. "python-3.11", "node-22"
        #[serde(default = "default_runtime")]
        runtime: String,
    },
    /// "I finished. Here is the result."
    JobResult {
        job_id: String,
        output: String,
        status: String,
    },
}
