use std::sync::Arc;

use graphene_node::cache::{build::LayeredBuildCache, BuildCache};
use graphene_node::cache::{iroh::IrohCache, local::LocalDiskCache};
use graphene_node::p2p::messages::CacheAnnouncement;
use graphene_node::p2p::mock::MockGrapheneNode;
use graphene_node::p2p::TopicId;

#[tokio::test]
async fn layered_build_cache_announces_on_store() {
    let network = Arc::new(MockGrapheneNode::new());
    let temp = tempfile::tempdir().unwrap();

    let local_cache = LocalDiskCache::new(temp.path().join("local").to_str().unwrap());
    let iroh_cache = IrohCache::new(network.clone(), temp.path().join("iroh"));
    let cache = LayeredBuildCache::new(local_cache, iroh_cache, network.clone());

    let kernel_spec = "python:3.12";
    let code_hash = [9u8; 32];
    let artifact_path = temp.path().join("artifact.unik");
    std::fs::write(&artifact_path, b"unikernel").unwrap();

    let blob_hash = cache
        .store(kernel_spec, &[], &code_hash, artifact_path)
        .await
        .unwrap();

    let spy = network.spy();
    assert_eq!(spy.broadcast_messages.len(), 1);

    let (topic, payload) = &spy.broadcast_messages[0];
    assert_eq!(*topic, TopicId::cache_v1());

    let announcement: CacheAnnouncement = serde_json::from_slice(payload).unwrap();
    assert_eq!(announcement.runtime_spec, kernel_spec);
    assert_eq!(announcement.blob_hash, blob_hash);
    assert_eq!(announcement.size_bytes, 9);
}
