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
use std::str::FromStr;
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::{fs::OpenOptions, process::Command};

use anyhow::Result;
#[cfg(target_os = "linux")]
use async_trait::async_trait;
use tracing::{error, info, warn};

use graphene_node::cache::build::LayeredBuildCache;
use graphene_node::cache::iroh::IrohCache;
use graphene_node::cache::local::LocalDiskCache;
use graphene_node::crypto::DefaultCryptoProvider;
use graphene_node::executor::output::DefaultOutputProcessor;

#[cfg(target_os = "linux")]
use graphene_node::executor::drive::linux::LinuxDriveBuilder;

#[cfg(not(target_os = "linux"))]
use graphene_node::executor::drive::mock::MockDriveBuilder;

#[cfg(target_os = "linux")]
use graphene_node::executor::runner::VmmRunner;

#[cfg(target_os = "linux")]
use graphene_node::executor::runner::{FirecrackerRunner, FirecrackerRunnerConfig};

use graphene_node::executor::runner::MockRunner;

use graphene_node::executor::{DefaultJobExecutor, ExecutorConfig};
use graphene_node::p2p::graphene::GrapheneNode;
use graphene_node::p2p::messages::WorkerCapabilities;
use graphene_node::p2p::protocol::handler::JobProtocolHandler;
use graphene_node::p2p::{P2PConfig, P2PNetwork};
use graphene_node::result::SyncDelivery;
use graphene_node::ticket::{
    ChannelConfig, ChannelLocalState, ChannelStateManager, DefaultChannelStateManager,
    DefaultSolanaChannelClient, DefaultTicketValidator, OnChainChannelState, SolanaChannelClient,
};
use graphene_node::worker::{WorkerEvent, WorkerJobContext, WorkerStateMachine};

use solana_sdk::pubkey::Pubkey;

/// Default number of concurrent job slots.
const DEFAULT_SLOTS: u32 = 4;
/// Default Solana program ID for Graphene.
const DEFAULT_SOLANA_PROGRAM_ID: &str = "3yErVeGSU3LHZzTnKjkoV5fPkcFQxyjeroLRo5VtSvEf";

/// Static kernel catalog (runtime, versions, entrypoint).
struct KernelConfig {
    runtime: &'static str,
    versions: &'static [&'static str],
}

const SUPPORTED_KERNELS: &[KernelConfig] = &[
    KernelConfig {
        runtime: "python",
        versions: &["3.12"],
    },
    KernelConfig {
        runtime: "node",
        versions: &["21"],
    },
];

#[cfg(target_os = "linux")]
fn env_truthy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn firecracker_available() -> bool {
    Command::new("firecracker")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn kvm_available() -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .is_ok()
}

#[cfg(target_os = "linux")]
fn should_use_mock_runner() -> bool {
    if env_truthy("GRAPHENE_FORCE_MOCK_RUNNER") {
        warn!("GRAPHENE_FORCE_MOCK_RUNNER set; using MockRunner");
        return true;
    }

    if !firecracker_available() {
        warn!("Firecracker binary not available; using MockRunner");
        return true;
    }

    if !kvm_available() {
        warn!("KVM not accessible (/dev/kvm); using MockRunner");
        return true;
    }

    false
}

#[cfg(target_os = "linux")]
enum RunnerKind {
    Firecracker(FirecrackerRunner),
    Mock(MockRunner),
}

#[cfg(target_os = "linux")]
#[async_trait]
impl VmmRunner for RunnerKind {
    async fn run(
        &self,
        kernel_path: &std::path::Path,
        drive_path: &std::path::Path,
        manifest: &graphene_node::p2p::messages::JobManifest,
        boot_args: &str,
    ) -> Result<
        graphene_node::executor::runner::VmmOutput,
        graphene_node::executor::runner::RunnerError,
    > {
        match self {
            RunnerKind::Firecracker(runner) => {
                runner
                    .run(kernel_path, drive_path, manifest, boot_args)
                    .await
            }
            RunnerKind::Mock(runner) => {
                runner
                    .run(kernel_path, drive_path, manifest, boot_args)
                    .await
            }
        }
    }
}

