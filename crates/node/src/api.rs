use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct JobRequest {
    pub job_id: String,
    pub code: String,              // The Python script
    pub requirements: Vec<String>, // e.g. ["pandas", "numpy"]
}

#[derive(Serialize, Debug)]
pub struct JobResponse {
    pub status: String,
    pub message: String,
    pub computation_time_ms: u64,
}
