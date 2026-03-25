//! Axum router construction for the worker HTTP API.

use crate::http::handlers;
use crate::http::management;
use crate::http::state::AppState;
use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;

/// Build the complete HTTP API router.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Job endpoints
        .route("/v1/jobs", post(handlers::submit_job))
        .route("/v1/jobs/{id}", get(handlers::get_job_status))
        .route("/v1/jobs/{id}/result", get(handlers::get_job_result))
        // Health & discovery
        .route("/v1/health", get(handlers::health))
        .route("/v1/capabilities", get(handlers::capabilities))
        // Management endpoints
        .route("/v1/management/status", get(management::get_status))
        .route("/v1/management/metrics", get(management::get_metrics))
        .route(
            "/v1/management/config",
            get(management::get_config).post(management::apply_config),
        )
        .route(
            "/v1/management/lifecycle/{action}",
            post(management::lifecycle_action),
        )
        .with_state(state)
}
