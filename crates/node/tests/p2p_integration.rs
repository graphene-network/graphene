//! P2P integration tests using mock implementations.
//!
//! These tests verify the P2P networking abstraction works correctly
//! without requiring actual network connections.

use monad_node::p2p::{
    mock::{MockBehavior, MockGrapheneNode, MockNetwork},
    P2PNetwork,
};

#[tokio::test]
async fn test_mock_blob_roundtrip() {
    let node = MockGrapheneNode::new();

    // Upload a blob
    let data = b"Hello, Graphene Network!";
    let hash = node.upload_blob(data).await.unwrap();

    // Verify we can check it exists
    assert!(node.has_blob(hash).await.unwrap());

    // Download and verify content
    let downloaded = node.download_blob(hash, None).await.unwrap();
    assert_eq!(downloaded, data);

    // Verify spy state recorded the operations
    let spy = node.spy();
    assert_eq!(spy.uploaded_blobs.len(), 1);
    assert_eq!(spy.download_attempts.len(), 1);
}

#[tokio::test]
async fn test_mock_network_blob_sharing() {
    // Create a shared network
    let network = MockNetwork::new();

    // Create two nodes on the same network
    let node1 = MockGrapheneNode::with_network(network.clone());
    let node2 = MockGrapheneNode::with_network(network);

    // Upload from node1
    let data = b"Shared blob data";
    let hash = node1.upload_blob(data).await.unwrap();

    // Download from node2 (should find it via shared network)
    let downloaded = node2.download_blob(hash, None).await.unwrap();
    assert_eq!(downloaded, data);

    // Verify node2 can also check the blob exists
    assert!(node2.has_blob(hash).await.unwrap());
}

#[tokio::test]
async fn test_mock_blob_not_found() {
    let node = MockGrapheneNode::new();

    // Try to download a non-existent blob
    let fake_hash = iroh_blobs::Hash::new(b"nonexistent");
    let result = node.download_blob(fake_hash, None).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_behavior_blob_failure() {
    let node = MockGrapheneNode::with_behavior(MockBehavior::BlobDownloadFailure);

    // Upload should still work
    let data = b"test data";
    let hash = node.upload_blob(data).await.unwrap();

    // But download should fail
    let result = node.download_blob(hash, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_gossip_subscription() {
    let node = MockGrapheneNode::new();

    // Create a topic
    let topic = monad_node::p2p::TopicId::from_name("test-topic-1");

    // Subscribe to the topic
    let subscription = node.subscribe(topic).await.unwrap();

    // Verify subscription was recorded
    assert_eq!(node.spy().subscribed_topics.len(), 1);
    assert_eq!(node.spy().subscribed_topics[0], topic);

    // Verify subscription has the correct topic
    assert_eq!(subscription.topic, topic);
}

#[tokio::test]
async fn test_mock_broadcast() {
    let node = MockGrapheneNode::new();

    let topic = monad_node::p2p::TopicId::from_name("broadcast-topic");
    let message = b"Hello, gossip network!";

    // Broadcast a message
    node.broadcast(topic, message).await.unwrap();

    // Verify it was recorded
    let spy = node.spy();
    assert_eq!(spy.broadcast_messages.len(), 1);
    assert_eq!(spy.broadcast_messages[0].0, topic);
    assert_eq!(spy.broadcast_messages[0].1, message.to_vec());
}

#[tokio::test]
async fn test_mock_shutdown() {
    let node = MockGrapheneNode::new();

    // Upload something first
    let data = b"pre-shutdown data";
    node.upload_blob(data).await.unwrap();

    // Shutdown
    node.shutdown().await.unwrap();

    // Verify shutdown was recorded
    assert!(node.spy().shutdown_called);

    // Operations should now fail
    let result = node.upload_blob(b"post-shutdown").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_node_identity() {
    let node = MockGrapheneNode::new();

    // Node should have a valid identity
    let _node_id = node.node_id();

    // Should be able to get the node address
    let addr = node.node_addr().await.unwrap();

    // The address should be valid (we can't easily compare the node ID)
    // Just verify we got an address without error
    let _ = addr;
}

#[tokio::test]
async fn test_mock_inject_blob() {
    let node = MockGrapheneNode::new();

    // Pre-inject a blob
    let data = b"injected blob data";
    let hash = iroh_blobs::Hash::new(data);
    node.inject_blob(hash, data.to_vec());

    // Should be able to download it without uploading
    let downloaded = node.download_blob(hash, None).await.unwrap();
    assert_eq!(downloaded, data);

    // Upload spy should be empty (we didn't use upload_blob)
    assert!(node.spy().uploaded_blobs.is_empty());
}

#[tokio::test]
async fn test_mock_behavior_gossip_failure() {
    let node = MockGrapheneNode::with_behavior(MockBehavior::GossipFailure);

    let topic = monad_node::p2p::TopicId::from_name("failing-topic");

    // Subscribe should fail
    let result = node.subscribe(topic).await;
    assert!(result.is_err());

    // Broadcast should also fail
    let result = node.broadcast(topic, b"message").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_behavior_connection_failure() {
    let node = MockGrapheneNode::with_behavior(MockBehavior::ConnectionFailure);

    // Create a fake address
    let mut key_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key_bytes);
    let fake_key = iroh::SecretKey::from_bytes(&key_bytes);
    let fake_addr = iroh::EndpointAddr::new(fake_key.public());

    // Connection should fail
    let result = node.connect(fake_addr, b"test-alpn").await;
    assert!(result.is_err());

    // But the attempt should be recorded
    assert_eq!(node.spy().connection_attempts.len(), 1);
}

#[tokio::test]
async fn test_mock_dynamic_behavior_change() {
    let node = MockGrapheneNode::new();

    // Start with happy path - upload works
    let data = b"test";
    let hash = node.upload_blob(data).await.unwrap();

    // Download works
    assert!(node.download_blob(hash, None).await.is_ok());

    // Change behavior to failure mode
    node.set_behavior(MockBehavior::BlobDownloadFailure);

    // Now download fails
    assert!(node.download_blob(hash, None).await.is_err());

    // Change back to happy path
    node.set_behavior(MockBehavior::HappyPath);

    // Download works again
    assert!(node.download_blob(hash, None).await.is_ok());
}
