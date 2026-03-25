//! OpenCapsule worker node HTTP server.
//!
//! Starts an axum HTTP server exposing the worker REST API for job submission,
//! status polling, result retrieval, health checks, and management operations.

use anyhow::Result;
use clap::Parser;
use opencapsule_node::executor::MockJobExecutor;
use opencapsule_node::http::{build_router, AppState};
use opencapsule_node::types::WorkerCapabilities;
use opencapsule_node::worker::{WorkerEvent, WorkerStateMachine};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tracing::info;

#[derive(Parser)]
#[command(name = "opencapsule-worker")]
#[command(about = "OpenCapsule worker node HTTP server")]
struct Args {
    /// Listen address.
    #[arg(long, default_value = "0.0.0.0:9000", env = "OPENCAPSULE_LISTEN_ADDR")]
    listen: SocketAddr,

    /// Maximum concurrent job slots.
    #[arg(long, default_value = "4", env = "OPENCAPSULE_MAX_SLOTS")]
    max_slots: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("opencapsule_node=debug".parse()?)
                .add_directive("opencapsule_worker=info".parse()?),
        )
        .init();

    let args = Args::parse();

    info!("OpenCapsule Worker Node v{}", env!("CARGO_PKG_VERSION"));
    info!("Listening on {}", args.listen);
    info!("Max slots: {}", args.max_slots);

    // Create worker state machine and bring to Online
    let worker = WorkerStateMachine::new_shared(args.max_slots);
    worker.transition(WorkerEvent::StakeConfirmed)?;
    worker.transition(WorkerEvent::JoinedGossip)?;
    info!("Worker state: {}", worker.state());

    // TODO(#200): Replace MockJobExecutor with real DefaultJobExecutor
    // when running on a host with Firecracker + kraft installed.
    let executor = Arc::new(MockJobExecutor::success());

    let capabilities = WorkerCapabilities {
        max_vcpu: num_cpus::get() as u8,
        max_memory_mb: 4096,
        kernels: vec![
            "python:3.12".to_string(),
            "python:3.10".to_string(),
            "node:20".to_string(),
        ],
        disk: None,
        gpus: vec![],
    };

    let state = Arc::new(AppState::new(executor, worker, capabilities));
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    info!("Server ready — accepting jobs");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received");
}
