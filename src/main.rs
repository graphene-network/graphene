mod api;
mod builder;
mod cache;
mod vmm;

use axum::{
    Router,
    extract::{Json, State},
    routing::post,
};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

// Import traits
use api::{JobRequest, JobResponse};
use builder::{DriveBuilder, mock::MockBuilder}; // Swap with LinuxBuilder in prod
use cache::{DependencyCache, local::LocalDiskCache, mock::MockCache};
use vmm::{Virtualizer, mock::MockBehavior, mock::MockVirtualizer};

// --- APPLICATION STATE ---
// This holds the "Singletons" that our API handlers need access to.
struct AppState {
    builder: Box<dyn DriveBuilder>,
    cache: Box<dyn DependencyCache>,
    // We use a Mutex for the VMM because Firecracker is single-threaded per VM
    // In a real node, you'd have a Pool of VMMs.
    vmm_factory: Box<dyn Fn() -> Box<dyn Virtualizer> + Send + Sync>,
}

// --- MAIN SERVER STARTUP ---
#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🦀 Talos Worker Node Initializing...");

    // 1. Setup the Layer Stack (Using Mocks for Mac dev)
    let shared_state = Arc::new(AppState {
        builder: Box::new(MockBuilder::new()),
        cache: Box::new(MockCache::new()), // or LocalDiskCache::new("./cache")
        vmm_factory: Box::new(|| {
            // Check for KVM or use Mock
            if std::path::Path::new("/dev/kvm").exists() {
                // Return Real Firecracker (impl required)
                Box::new(MockVirtualizer::new(MockBehavior::HappyPath)) // Placeholder
            } else {
                Box::new(MockVirtualizer::new(MockBehavior::HappyPath))
            }
        }),
    });

    // 2. Define Routes
    let app = Router::new()
        .route("/submit", post(submit_job))
        .with_state(shared_state);

    // 3. Start Server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("🚀 Server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}

// --- API HANDLER ---
async fn submit_job(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<JobRequest>,
) -> Json<JobResponse> {
    println!("📩 Received Job: {}", payload.job_id);
    let start_time = Instant::now();

    // 1. RESOLVE DEPENDENCIES (JIT Layer)
    // Note: We use the helper function logic we wrote earlier
    let deps_hash = state.cache.calculate_hash(&payload.requirements);

    // Check Cache
    let deps_path = match state.cache.get(&deps_hash).await.unwrap() {
        Some(path) => path,
        None => {
            println!("🧊 Cache Miss. Building...");
            let new_path = state
                .builder
                .build_dependency_drive(&payload.job_id, payload.requirements)
                .await
                .unwrap();
            state.cache.put(&deps_hash, new_path).await.unwrap()
        }
    };

    // 2. BUILD CODE LAYER
    let code_path = state
        .builder
        .create_code_drive(&payload.job_id, &payload.code)
        .await
        .unwrap();

    // 3. BOOT VM
    // Instantiate a fresh VM for this request
    let mut machine = (state.vmm_factory)();

    // Configure & Attach
    machine.configure(1, 128).await.unwrap();
    machine
        .set_boot_source(
            std::path::PathBuf::from("resources/vmlinux"),
            "console=ttyS0".into(),
        )
        .await
        .unwrap();
    machine
        .attach_drive("deps", deps_path, false, true)
        .await
        .unwrap();
    machine
        .attach_drive("code", code_path, false, true)
        .await
        .unwrap();

    // Run
    match machine.start().await {
        Ok(_) => {
            // In a real implementation, we'd capture stdout here
            match machine.wait().await {
                Ok(_) => Json(JobResponse {
                    status: "success".to_string(),
                    message: "Job executed successfully".to_string(),
                    computation_time_ms: start_time.elapsed().as_millis() as u64,
                }),
                Err(e) => Json(JobResponse {
                    status: "crashed".to_string(),
                    message: format!("VM crashed: {:?}", e),
                    computation_time_ms: start_time.elapsed().as_millis() as u64,
                }),
            }
        }
        Err(e) => Json(JobResponse {
            status: "failed_boot".to_string(),
            message: format!("Failed to boot: {:?}", e),
            computation_time_ms: 0,
        }),
    }
}
