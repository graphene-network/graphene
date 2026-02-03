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

/// Cache level for metrics labeling.
///
/// The L1/L2/L3 hierarchy represents:
/// - **L1**: Pre-built kernel binaries (~100% hit rate)
/// - **L2**: Kernel + dependencies (~95% hit rate)
/// - **L3**: Full builds including user code
///
/// Each level can be served from local disk or Iroh P2P.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLevel {
    /// Legacy: Local disk cache (backward compatibility).
    Local,
    /// Legacy: Iroh P2P cache (backward compatibility).
    Iroh,
    /// Legacy: Cache miss, rebuild required (backward compatibility).
    Rebuild,

    /// L1 kernel cache hit (local).
    L1Kernel,
    /// L2 dependencies cache hit (local).
    L2DepsLocal,
    /// L2 dependencies cache hit (Iroh P2P).
    L2DepsIroh,
    /// L3 full build cache hit (local).
    L3Local,
    /// L3 full build cache hit (Iroh P2P).
    L3Iroh,
    /// L3 cache miss, rebuild required.
    L3Rebuild,
}

impl CacheLevel {
    fn as_str(&self) -> &'static str {
        match self {
            CacheLevel::Local => "local",
            CacheLevel::Iroh => "iroh",
            CacheLevel::Rebuild => "rebuild",
            CacheLevel::L1Kernel => "l1_kernel",
            CacheLevel::L2DepsLocal => "l2_deps_local",
            CacheLevel::L2DepsIroh => "l2_deps_iroh",
            CacheLevel::L3Local => "l3_local",
            CacheLevel::L3Iroh => "l3_iroh",
            CacheLevel::L3Rebuild => "l3_rebuild",
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
