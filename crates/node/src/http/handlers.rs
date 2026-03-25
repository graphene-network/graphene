//! HTTP request handlers for the worker API.

use crate::api::{
    ApiError, CapabilitiesResponse, HealthResponse, JobResultResponse, JobStatusResponse,
    SubmitJobRequest, SubmitJobResponse,
};
use crate::executor::ExecutionRequest;
use crate::http::state::AppState;
use crate::job::{Job, JobState};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use tracing::{info, warn};

type ApiResult<T> = Result<(StatusCode, Json<T>), (StatusCode, Json<ApiError>)>;

/// POST /v1/jobs — Submit a new job for execution.
pub async fn submit_job(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubmitJobRequest>,
) -> ApiResult<SubmitJobResponse> {
    // Check if worker can accept jobs
    if !state.worker.can_accept_job() {
        return Err((StatusCode::SERVICE_UNAVAILABLE, Json(ApiError::capacity_full())));
    }

    // Try to reserve a slot
    let slot_guard = match state.worker.try_reserve_slot() {
        Ok(guard) => guard,
        Err(_) => {
            return Err((StatusCode::SERVICE_UNAVAILABLE, Json(ApiError::capacity_full())));
        }
    };

    // Generate job ID
    let job_id = uuid::Uuid::new_v4().to_string();

    // Create job and transition to Accepted
    let mut job = Job::new(&job_id);
    if let Err(e) = job.transition(JobState::Accepted) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal(e.to_string())),
        ));
    }

    // Store job
    {
        let mut jobs = state.jobs.write().await;
        jobs.insert(job_id.clone(), job);
    }

    let response = SubmitJobResponse {
        job_id: job_id.clone(),
        status: JobState::Accepted,
    };

    // Spawn execution task
    let executor = state.executor.clone();
    let jobs = state.jobs.clone();
    let results = state.results.clone();
    let exec_job_id = job_id.clone();
    let manifest = req.manifest.clone();
    let assets = req.assets.clone();

    tokio::spawn(async move {
        // Keep slot guard alive until execution completes
        let _guard = slot_guard;

        let exec_request = ExecutionRequest::new(&exec_job_id, manifest, assets);

        // Transition to Building
        {
            let mut jobs = jobs.write().await;
            if let Some(job) = jobs.get_mut(&exec_job_id) {
                let _ = job.transition(JobState::Building);
            }
        }

        // Transition to Running
        {
            let mut jobs = jobs.write().await;
            if let Some(job) = jobs.get_mut(&exec_job_id) {
                let _ = job.transition(JobState::Running);
            }
        }

        // Execute
        match executor.execute(exec_request).await {
            Ok(result) => {
                let exit_code = result.exit_code;
                info!(job_id = %exec_job_id, exit_code, "Job completed");

                // Transition to terminal state
                {
                    let mut jobs = jobs.write().await;
                    if let Some(job) = jobs.get_mut(&exec_job_id) {
                        let target_state = if result.succeeded() {
                            JobState::Succeeded
                        } else {
                            JobState::Failed
                        };
                        let _ = job.transition_with_exit_code(target_state, exit_code);
                        // Mark as delivered (sync mode — result available inline)
                        let _ = job.transition_to_delivered_sync();
                    }
                }

                // Store result
                {
                    let mut results = results.write().await;
                    results.insert(exec_job_id, result);
                }
            }
            Err(e) => {
                warn!(job_id = %exec_job_id, error = %e, "Job execution failed");

                let mut jobs = jobs.write().await;
                if let Some(job) = jobs.get_mut(&exec_job_id) {
                    let _ = job.transition_with_exit_code(
                        JobState::Failed,
                        crate::job::exit_code::WORKER_CRASH,
                    );
                    let _ = job.transition_to_delivered_sync();
                }
            }
        }
    });

    Ok((StatusCode::ACCEPTED, Json(response)))
}

