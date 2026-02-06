//! Real worker discovery implementation using Iroh gossip.

use super::types::{DiscoveryConfig, JobRequirements, WorkerInfo, WorkerStatus};
use super::DiscoveryError;
use super::WorkerDiscovery;
use crate::p2p::messages::{
    ComputeMessage, GossipWorkerState, WorkerAnnouncement, WorkerHeartbeat, WorkerLoad,
};
use crate::p2p::{P2PNetwork, TopicId};
use async_trait::async_trait;
use iroh::PublicKey;
use iroh_gossip::api::Event;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Worker discovery service using Iroh gossip.
pub struct IrohWorkerDiscovery<N: P2PNetwork> {
    /// The P2P network to use for gossip.
    network: Arc<N>,

    /// Configuration for the discovery service.
    config: DiscoveryConfig,

    /// Our announcement to broadcast.
    our_announcement: Arc<RwLock<Option<WorkerAnnouncement>>>,

    /// Known workers discovered via gossip.
    known_workers: Arc<RwLock<HashMap<PublicKey, WorkerInfo>>>,

    /// Whether the service is running.
    running: Arc<AtomicBool>,

    /// Background tasks.
    tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
}

impl<N: P2PNetwork + 'static> IrohWorkerDiscovery<N> {
    /// Create a new discovery service.
    pub fn new(network: Arc<N>, config: DiscoveryConfig) -> Self {
        Self {
            network,
            config,
            our_announcement: Arc::new(RwLock::new(None)),
            known_workers: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
            tasks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Set the announcement to broadcast.
    pub async fn set_announcement(&self, announcement: WorkerAnnouncement) {
        *self.our_announcement.write().await = Some(announcement);
    }

    /// Get current Unix timestamp in seconds.
    fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Spawn the announcer task that periodically broadcasts our announcement.
    async fn spawn_announcer(&self) {
        let network = Arc::clone(&self.network);
        let running = Arc::clone(&self.running);
        let announcement = Arc::clone(&self.our_announcement);
        let interval = self.config.announce_interval;

        let handle = tokio::spawn(async move {
            let topic = TopicId::compute_v1();

            while running.load(Ordering::Relaxed) {
                // Get and update timestamp
                if let Some(mut ann) = announcement.write().await.clone() {
                    ann.timestamp = Self::now_unix();

                    let msg = ComputeMessage::Announcement(ann);
                    if let Ok(bytes) = serde_json::to_vec(&msg) {
                        let _ = network.broadcast(topic, &bytes).await;
                    }
                }

                tokio::time::sleep(interval).await;
            }
        });

        self.tasks.write().await.push(handle);
    }

    /// Spawn the listener task that processes incoming gossip.
    async fn spawn_listener(&self) -> Result<(), DiscoveryError> {
        let network = Arc::clone(&self.network);
        let running = Arc::clone(&self.running);
        let known_workers = Arc::clone(&self.known_workers);
        let our_node_id = network.node_id();

        let topic = TopicId::compute_v1();
        let mut subscription = network.subscribe(topic).await?;

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                match subscription.recv().await {
                    Some(Event::Received(gossip_msg)) => {
                        if let Ok(msg) =
                            serde_json::from_slice::<ComputeMessage>(&gossip_msg.content)
                        {
                            match msg {
                                ComputeMessage::Announcement(ann) => {
                                    // Don't add ourselves
                                    if ann.node_id == our_node_id {
                                        continue;
                                    }
                                    Self::handle_announcement(&known_workers, ann).await;
                                }
                                ComputeMessage::Heartbeat(hb) => {
                                    if hb.node_id == our_node_id {
                                        continue;
                                    }
                                    Self::handle_heartbeat(&known_workers, hb).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    None => break,
                    _ => {}
                }
            }
        });

        self.tasks.write().await.push(handle);
        Ok(())
    }

    /// Spawn the expiry checker task.
    async fn spawn_expiry_checker(&self) {
        let running = Arc::clone(&self.running);
        let known_workers = Arc::clone(&self.known_workers);
        let offline_threshold = self.config.offline_threshold;
        let expiry_threshold = self.config.expiry_threshold;

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                let now = Instant::now();

                let mut workers = known_workers.write().await;
                workers.retain(|_, worker| {
                    let elapsed = now.duration_since(worker.last_seen);

                    // Remove if expired
                    if elapsed > expiry_threshold {
                        return false;
                    }

                    // Mark offline if threshold exceeded
                    if elapsed > offline_threshold {
                        worker.status = WorkerStatus::Offline;
                    }

                    true
                });

                // Check at a reasonable interval (1/4 of offline threshold)
                let check_interval = offline_threshold / 4;
                tokio::time::sleep(check_interval).await;
            }
        });

        self.tasks.write().await.push(handle);
    }

    /// Handle an incoming worker announcement.
    async fn handle_announcement(
        known_workers: &Arc<RwLock<HashMap<PublicKey, WorkerInfo>>>,
        ann: WorkerAnnouncement,
    ) {
        let info = WorkerInfo {
            node_id: ann.node_id,
            addr: None, // TODO: Could be populated from gossip source
            version: ann.version,
            capabilities: ann.capabilities,
            pricing: ann.pricing,
            load: ann.load,
            status: WorkerStatus::Online,
            last_seen: Instant::now(),
            regions: ann.regions,
            reputation: ann.reputation,
        };

        known_workers.write().await.insert(ann.node_id, info);
    }

    /// Handle an incoming heartbeat.
    async fn handle_heartbeat(
        known_workers: &Arc<RwLock<HashMap<PublicKey, WorkerInfo>>>,
        hb: WorkerHeartbeat,
    ) {
        let mut workers = known_workers.write().await;
        if let Some(worker) = workers.get_mut(&hb.node_id) {
            worker.load = hb.load;
            worker.status = WorkerStatus::Online;
            worker.last_seen = Instant::now();
        }
        // If we don't know this worker, ignore the heartbeat
        // They should send an announcement first
    }

    /// Broadcast a shutdown announcement with zero slots.
    async fn broadcast_shutdown(&self) -> Result<(), DiscoveryError> {
        if let Some(mut ann) = self.our_announcement.read().await.clone() {
            ann.load = WorkerLoad {
                available_slots: 0,
                queue_depth: 0,
            };
            ann.timestamp = Self::now_unix();

            let msg = ComputeMessage::Heartbeat(WorkerHeartbeat {
                node_id: ann.node_id,
                load: ann.load,
                state: GossipWorkerState::Draining,
                timestamp: ann.timestamp,
            });

            if let Ok(bytes) = serde_json::to_vec(&msg) {
                let topic = TopicId::compute_v1();
                self.network.broadcast(topic, &bytes).await?;
            }
        }
        Ok(())
    }

    /// Inject a worker for testing (package-private).
    #[cfg(test)]
    pub async fn inject_worker_for_test(&self, worker: WorkerInfo) {
        self.known_workers
            .write()
            .await
            .insert(worker.node_id, worker);
    }
}

