//! P2P integration tests using real Iroh infrastructure.
//!
//! These tests spin up actual GrapheneNode instances and verify real P2P
//! networking operations work correctly.
//!
//! Run with: cargo test --features integration-tests

#![cfg(feature = "integration-tests")]

use monad_node::p2p::{GrapheneNode, P2PConfig, P2PNetwork, TopicId};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Helper to create a GrapheneNode with a temporary storage directory.
async fn create_test_node() -> (GrapheneNode, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config = P2PConfig::new(temp_dir.path()).with_relay(false);
    let node = GrapheneNode::new(config)
        .await
        .expect("Failed to create GrapheneNode");
    (node, temp_dir)
}

#[tokio::test]
async fn test_node_initialization() {
    let (node, _temp_dir) = create_test_node().await;

    // Node should have a valid identity
    let node_id = node.node_id();
    assert!(!node_id.as_bytes().iter().all(|&b| b == 0));

    // Should be able to get the node address
    let addr = node.node_addr().await.expect("Failed to get node address");
    assert_eq!(addr.id, node_id);

    node.shutdown().await.expect("Failed to shutdown");
}

#[tokio::test]
async fn test_blob_upload_and_local_download() {
    let (node, _temp_dir) = create_test_node().await;

    // Upload a blob
    let data = b"Hello, Graphene Network! This is a real blob.";
    let hash = node.upload_blob(data).await.expect("Failed to upload blob");

    // Verify the blob exists locally
    assert!(node.has_blob(hash).await.expect("Failed to check blob"));

    // Download and verify content
    let downloaded = node
        .download_blob(hash, None)
        .await
        .expect("Failed to download blob");
    assert_eq!(downloaded, data);

    node.shutdown().await.expect("Failed to shutdown");
}

#[tokio::test]
async fn test_blob_upload_from_file() {
    let (node, temp_dir) = create_test_node().await;

    // Create a test file
    let file_path = temp_dir.path().join("test_file.txt");
    let file_content = b"This is content from a file on disk.";
    std::fs::write(&file_path, file_content).expect("Failed to write test file");

    // Upload from path
    let hash = node
        .upload_blob_from_path(&file_path)
        .await
        .expect("Failed to upload blob from path");

    // Verify it exists and content matches
    assert!(node.has_blob(hash).await.expect("Failed to check blob"));
    let downloaded = node
        .download_blob(hash, None)
        .await
        .expect("Failed to download blob");
    assert_eq!(downloaded, file_content);

    node.shutdown().await.expect("Failed to shutdown");
}

#[tokio::test]
async fn test_blob_not_found() {
    let (node, _temp_dir) = create_test_node().await;

    // Try to download a non-existent blob
    let fake_hash = iroh_blobs::Hash::new(b"this content does not exist anywhere");

    // Should not exist locally
    assert!(!node
        .has_blob(fake_hash)
        .await
        .expect("Failed to check blob"));

    // Download should fail
    let result = node.download_blob(fake_hash, None).await;
    assert!(result.is_err());

    node.shutdown().await.expect("Failed to shutdown");
}

#[tokio::test]
async fn test_large_blob_upload() {
    let (node, _temp_dir) = create_test_node().await;

    // Create a 1MB blob
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
    let hash = node
        .upload_blob(&data)
        .await
        .expect("Failed to upload large blob");

    // Verify it exists and content matches
    assert!(node.has_blob(hash).await.expect("Failed to check blob"));
    let downloaded = node
        .download_blob(hash, None)
        .await
        .expect("Failed to download blob");
    assert_eq!(downloaded.len(), data.len());
    assert_eq!(downloaded, data);

    node.shutdown().await.expect("Failed to shutdown");
}

#[tokio::test]
async fn test_gossip_subscription() {
    let (node, _temp_dir) = create_test_node().await;

    let topic = TopicId::from_name("integration-test-topic");

    // Subscribe to a topic
    let subscription = node
        .subscribe(topic)
        .await
        .expect("Failed to subscribe to topic");

    // Verify subscription has the correct topic
    assert_eq!(subscription.topic, topic);

    node.shutdown().await.expect("Failed to shutdown");
}

#[tokio::test]
async fn test_gossip_broadcast() {
    let (node, _temp_dir) = create_test_node().await;

    let topic = TopicId::from_name("broadcast-test-topic");
    let message = b"Hello from the gossip network!";

    // Broadcasting should succeed (even with no peers)
    node.broadcast(topic, message)
        .await
        .expect("Failed to broadcast message");

    node.shutdown().await.expect("Failed to shutdown");
}

#[tokio::test]
async fn test_identity_persistence() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let storage_path = temp_dir.path().to_path_buf();

    // Create a node and get its identity
    let config1 = P2PConfig::new(storage_path.clone()).with_relay(false);
    let node1 = GrapheneNode::new(config1)
        .await
        .expect("Failed to create first node");
    let node_id1 = node1.node_id();
    node1
        .shutdown()
        .await
        .expect("Failed to shutdown first node");

    // Create another node with the same storage path
    let config2 = P2PConfig::new(storage_path).with_relay(false);
    let node2 = GrapheneNode::new(config2)
        .await
        .expect("Failed to create second node");
    let node_id2 = node2.node_id();
    node2
        .shutdown()
        .await
        .expect("Failed to shutdown second node");

    // Both should have the same identity
    assert_eq!(node_id1, node_id2);
}