/// GET /v1/jobs/:id — Get job status.
pub async fn get_job_status(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> ApiResult<JobStatusResponse> {
    let jobs = state.jobs.read().await;
    let job = jobs.get(&job_id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(ApiError::not_found(&job_id)))
    })?;

    Ok((
        StatusCode::OK,
        Json(JobStatusResponse {
            job_id: job.id.clone(),
            state: job.state,
            metrics: job.compute_metrics(),
            exit_code: job.exit_code,
        }),
    ))
}

/// GET /v1/jobs/:id/result — Get job result.
pub async fn get_job_result(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> ApiResult<JobResultResponse> {
    // Check job exists
    {
        let jobs = state.jobs.read().await;
        let job = jobs.get(&job_id).ok_or_else(|| {
            (StatusCode::NOT_FOUND, Json(ApiError::not_found(&job_id)))
        })?;

        // Check if result is ready
        if !job.state.is_execution_complete() {
            return Err((StatusCode::ACCEPTED, Json(ApiError::not_ready())));
        }
    }

    // Get result
    let results = state.results.read().await;
    let result = results.get(&job_id).ok_or_else(|| {
        (StatusCode::ACCEPTED, Json(ApiError::not_ready()))
    })?;

    Ok((
        StatusCode::OK,
        Json(JobResultResponse {
            job_id,
            exit_code: result.exit_code,
            duration_ms: result.duration_ms(),
            stdout: result.stdout.clone(),
            stderr: result.stderr.clone(),
            result: result.result.clone(),
            result_hash: hex::encode(result.result_hash),
        }),
    ))
}

/// GET /v1/health — Health check.
pub async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let worker_state = state.worker.state();
    let status = if worker_state.is_active() {
        "ok"
    } else {
        "degraded"
    };

    Json(HealthResponse {
        status: status.to_string(),
        worker_state: worker_state.to_string(),
        available_slots: state.worker.available_slots(),
        uptime_secs: state.uptime_secs(),
    })
}

/// GET /v1/capabilities — List supported runtimes and capabilities.
pub async fn capabilities(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(CapabilitiesResponse {
        capabilities: (*state.capabilities).clone(),
        runtimes: state.capabilities.kernels.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::MockJobExecutor;
    use crate::http::router::build_router;
    use crate::types::WorkerCapabilities;
    use crate::worker::{WorkerEvent, WorkerStateMachine};
    use axum::body::Body;
    use axum::http::Request;
    use std::time::Duration;
    use tower::ServiceExt;

    fn make_test_state() -> Arc<AppState> {
        let executor = Arc::new(MockJobExecutor::success());
        let worker = WorkerStateMachine::new_shared(4);
        // Get to Online state
        worker.transition(WorkerEvent::StakeConfirmed).unwrap();
        worker.transition(WorkerEvent::JoinedGossip).unwrap();

        let caps = WorkerCapabilities {
            max_vcpu: 4,
            max_memory_mb: 4096,
            kernels: vec!["python:3.12".to_string(), "node:20".to_string()],
            disk: None,
            gpus: vec![],
        };

        Arc::new(AppState::new(executor, worker, caps))
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = make_test_state();
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let health: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.status, "ok");
        assert_eq!(health.available_slots, 4);
    }

    #[tokio::test]
    async fn test_capabilities_endpoint() {
        let state = make_test_state();
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/capabilities")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let caps: CapabilitiesResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(caps.runtimes.len(), 2);
    }

    #[tokio::test]
    async fn test_submit_and_poll_job() {
        let state = make_test_state();
        let app = build_router(state);

        // Submit job
        let submit_body = serde_json::json!({
            "manifest": {
                "vcpu": 1,
                "memory_mb": 256,
                "timeout_ms": 5000,
                "runtime": "python:3.12",
                "egress_allowlist": [],
                "env": {}
            },
            "assets": {
                "code": { "Inline": { "data": [112, 114, 105, 110, 116] } },
                "files": [],
                "compression": "none"
            }
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/jobs")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&submit_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let submit_resp: SubmitJobResponse = serde_json::from_slice(&body).unwrap();
        assert!(!submit_resp.job_id.is_empty());
    }

    #[tokio::test]
    async fn test_get_nonexistent_job() {
        let state = make_test_state();
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/jobs/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