#[async_trait]
impl<N: P2PNetwork + 'static> WorkerDiscovery for IrohWorkerDiscovery<N> {
    async fn start(&self) -> Result<(), DiscoveryError> {
        if self.running.swap(true, Ordering::Relaxed) {
            return Err(DiscoveryError::AlreadyRunning);
        }

        // Start background tasks
        self.spawn_listener().await?;
        self.spawn_announcer().await;
        self.spawn_expiry_checker().await;

        Ok(())
    }

    async fn stop(&self) -> Result<(), DiscoveryError> {
        if !self.running.swap(false, Ordering::Relaxed) {
            return Err(DiscoveryError::NotRunning);
        }

        // Broadcast shutdown
        let _ = self.broadcast_shutdown().await;

        // Cancel all tasks
        let tasks = std::mem::take(&mut *self.tasks.write().await);
        for task in tasks {
            task.abort();
        }

        Ok(())
    }

    async fn find_workers(&self, requirements: &JobRequirements) -> Vec<WorkerInfo> {
        self.known_workers
            .read()
            .await
            .values()
            .filter(|w| w.meets_requirements(requirements))
            .cloned()
            .collect()
    }

    async fn list_workers(&self) -> Vec<WorkerInfo> {
        self.known_workers.read().await.values().cloned().collect()
    }

    async fn update_load(&self, load: WorkerLoad) -> Result<(), DiscoveryError> {
        if !self.running.load(Ordering::Relaxed) {
            return Err(DiscoveryError::NotRunning);
        }

        // Update our announcement
        {
            let mut ann = self.our_announcement.write().await;
            if let Some(ref mut a) = *ann {
                a.load = load;
            }
        }

        // Broadcast heartbeat
        if let Some(ann) = self.our_announcement.read().await.as_ref() {
            let hb = WorkerHeartbeat {
                node_id: ann.node_id,
                load,
                state: ann.state,
                timestamp: Self::now_unix(),
            };

            let msg = ComputeMessage::Heartbeat(hb);
            if let Ok(bytes) = serde_json::to_vec(&msg) {
                let topic = TopicId::compute_v1();
                self.network.broadcast(topic, &bytes).await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::messages::{WorkerCapabilities, WorkerPricing, WorkerReputation};
    use crate::p2p::MockGrapheneNode;
    use rand::RngCore;
    use std::time::Duration;

    fn make_test_node_id() -> iroh::PublicKey {
        let mut key_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key_bytes);
        iroh::SecretKey::from_bytes(&key_bytes).public()
    }

    fn make_test_announcement(node: &MockGrapheneNode) -> WorkerAnnouncement {
        WorkerAnnouncement {
            node_id: node.node_id(),
            version: "0.1.0".to_string(),
            capabilities: WorkerCapabilities {
                max_vcpu: 8,
                max_memory_mb: 16384,
                kernels: vec!["node-20-unikraft".to_string()],
                disk: None,
                gpus: Vec::new(),
            },
            pricing: WorkerPricing::default(),
            load: WorkerLoad {
                available_slots: 4,
                queue_depth: 0,
            },
            state: GossipWorkerState::Online,
            timestamp: IrohWorkerDiscovery::<MockGrapheneNode>::now_unix(),
            regions: Vec::new(),
            reputation: WorkerReputation::default(),
        }
    }

    #[tokio::test]
    async fn test_discovery_start_stop() {
        let node = MockGrapheneNode::new();
        let discovery = IrohWorkerDiscovery::new(Arc::new(node), DiscoveryConfig::for_testing());

        discovery.start().await.unwrap();
        assert!(discovery.running.load(Ordering::Relaxed));

        discovery.stop().await.unwrap();
        assert!(!discovery.running.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_discovery_double_start() {
        let node = MockGrapheneNode::new();
        let discovery = IrohWorkerDiscovery::new(Arc::new(node), DiscoveryConfig::for_testing());

        discovery.start().await.unwrap();
        let result = discovery.start().await;
        assert!(matches!(result, Err(DiscoveryError::AlreadyRunning)));

        discovery.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_discovery_stop_without_start() {
        let node = MockGrapheneNode::new();
        let discovery = IrohWorkerDiscovery::new(Arc::new(node), DiscoveryConfig::for_testing());

        let result = discovery.stop().await;
        assert!(matches!(result, Err(DiscoveryError::NotRunning)));
    }

    #[tokio::test]
    async fn test_discovery_broadcasts_announcement() {
        let node = MockGrapheneNode::new();
        let node_arc = Arc::new(node);
        let discovery =
            IrohWorkerDiscovery::new(Arc::clone(&node_arc), DiscoveryConfig::for_testing());

        let ann = make_test_announcement(&node_arc);
        discovery.set_announcement(ann).await;

        discovery.start().await.unwrap();

        // Wait for at least one broadcast cycle with yield points
        for _ in 0..10 {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(20)).await;

            let count = node_arc.spy().broadcast_messages.len();
            if count > 0 {
                break;
            }
        }

        let count = node_arc.spy().broadcast_messages.len();
        assert!(count > 0, "Expected at least one broadcast, got {}", count);

        discovery.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_discovery_update_load() {
        let node = MockGrapheneNode::new();
        let node_arc = Arc::new(node);
        let discovery =
            IrohWorkerDiscovery::new(Arc::clone(&node_arc), DiscoveryConfig::for_testing());

        let ann = make_test_announcement(&node_arc);
        discovery.set_announcement(ann).await;

        discovery.start().await.unwrap();

        let new_load = WorkerLoad {
            available_slots: 2,
            queue_depth: 3,
        };
        discovery.update_load(new_load).await.unwrap();

        // Verify load was updated in our announcement
        let our_ann = discovery.our_announcement.read().await;
        assert_eq!(our_ann.as_ref().unwrap().load.available_slots, 2);
        assert_eq!(our_ann.as_ref().unwrap().load.queue_depth, 3);

        discovery.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_discovery_find_workers() {
        let node = MockGrapheneNode::new();
        let discovery = IrohWorkerDiscovery::new(Arc::new(node), DiscoveryConfig::for_testing());

        // Inject a test worker
        let test_worker = WorkerInfo {
            node_id: make_test_node_id(),
            addr: None,
            version: "0.1.0".to_string(),
            capabilities: WorkerCapabilities {
                max_vcpu: 8,
                max_memory_mb: 16384,
                kernels: vec!["node-20-unikraft".to_string()],
                disk: None,
                gpus: Vec::new(),
            },
            pricing: WorkerPricing::default(),
            load: WorkerLoad {
                available_slots: 4,
                queue_depth: 0,
            },
            status: WorkerStatus::Online,
            last_seen: Instant::now(),
            regions: Vec::new(),
            reputation: WorkerReputation::default(),
        };

        discovery.inject_worker_for_test(test_worker).await;

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            runtime: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
            ..Default::default()
        };

        let found = discovery.find_workers(&requirements).await;
        assert_eq!(found.len(), 1);
    }

    #[tokio::test]
    async fn test_discovery_worker_expiry() {
        let node = MockGrapheneNode::new();
        let config = DiscoveryConfig {
            offline_threshold: Duration::from_millis(50),
            expiry_threshold: Duration::from_millis(150),
            ..DiscoveryConfig::for_testing()
        };
        let discovery = IrohWorkerDiscovery::new(Arc::new(node), config);

        // Inject a worker with old last_seen
        let test_worker = WorkerInfo {
            node_id: make_test_node_id(),
            addr: None,
            version: "0.1.0".to_string(),
            capabilities: WorkerCapabilities::default(),
            pricing: WorkerPricing::default(),
            load: WorkerLoad::default(),
            status: WorkerStatus::Online,
            last_seen: Instant::now(),
            regions: Vec::new(),
            reputation: WorkerReputation::default(),
        };

        discovery.inject_worker_for_test(test_worker).await;

        discovery.start().await.unwrap();

        // Initially online
        let workers = discovery.list_workers().await;
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].status, WorkerStatus::Online);

        // Wait for offline threshold
        tokio::time::sleep(Duration::from_millis(75)).await;

        // Should be marked offline
        let workers = discovery.list_workers().await;
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].status, WorkerStatus::Offline);

        // Wait for expiry threshold
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should be removed
        let workers = discovery.list_workers().await;
        assert_eq!(workers.len(), 0);

        discovery.stop().await.unwrap();
    }
}
