use super::types::{
    ACTIVE_VMS, BUILD_DURATION_SECONDS, CACHE_HITS_TOTAL, CACHE_MISSES_TOTAL, JOBS_TOTAL,
    JOB_DURATION_SECONDS,
};
use std::time::Instant;

/// Job execution status for metrics labeling
#[derive(Debug, Clone, Copy)]
pub enum JobStatus {
    Success,
    Failed,
    Timeout,
}

impl JobStatus {
    fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Success => "success",
            JobStatus::Failed => "failed",
            JobStatus::Timeout => "timeout",
        }
    }
}

/// Build execution status for metrics labeling
#[derive(Debug, Clone, Copy)]
pub enum BuildStatus {
    Success,
    Failed,
}

impl BuildStatus {
    fn as_str(&self) -> &'static str {
        match self {
            BuildStatus::Success => "success",
            BuildStatus::Failed => "failed",
        }
    }
}

/// Cache level for metrics labeling
#[derive(Debug, Clone, Copy)]
pub enum CacheLevel {
    Local,
    Iroh,
    Rebuild,
}

impl CacheLevel {
    fn as_str(&self) -> &'static str {
        match self {
            CacheLevel::Local => "local",
            CacheLevel::Iroh => "iroh",
            CacheLevel::Rebuild => "rebuild",
        }
    }
}

/// RAII timer for job execution metrics.
///
/// On creation, increments the active VM gauge.
/// When `complete()` is called, records the duration and job count.
/// On drop, decrements the active VM gauge.
pub struct JobTimer {
    start: Instant,
    completed: bool,
}

impl JobTimer {
    /// Create a new job timer and increment active VMs
    pub fn new() -> Self {
        ACTIVE_VMS.inc();
        Self {
            start: Instant::now(),
            completed: false,
        }
    }

    /// Complete the job with the given status, recording duration and count
    pub fn complete(mut self, status: JobStatus) {
        self.completed = true;
        let duration = self.start.elapsed().as_secs_f64();
        let label = status.as_str();

        JOBS_TOTAL.with_label_values(&[label]).inc();
        JOB_DURATION_SECONDS
            .with_label_values(&[label])
            .observe(duration);
    }
}

impl Default for JobTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for JobTimer {
    fn drop(&mut self) {
        ACTIVE_VMS.dec();
        // If not completed, record as failed (e.g., panic during execution)
        if !self.completed {
            let duration = self.start.elapsed().as_secs_f64();
            JOBS_TOTAL.with_label_values(&["failed"]).inc();
            JOB_DURATION_SECONDS
                .with_label_values(&["failed"])
                .observe(duration);
        }
    }
}

/// RAII timer for build operations
pub struct BuildTimer {
    start: Instant,
    completed: bool,
}

impl BuildTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            completed: false,
        }
    }

    pub fn complete(mut self, status: BuildStatus) {
        self.completed = true;
        let duration = self.start.elapsed().as_secs_f64();
        let label = status.as_str();

        BUILD_DURATION_SECONDS
            .with_label_values(&[label])
            .observe(duration);
    }
}

impl Default for BuildTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BuildTimer {
    fn drop(&mut self) {
        if !self.completed {
            let duration = self.start.elapsed().as_secs_f64();
            BUILD_DURATION_SECONDS
                .with_label_values(&["failed"])
                .observe(duration);
        }
    }
}

/// Record a cache hit at the given level
pub fn record_cache_hit(level: CacheLevel) {
    CACHE_HITS_TOTAL.with_label_values(&[level.as_str()]).inc();
}

/// Record a cache miss at the given level
pub fn record_cache_miss(level: CacheLevel) {
    CACHE_MISSES_TOTAL
        .with_label_values(&[level.as_str()])
        .inc();
}
