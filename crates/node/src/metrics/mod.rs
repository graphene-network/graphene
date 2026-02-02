pub mod recorder;
pub mod types;

pub use recorder::{
    record_cache_hit, record_cache_miss, BuildStatus, BuildTimer, CacheLevel, JobStatus, JobTimer,
};

use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use prometheus::{Encoder, TextEncoder};
use std::net::SocketAddr;

/// Configuration for the metrics HTTP server
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// Address to bind the metrics server to
    pub bind_addr: SocketAddr,
    /// Whether metrics collection is enabled
    pub enabled: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 9100)),
            enabled: true,
        }
    }
}

impl MetricsConfig {
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            enabled: true,
        }
    }

    pub fn with_port(port: u16) -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], port)),
            enabled: true,
        }
    }
}

/// Start the Prometheus metrics HTTP server
///
/// This should be spawned as a separate tokio task:
/// ```ignore
/// tokio::spawn(metrics::start_metrics_server(MetricsConfig::default()));
/// ```
pub async fn start_metrics_server(config: MetricsConfig) -> Result<(), std::io::Error> {
    if !config.enabled {
        tracing::info!("Metrics server disabled");
        return Ok(());
    }

    // Initialize all metrics
    types::init();

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler));

    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!("Metrics server listening on {}", config.bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Handler for /metrics endpoint - returns Prometheus text format
async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();

    let mut buffer = Vec::new();
    match encoder.encode(&metric_families, &mut buffer) {
        Ok(_) => (
            StatusCode::OK,
            [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
            buffer,
        ),
        Err(e) => {
            tracing::error!("Failed to encode metrics: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "text/plain; charset=utf-8")],
                format!("Failed to encode metrics: {}", e).into_bytes(),
            )
        }
    }
}

/// Handler for /health endpoint
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_config_default() {
        let config = MetricsConfig::default();
        assert_eq!(config.bind_addr.port(), 9100);
        assert!(config.enabled);
    }

    #[test]
    fn test_metrics_config_with_port() {
        let config = MetricsConfig::with_port(9200);
        assert_eq!(config.bind_addr.port(), 9200);
    }

    #[test]
    fn test_job_timer_records_metrics() {
        types::init();

        let timer = JobTimer::new();
        timer.complete(JobStatus::Success);

        // Verify the metric was recorded
        let count = types::JOBS_TOTAL.with_label_values(&["success"]).get();
        assert!(count >= 1.0);
    }

    #[test]
    fn test_cache_hit_miss_recording() {
        types::init();

        record_cache_hit(CacheLevel::Local);
        record_cache_miss(CacheLevel::Local);

        let hits = types::CACHE_HITS_TOTAL.with_label_values(&["local"]).get();
        let misses = types::CACHE_MISSES_TOTAL
            .with_label_values(&["local"])
            .get();

        assert!(hits >= 1.0);
        assert!(misses >= 1.0);
    }
}
