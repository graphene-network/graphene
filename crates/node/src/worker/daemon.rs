//! Daemon implementation for the worker binary.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::p2p::messages::{ComputeMessage, WorkerAnnouncement, WorkerHeartbeat};
use crate::p2p::types::TopicId;
use crate::p2p::{GrapheneNode, P2PNetwork};

use super::config::WorkerConfig;
use super::solana::SolanaClient;
use super::WorkerError;

/// Run the worker daemon.
///
/// This initializes the P2P network, subscribes to topics, and runs the main event loop.
pub async fn run_daemon(config: WorkerConfig, foreground: bool) -> Result<(), WorkerError> {
    info!(
        "Starting Graphene worker '{}' (foreground={})",
        config.worker.name, foreground
    );

    // Initialize P2P node
    let p2p_config = config.to_p2p_config();
    let node = Arc::new(GrapheneNode::new(p2p_config).await?);

    info!("P2P node initialized with ID: {}", node.node_id());

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Spawn signal handler
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = shutdown_signal().await {
            error!("Signal handler error: {}", e);
        }
        let _ = shutdown_tx_clone.send(true);
    });

    // Subscribe to compute topic
    let mut compute_sub = node.subscribe(TopicId::compute_v1()).await?;
    info!("Subscribed to compute topic");

    // Broadcast initial announcement
    let announcement = create_announcement(&config, &node);
    let msg = ComputeMessage::Announcement(announcement);
    let encoded = serde_json::to_vec(&msg).map_err(|e| {
        WorkerError::P2PError(crate::p2p::P2PError::GossipError(format!(
            "Failed to encode announcement: {}",
            e
        )))
    })?;
    compute_sub.broadcast(encoded).await.map_err(|e| {
        WorkerError::P2PError(crate::p2p::P2PError::GossipError(format!(
            "Failed to broadcast: {}",
            e
        )))
    })?;
    info!("Broadcast initial worker announcement");

    // Spawn heartbeat task
    let node_clone = node.clone();
    let config_clone = config.clone();
    let mut heartbeat_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        heartbeat_loop(node_clone, config_clone, &mut heartbeat_shutdown).await;
    });

    // Main event loop
    let mut shutdown_rx = shutdown_rx;
    loop {
        tokio::select! {
            // Check for shutdown signal
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("Shutdown signal received");
                    break;
                }
            }

            // Handle incoming gossip events
            event = compute_sub.recv() => {
                match event {
                    Some(gossip_event) => {
                        handle_gossip_event(gossip_event, &node, &config).await;
                    }
                    None => {
                        warn!("Gossip subscription closed");
                        break;
                    }
                }
            }
        }
    }

    // Graceful shutdown
    info!("Shutting down worker...");
    node.shutdown().await?;
    info!("Worker shutdown complete");

    Ok(())
}

/// Wait for shutdown signal (SIGINT or SIGTERM).
async fn shutdown_signal() -> Result<(), WorkerError> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;

        tokio::select! {
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        info!("Received Ctrl+C");
    }

    Ok(())
}

/// Create a worker announcement from config.
fn create_announcement(config: &WorkerConfig, node: &Arc<GrapheneNode>) -> WorkerAnnouncement {
    use crate::p2p::messages::{WorkerCapabilities, WorkerLoad, WorkerPricing};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    WorkerAnnouncement {
        node_id: node.node_id(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: WorkerCapabilities {
            max_vcpu: 4,         // TODO: Detect or configure
            max_memory_mb: 4096, // TODO: Detect or configure
            kernels: config.worker.capabilities.clone(),
        },
        pricing: WorkerPricing {
            cpu_ms_micros: config.worker.price_per_unit,
            memory_mb_ms_micros: 0.0, // TODO: Add to config
        },
        load: WorkerLoad {
            available_slots: config.worker.job_slots as u8,
            queue_depth: 0,
        },
        timestamp,
    }
}

/// Heartbeat loop that broadcasts periodic heartbeats.
async fn heartbeat_loop(
    node: Arc<GrapheneNode>,
    _config: WorkerConfig,
    shutdown: &mut watch::Receiver<bool>,
) {
    let interval = Duration::from_secs(30);

    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                use crate::p2p::messages::WorkerLoad;

                let heartbeat = WorkerHeartbeat {
                    node_id: node.node_id(),
                    load: WorkerLoad {
                        available_slots: 4, // TODO: Track actual available slots
                        queue_depth: 0,     // TODO: Track job queue depth
                    },
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };

                let msg = ComputeMessage::Heartbeat(heartbeat);
                if let Ok(encoded) = serde_json::to_vec(&msg) {
                    if let Err(e) = node.broadcast(TopicId::compute_v1(), &encoded).await {
                        warn!("Failed to broadcast heartbeat: {}", e);
                    }
                }
            }

            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!("Heartbeat loop shutting down");
                    break;
                }
            }
        }
    }
}