#[tokio::test]
async fn test_multiple_blobs() {
    let (node, _temp_dir) = create_test_node().await;

    let blobs: Vec<&[u8]> = vec![
        b"First blob content",
        b"Second blob content with different data",
        b"Third blob - even more data here",
    ];

    let mut hashes = Vec::new();

    // Upload all blobs
    for data in &blobs {
        let hash = node
            .upload_blob(*data)
            .await
            .expect("Failed to upload blob");
        hashes.push(hash);
    }

    // Verify all exist and have correct content
    for (data, hash) in blobs.iter().zip(hashes.iter()) {
        assert!(node.has_blob(*hash).await.expect("Failed to check blob"));
        let downloaded = node
            .download_blob(*hash, None)
            .await
            .expect("Failed to download blob");
        assert_eq!(&downloaded[..], *data);
    }

    node.shutdown().await.expect("Failed to shutdown");
}

#[tokio::test]
async fn test_shutdown_prevents_operations() {
    let (node, _temp_dir) = create_test_node().await;

    // Upload before shutdown
    let data = b"pre-shutdown data";
    let hash = node
        .upload_blob(data)
        .await
        .expect("Failed to upload before shutdown");

    // Shutdown
    node.shutdown().await.expect("Failed to shutdown");

    // Operations should now fail
    let result = node.upload_blob(b"post-shutdown").await;
    assert!(result.is_err());

    let result = node.download_blob(hash, None).await;
    assert!(result.is_err());

    let result = node.has_blob(hash).await;
    assert!(result.is_err());

    let result = node.node_addr().await;
    assert!(result.is_err());
}

/// This test requires network connectivity between nodes which may not be available in CI.
/// The test passes locally but can timeout in isolated CI environments without a relay.
#[tokio::test]
#[ignore]
async fn test_two_node_connection() {
    let (node1, _temp_dir1) = create_test_node().await;
    let (node2, _temp_dir2) = create_test_node().await;

    // Get node1's address
    let addr1 = node1
        .node_addr()
        .await
        .expect("Failed to get node1 address");

    // Node2 connects to node1 using the job ALPN
    let result = node2
        .connect(addr1, monad_node::p2p::graphene::GRAPHENE_JOB_ALPN)
        .await;

    // Connection should succeed (node1 supports this ALPN)
    assert!(result.is_ok());

    node1.shutdown().await.expect("Failed to shutdown node1");
    node2.shutdown().await.expect("Failed to shutdown node2");
}

#[tokio::test]
async fn test_accept_loop_with_handler() {
    let (node1, _temp_dir1) = create_test_node().await;
    let node1 = Arc::new(node1);

    // Start accept loop with a simple handler
    let handler = Arc::new(
        |_conn: iroh::endpoint::Connection, _node: Arc<GrapheneNode>| async move {
            Ok::<(), monad_node::p2p::P2PError>(())
        },
    );

    let node1_clone = node1.clone();
    let accept_handle = tokio::spawn(async move {
        node1_clone.accept_loop(handler).await;
    });

    // Create a second node and connect
    let (node2, _temp_dir2) = create_test_node().await;

    let addr1 = node1
        .node_addr()
        .await
        .expect("Failed to get node1 address");

    // Give the accept loop time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect should work
    let conn = node2
        .connect(addr1, monad_node::p2p::graphene::GRAPHENE_JOB_ALPN)
        .await
        .expect("Failed to connect");

    // Connection should be usable
    assert!(!conn.alpn().is_empty());

    // Cleanup
    node1.shutdown().await.expect("Failed to shutdown node1");
    node2.shutdown().await.expect("Failed to shutdown node2");
    accept_handle.abort();
}

#[tokio::test]
async fn test_concurrent_blob_operations() {
    let (node, _temp_dir) = create_test_node().await;
    let node = Arc::new(node);

    // Spawn multiple concurrent upload tasks
    let mut handles = Vec::new();
    for i in 0..10 {
        let node_clone = node.clone();
        let handle = tokio::spawn(async move {
            let data = format!("Concurrent blob number {}", i);
            node_clone.upload_blob(data.as_bytes()).await
        });
        handles.push(handle);
    }

    // Wait for all uploads and collect hashes
    let mut hashes = Vec::new();
    for handle in handles {
        let hash = handle.await.expect("Task panicked").expect("Upload failed");
        hashes.push(hash);
    }

    // Verify all blobs exist
    for hash in &hashes {
        assert!(node.has_blob(*hash).await.expect("Failed to check blob"));
    }

    node.shutdown().await.expect("Failed to shutdown");
}