fn load_capabilities_from_catalog() -> WorkerCapabilities {
    let kernels: Vec<String> = SUPPORTED_KERNELS
        .iter()
        .flat_map(|k| {
            k.versions
                .iter()
                .map(move |v| format!("{}:{}", k.runtime, v))
        })
        .collect();

    WorkerCapabilities {
        max_vcpu: 4,
        max_memory_mb: 4096,
        kernels,
        disk: None,
        gpus: vec![],
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    // Note: Binary logs use "graphene_worker" as the target (binary name with underscores)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("graphene_node=debug".parse()?)
                .add_directive("graphene_worker=debug".parse()?),
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

    // Create P2P configuration
    let bind_addr = std::env::var("GRAPHENE_P2P_BIND_ADDR")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let bind_port = std::env::var("GRAPHENE_P2P_BIND_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);

    if bind_addr.is_some() || bind_port != 0 {
        info!(
            "🔌 P2P bind override: addr={:?}, port={}",
            bind_addr, bind_port
        );
    }

    let config = P2PConfig {
        storage_path: p2p_path,
        bind_addr,
        bind_port,
        ..Default::default()
    };

    // Initialize P2P node
    let node = Arc::new(GrapheneNode::new(config).await?);
    let node_id = node.node_id();
    info!("🆔 Worker Node ID: {}", node_id);

    // Use the same long-term identity for payment-channel crypto. This fixes
    // a mismatch where job encryption/decryption derived channel keys from a
    // randomly generated worker secret that was *different* from the node's
    // identity key. The SDK uses workerNodeId (derived from the node identity)
    // to derive channel keys, so the worker must use the same secret to avoid
    // authentication tag mismatches.
    let worker_secret: [u8; 32] = node.secret_key_bytes();

    // Print node address for SDK connection
    let node_addr = node.node_addr().await?;
    info!("📍 Node Address: {:?}", node_addr);
    info!("");
    info!("═══════════════════════════════════════════════════════════════");
    info!("  SDK Connection Info:");
    info!("  workerNodeId: \"{}\"", node_id);
    // Add relay URL for client connection (needed by Iroh 0.96 for NAT traversal)
    if let Some(relay_url) = node_addr.relay_urls().next() {
        info!("  relayUrl: \"{}\"", relay_url);
    }
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
    let runner = if should_use_mock_runner() {
        Arc::new(RunnerKind::Mock(MockRunner::new(
            graphene_node::executor::runner::MockRunnerBehavior::default(),
        )))
    } else {
        let runner_config = FirecrackerRunnerConfig::new().with_runtime_dir(drives_path.clone());
        Arc::new(RunnerKind::Firecracker(FirecrackerRunner::new(
            runner_config,
        )))
    };

    #[cfg(not(target_os = "linux"))]
    let runner = Arc::new(MockRunner::new(
        graphene_node::executor::runner::MockRunnerBehavior::default(),
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

    let cleanup_drives = !env_truthy("GRAPHENE_KEEP_DRIVES");
    if !cleanup_drives {
        info!("🧪 GRAPHENE_KEEP_DRIVES enabled; execution drives will be preserved");
    }

    // Create the full job executor
    let executor = Arc::new(DefaultJobExecutor::with_config(
        drive_builder,
        runner,
        output_processor,
        crypto,
        node.clone(),
        build_cache,
        worker_secret,
        ExecutorConfig {
            cleanup_drives,
            max_concurrent_jobs: 0,
        },
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

    // Optional Solana client for on-demand channel sync (enabled via env)
    let solana_client: Option<Arc<dyn SolanaChannelClient>> =
        std::env::var("GRAPHENE_SOLANA_RPC_URL")
            .ok()
            .and_then(|rpc_url| {
                let ws_url =
                    std::env::var("GRAPHENE_SOLANA_WS_URL").unwrap_or_else(|_| rpc_url.clone());
                let program_id = std::env::var("GRAPHENE_SOLANA_PROGRAM_ID")
                    .unwrap_or_else(|_| DEFAULT_SOLANA_PROGRAM_ID.to_string());
                match Pubkey::from_str(&program_id) {
                    Ok(pubkey) => Some(Arc::new(DefaultSolanaChannelClient::new(
                        rpc_url,
                        ws_url,
                        pubkey.to_bytes(),
                    )) as Arc<dyn SolanaChannelClient>),
                    Err(_) => {
                        warn!("Invalid GRAPHENE_SOLANA_PROGRAM_ID; skipping Solana client");
                        None
                    }
                }
            });

    if solana_client.is_some() {
        info!("🔗 Solana channel sync enabled (on-demand)");
    }

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
    let capabilities = load_capabilities_from_catalog();

    let context = Arc::new(WorkerJobContext::new(
        state_machine.clone(),
        executor,
        delivery,
        channel_manager,
        solana_client.clone(),
        capabilities.clone(),
        worker_pubkey,
    ));

    // 6. Create JobProtocolHandler with ticket validator and context
    let handler = Arc::new(JobProtocolHandler::new(ticket_validator, context));

    info!("🎯 Worker ready to accept jobs");
    info!("   Supported kernels: {}", capabilities.kernels.join(", "));
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
                    Err(graphene_node::p2p::P2PError::ConnectionError(e.to_string()))
                }
            }
        }
    }))
    .await;

    info!("👋 Worker shutting down");
    node_for_shutdown.shutdown().await?;

    Ok(())
}