/// Handle incoming gossip events.
async fn handle_gossip_event(
    event: iroh_gossip::api::Event,
    _node: &Arc<GrapheneNode>,
    _config: &WorkerConfig,
) {
    use iroh_gossip::api::Event;

    match event {
        Event::Received(msg) => {
            // Try to decode the message
            if let Ok(compute_msg) = serde_json::from_slice::<ComputeMessage>(&msg.content) {
                match compute_msg {
                    ComputeMessage::Announcement(ann) => {
                        info!("Received announcement from peer: {}", ann.node_id);
                    }
                    ComputeMessage::Heartbeat(hb) => {
                        info!(
                            "Received heartbeat from peer: {} (slots={}, queue={})",
                            hb.node_id, hb.load.available_slots, hb.load.queue_depth
                        );
                    }
                    ComputeMessage::DiscoveryQuery(query) => {
                        info!("Received discovery query: {}", query.query_id);
                        // TODO: Respond if we match the criteria
                    }
                    ComputeMessage::DiscoveryResponse(resp) => {
                        info!("Received discovery response for: {}", resp.query_id);
                    }
                }
            }
        }
        Event::NeighborUp(peer) => {
            info!("Neighbor up: {}", peer);
        }
        Event::NeighborDown(peer) => {
            info!("Neighbor down: {}", peer);
        }
        _ => {
            // Handle other events (Lagged, etc.)
        }
    }
}

/// Register worker on Solana.
pub async fn register_worker(
    config: &WorkerConfig,
    stake_sol: f64,
    confirm: bool,
) -> Result<(), WorkerError> {
    let client = SolanaClient::new(&config.solana)?;

    // Convert SOL to lamports
    let stake_lamports = (stake_sol * 1_000_000_000.0) as u64;

    info!(
        "Registering worker with {} SOL stake ({} lamports)",
        stake_sol, stake_lamports
    );

    if !confirm {
        println!("Worker: {}", config.worker.name);
        println!("Authority: {}", client.authority());
        println!("Stake: {} SOL", stake_sol);
        println!("\nAdd --yes to confirm registration");
        return Ok(());
    }

    let sig = client.register_worker(stake_lamports).await?;
    info!("Worker registered! Transaction: {}", sig);
    println!("Worker registered successfully!");
    println!("Transaction: {}", sig);

    Ok(())
}

/// Unregister worker from Solana.
pub async fn unregister_worker(config: &WorkerConfig, confirm: bool) -> Result<(), WorkerError> {
    let client = SolanaClient::new(&config.solana)?;

    info!("Unregistering worker");

    if !confirm {
        println!("Worker: {}", config.worker.name);
        println!("Authority: {}", client.authority());
        println!("\nAdd --yes to confirm unregistration");
        return Ok(());
    }

    let sig = client.unregister_worker().await?;
    info!("Worker unregistered! Transaction: {}", sig);
    println!("Worker unregistered successfully!");
    println!("Transaction: {}", sig);

    Ok(())
}

/// Display worker status.
pub async fn show_status(config: &WorkerConfig, format: &str) -> Result<(), WorkerError> {
    let client = SolanaClient::new(&config.solana)?;

    let status = client.get_worker_status().await?;

    match format {
        "json" => {
            let output = if let Some(ref s) = status {
                serde_json::json!({
                    "registered": true,
                    "authority": s.authority.to_string(),
                    "stake_lamports": s.stake,
                    "stake_sol": s.stake as f64 / 1_000_000_000.0,
                    "is_active": s.is_active,
                    "registered_at": s.registered_at,
                })
            } else {
                serde_json::json!({
                    "registered": false,
                })
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        _ => {
            println!("Worker: {}", config.worker.name);
            println!("Authority: {}", client.authority());
            println!();

            if let Some(s) = status {
                println!("Status: Registered");
                println!("Active: {}", if s.is_active { "Yes" } else { "No" });
                println!(
                    "Stake: {} SOL ({} lamports)",
                    s.stake as f64 / 1_000_000_000.0,
                    s.stake
                );
                println!("Registered at: {}", s.registered_at);
            } else {
                println!("Status: Not registered");
            }
        }
    }

    Ok(())
}
