//! Graphene worker node binary.
//!
//! This binary runs a Graphene worker that accepts job requests over QUIC,
//! validates payment tickets, and executes jobs in isolated unikernels.
//!
//! # Architecture
//!
//! ```text
//! Incoming QUIC connection (GRAPHENE_JOB_ALPN)
//!          │
//!          ▼
//! ┌─────────────────────────────────────────────────────┐
//! │  JobProtocolHandler<DefaultTicketValidator,         │
//! │                     WorkerJobContext>               │
//! │    - Validates tickets (real Ed25519 signatures)    │
//! │    - Checks capabilities, slots, resources          │
//! └─────────────────────────────────────────────────────┘
//!          │
//!          ▼
//! ┌─────────────────────────────────────────────────────┐
//! │  WorkerJobContext                                   │
//! │    - WorkerStateMachine (slot management)           │
//! │    - DefaultJobExecutor (Firecracker runner)        │
//! │    - SyncDelivery (QUIC result streaming)           │
//! └─────────────────────────────────────────────────────┘
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rand::RngCore;
#[cfg(not(target_os = "linux"))]
use tracing::warn;
use tracing::{error, info};

use monad_node::cache::build::LayeredBuildCache;
use monad_node::cache::iroh::IrohCache;
use monad_node::cache::local::LocalDiskCache;
use monad_node::crypto::DefaultCryptoProvider;
use monad_node::executor::output::DefaultOutputProcessor;

#[cfg(target_os = "linux")]
use monad_node::executor::drive::linux::LinuxDriveBuilder;

#[cfg(not(target_os = "linux"))]
use monad_node::executor::drive::mock::MockDriveBuilder;

#[cfg(target_os = "linux")]
use monad_node::executor::runner::{FirecrackerRunner, FirecrackerRunnerConfig};

#[cfg(not(target_os = "linux"))]
use monad_node::executor::runner::MockRunner;

use monad_node::executor::DefaultJobExecutor;
use monad_node::p2p::graphene::GrapheneNode;
use monad_node::p2p::messages::WorkerCapabilities;
use monad_node::p2p::protocol::handler::JobProtocolHandler;
use monad_node::p2p::{P2PConfig, P2PNetwork};
use monad_node::result::SyncDelivery;
use monad_node::ticket::{
    ChannelConfig, ChannelLocalState, ChannelStateManager, DefaultChannelStateManager,
    DefaultTicketValidator, OnChainChannelState,
};
use monad_node::worker::{WorkerEvent, WorkerJobContext, WorkerStateMachine};

/// Default number of concurrent job slots.
const DEFAULT_SLOTS: u32 = 4;

