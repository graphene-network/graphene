use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct JobRequest {
    pub job_id: String,
    pub code: String,
    pub requirements: Vec<String>, // e.g. ["pandas", "numpy"]
    /// Runtime specification, e.g. "python-3.11", "node-22"
    #[serde(default = "default_runtime")]
    pub runtime: String,
}

fn default_runtime() -> String {
    "python-3.11".to_string()
}

#[derive(Serialize, Debug)]
pub struct JobResponse {
    pub status: String,
    pub message: String,
    pub computation_time_ms: u64,
}
