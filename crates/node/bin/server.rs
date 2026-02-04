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
//! │    - JobExecutor (mock for testing, real for prod)  │
//! │    - ResultDelivery (result delivery)               │
//! └─────────────────────────────────────────────────────┘
//! ```

use std::sync::Arc;

use anyhow::Result;
use tracing::{error, info};

use monad_node::executor::{MockExecutorBehavior, MockJobExecutor};
use monad_node::p2p::graphene::GrapheneNode;
use monad_node::p2p::messages::WorkerCapabilities;
use monad_node::p2p::protocol::handler::JobProtocolHandler;
use monad_node::p2p::{P2PConfig, P2PNetwork};
use monad_node::result::MockResultDelivery;
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

    // Create P2P configuration
    let storage_path = std::env::var("GRAPHENE_STORAGE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("graphene-worker"));

    let config = P2PConfig {
        storage_path,
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

    // 2. Job executor (mock for e2e testing - replace with DefaultJobExecutor for production)
    // TODO(#141): Replace with real DefaultJobExecutor when Firecracker setup is available
    let executor = Arc::new(MockJobExecutor::new(MockExecutorBehavior::Success {
        exit_code: 0,
        duration: std::time::Duration::from_millis(100),
    }));

    // 3. Result delivery (mock for e2e testing - replace with SyncDelivery for production)
    // TODO(#141): Replace with real SyncDelivery when P2P result streaming is implemented
    let delivery = Arc::new(MockResultDelivery::new());

    // 4. Channel state manager with real ticket validator
    let channel_config = ChannelConfig::default();
    let ticket_validator = Arc::new(DefaultTicketValidator::new());
    let channel_manager = Arc::new(DefaultChannelStateManager::new(
        channel_config,
        ticket_validator.clone(),
    ));

    // Add a test channel for e2e testing
    // TODO(#141): Remove this when real channel registration is implemented
    let test_channel = ChannelLocalState {
        channel_id: [1u8; 32],
        user: [2u8; 32],
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
