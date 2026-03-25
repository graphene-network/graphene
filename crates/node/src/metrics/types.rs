use lazy_static::lazy_static;
use prometheus::{CounterVec, Gauge, HistogramOpts, HistogramVec, Opts};

lazy_static! {
    /// Total number of jobs processed, labeled by status (success/failed/timeout)
    pub static ref JOBS_TOTAL: CounterVec = CounterVec::new(
        Opts::new("opencapsule_jobs_total", "Total number of jobs processed"),
        &["status"]
    ).expect("metric can be created");

    /// Job execution duration in seconds, labeled by status
    pub static ref JOB_DURATION_SECONDS: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "opencapsule_job_duration_seconds",
            "Job execution duration in seconds"
        ).buckets(vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0]),
        &["status"]
    ).expect("metric can be created");

    /// Total cache hits, labeled by level (local/iroh/rebuild)
    pub static ref CACHE_HITS_TOTAL: CounterVec = CounterVec::new(
        Opts::new("opencapsule_cache_hits_total", "Total cache hits"),
        &["level"]
    ).expect("metric can be created");

    /// Total cache misses, labeled by level
    pub static ref CACHE_MISSES_TOTAL: CounterVec = CounterVec::new(
        Opts::new("opencapsule_cache_misses_total", "Total cache misses"),
        &["level"]
    ).expect("metric can be created");

    /// Build duration in seconds, labeled by status
    pub static ref BUILD_DURATION_SECONDS: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "opencapsule_build_duration_seconds",
            "Build duration in seconds"
        ).buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0]),
        &["status"]
    ).expect("metric can be created");

    /// Current number of active VMs
    pub static ref ACTIVE_VMS: Gauge = Gauge::new(
        "opencapsule_active_vms",
        "Number of currently active VMs"
    ).expect("metric can be created");
}

use std::sync::Once;

static INIT: Once = Once::new();

/// Force initialization of all lazy_static metrics and register them.
/// Safe to call multiple times - will only register once.
pub fn init() {
    INIT.call_once(|| {
        let registry = prometheus::default_registry();

        // Force lazy initialization by accessing each metric
        let _ = JOBS_TOTAL.with_label_values(&["success"]);
        let _ = JOB_DURATION_SECONDS.with_label_values(&["success"]);
        let _ = CACHE_HITS_TOTAL.with_label_values(&["local"]);
        let _ = CACHE_MISSES_TOTAL.with_label_values(&["local"]);
        let _ = BUILD_DURATION_SECONDS.with_label_values(&["success"]);
        let _ = ACTIVE_VMS.get();

        // Register all metrics (ignore AlreadyReg errors for robustness)
        let _ = registry.register(Box::new(JOBS_TOTAL.clone()));
        let _ = registry.register(Box::new(JOB_DURATION_SECONDS.clone()));
        let _ = registry.register(Box::new(CACHE_HITS_TOTAL.clone()));
        let _ = registry.register(Box::new(CACHE_MISSES_TOTAL.clone()));
        let _ = registry.register(Box::new(BUILD_DURATION_SECONDS.clone()));
        let _ = registry.register(Box::new(ACTIVE_VMS.clone()));
    });
}