/// Worker capabilities advertised to clients.
fn default_capabilities() -> WorkerCapabilities {
    WorkerCapabilities {
        max_vcpu: 4,
        max_memory_mb: 4096,
        kernels: vec!["python:3.12".to_string(), "node:20".to_string()],
        disk: None,
        gpus: vec![],
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("monad_node=debug".parse()?)
                .add_directive("server=debug".parse()?),
        )
        .init();

    info!("🚀 Graphene Worker Node Initializing...");

    // Create storage paths
    let base_path = std::env::var("GRAPHENE_STORAGE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("graphene-worker"));

    let p2p_path = base_path.join("p2p");
    let cache_path = base_path.join("cache");
    let drives_path = base_path.join("drives");

    // Ensure directories exist
    std::fs::create_dir_all(&p2p_path)?;
    std::fs::create_dir_all(&cache_path)?;
    std::fs::create_dir_all(&drives_path)?;

    // Generate or load worker secret key
    let worker_secret = load_or_generate_worker_secret(&base_path)?;
    info!("🔑 Worker secret key loaded");

    // Create P2P configuration
    let config = P2PConfig {
        storage_path: p2p_path,
        ..Default::default()
    };

    // Initialize P2P node
    let node = Arc::new(GrapheneNode::new(config).await?);
    let node_id = node.node_id();
    info!("🆔 Worker Node ID: {}", node_id);

    // Print node address for SDK connection
    let node_addr = node.node_addr().await?;
    info!("📍 Node Address: {:?}", node_addr);
    info!("");
    info!("═══════════════════════════════════════════════════════════════");
    info!("  SDK Connection Info:");
    info!("  workerNodeId: \"{}\"", node_id);
    info!("═══════════════════════════════════════════════════════════════");
    info!("");

    // Create worker components
    // 1. State machine for slot management
    let state_machine = WorkerStateMachine::new_shared(DEFAULT_SLOTS);

    // Transition to Online state (simulating successful registration)
    state_machine.transition(WorkerEvent::StakeConfirmed)?;
    state_machine.transition(WorkerEvent::JoinedGossip)?;
    info!(
        "✅ Worker state: {} (slots: {})",
        state_machine.state(),
        state_machine.available_slots()
    );

    // 2. Build the real job executor pipeline
    let crypto = Arc::new(DefaultCryptoProvider);

    // Drive builder for creating ext4 execution images (platform-specific)
    #[cfg(target_os = "linux")]
    let drive_builder = Arc::new(LinuxDriveBuilder::with_defaults());

    #[cfg(not(target_os = "linux"))]
    let drive_builder = {
        warn!("⚠️  Running on non-Linux platform - using mock drive builder");
        warn!("   Production deployment requires Linux for Firecracker VMM");
        Arc::new(MockDriveBuilder::new())
    };

    // VMM runner for VM execution (platform-specific)
    #[cfg(target_os = "linux")]
    let runner = {
        let runner_config = FirecrackerRunnerConfig::new().with_runtime_dir(drives_path.clone());
        Arc::new(FirecrackerRunner::new(runner_config))
    };

    #[cfg(not(target_os = "linux"))]
    let runner = Arc::new(MockRunner::new(
        monad_node::executor::runner::MockRunnerBehavior::default(),
    ));

    // Output processor for encrypting results
    let output_processor = Arc::new(DefaultOutputProcessor::new(crypto.clone()));

    // Build cache for kernel lookups
    let local_cache = LocalDiskCache::new(cache_path.join("local").to_str().unwrap());
    let iroh_cache = IrohCache::new(node.clone(), cache_path.join("iroh"));
    let build_cache = Arc::new(LayeredBuildCache::new(
        local_cache,
        iroh_cache,
        node.clone(),
    ));

    // Create the full job executor
    let executor = Arc::new(DefaultJobExecutor::new(
        drive_builder,
        runner,
        output_processor,
        crypto,
        node.clone(),
        build_cache,
        worker_secret,
    ));
    info!("⚙️  Job executor initialized (Firecracker + real crypto)");

    // 3. Result delivery via QUIC streaming
    let delivery = Arc::new(SyncDelivery::new(node.clone()));
    info!("📤 Result delivery initialized (QUIC streaming)");

    // 4. Channel state manager with real ticket validator
    let channel_config = ChannelConfig::default();
    let ticket_validator = Arc::new(DefaultTicketValidator::new());
    let channel_manager = Arc::new(DefaultChannelStateManager::new(
        channel_config,
        ticket_validator.clone(),
    ));

    // Add a test channel for e2e testing
    // TODO(#141): Remove this when real channel registration is implemented
    let test_user_pubkey: [u8; 32] = match std::env::var("GRAPHENE_TEST_USER_PUBKEY") {
        Ok(hex_str) => {
            let bytes = hex::decode(&hex_str).expect("GRAPHENE_TEST_USER_PUBKEY must be valid hex");
            bytes
                .try_into()
                .expect("GRAPHENE_TEST_USER_PUBKEY must be 64 hex chars (32 bytes)")
        }
        Err(_) => [2u8; 32],
    };

    let test_channel = ChannelLocalState {
        channel_id: [1u8; 32],
        user: test_user_pubkey,
        worker: *node_id.as_bytes(),
        on_chain_balance: 100_000_000, // 100 USDC worth of micros
        accepted_amount: 0,
        last_settled_amount: 0,
        last_nonce: 0,
        last_sync: 0,
        highest_ticket: None,
        on_chain_state: OnChainChannelState::Open,
        dispute_timeout: 0,
    };
    channel_manager.upsert_channel(test_channel).await?;
    info!("📝 Test channel registered for e2e testing");

    // 5. Create WorkerJobContext combining all components
    let worker_pubkey: [u8; 32] = *node_id.as_bytes();
    let context = Arc::new(WorkerJobContext::new(
        state_machine.clone(),
        executor,
        delivery,
        channel_manager,
        default_capabilities(),
        worker_pubkey,
    ));

    // 6. Create JobProtocolHandler with ticket validator and context
    let handler = Arc::new(JobProtocolHandler::new(ticket_validator, context));

    info!("🎯 Worker ready to accept jobs");
    info!("   Supported kernels: python:3.12, node:20");
    info!("   Max vCPU: 4, Max Memory: 4096 MB");
    info!("   Available slots: {}", state_machine.available_slots());

    // Accept incoming connections
    info!("👂 Listening for job requests...");

    // Clone node for shutdown (accept_loop takes ownership of Arc)
    let node_for_shutdown = node.clone();

    // Use the GrapheneNode's accept loop
    let handler_clone = handler.clone();
    node.accept_loop(Arc::new(move |conn, _node| {
        let handler = handler_clone.clone();
        async move {
            match handler.handle_connection(conn).await {
                Ok(()) => {
                    info!("✅ Job request handled successfully");
                    Ok(())
                }
                Err(e) => {
                    error!("❌ Job request failed: {}", e);
                    // Convert protocol error to P2P error
                    Err(monad_node::p2p::P2PError::ConnectionError(e.to_string()))
                }
            }
        }
    }))
    .await;

    info!("👋 Worker shutting down");
    node_for_shutdown.shutdown().await?;

    Ok(())
}

/// Load worker secret key from disk, or generate a new one.
fn load_or_generate_worker_secret(base_path: &std::path::Path) -> Result<[u8; 32]> {
    let secret_path = base_path.join("worker_secret.key");

    if secret_path.exists() {
        // Load existing key
        let bytes = std::fs::read(&secret_path)?;
        if bytes.len() != 32 {
            anyhow::bail!(
                "Invalid worker secret key length: expected 32, got {}",
                bytes.len()
            );
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(key)
    } else {
        // Generate new key
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        std::fs::write(&secret_path, key)?;
        info!("🔐 Generated new worker secret key at {:?}", secret_path);
        Ok(key)
    }
}
