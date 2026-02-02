use serde::{Deserialize, Serialize};

pub const TALOS_ALPN: &[u8] = b"monad/job/1";

/// The message Node A sends to Node B
#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    /// "Please run this job for me"
    JobRequest {
        job_id: String,
        code: String,
        requirements: Vec<String>,
    },
    /// "I finished. Here is the result."
    JobResult {
        job_id: String,
        output: String,
        status: String,
    },
}
