use super::{CacheError, DependencyCache};
use async_trait::async_trait;
use iroh::bytes::util::runtime;
use iroh::client::Doc;
use iroh::node::{Node, NodeOptions};
use std::path::PathBuf;
use std::sync::Arc;

pub struct IrohCache {
    node: Node<runtime::Handle>, // The Iroh Node
    storage_path: PathBuf,
}

impl IrohCache {
    pub async fn new(storage_path: PathBuf) -> Self {
        // 1. Initialize Iroh Node (Auto-generates identity + binds ports)
        let builder = Node::builder().persist(&storage_path).await.unwrap();
        let node = builder.spawn().await.unwrap();

        println!("🌍 Iroh Node Started. PeerID: {}", node.node_id());

        Self { node, storage_path }
    }

    /// Helper to convert our dependency list to a "Ticket" (Iroh's link format)
    fn reqs_to_tag(&self, reqs: &[String]) -> Vec<u8> {
        let mut sorted = reqs.to_vec();
        sorted.sort();
        sorted.join("|").into_bytes()
    }
}

#[async_trait]
impl DependencyCache for IrohCache {
    fn calculate_hash(&self, reqs: &[String]) -> String {
        // In Iroh, we use BLAKE3 hashes, but for the API we keep a string representation
        let tag = self.reqs_to_tag(reqs);
        hex::encode(blake3::hash(&tag).as_bytes())
    }

    async fn get(&self, hash: &str) -> Result<Option<PathBuf>, CacheError> {
        let client = self.node.client();

        // 1. Check Local Blob Store
        // Iroh stores data by Hash. We check if we have the blob.
        let hash_bytes = hex::decode(hash).map_err(|_| CacheError::InvalidHash)?;
        let blob_hash: iroh::Hash = hash_bytes.try_into().unwrap();

        if client.blobs().has(blob_hash).await? {
            // It's local! Export it to a file path for Firecracker
            let reader = client.blobs().read(blob_hash).await?;
            let path = self.storage_path.join(format!("{}.img", hash));
            iroh::bytes::store::export_to_path(reader, &path).await?;
            return Ok(Some(path));
        }

        // 2. Check Network (The "Magic" Part)
        // Note: In Iroh, you typically need a "Ticket" (PeerID + Hash) to find data.
        // For a global cache, we can use the "Gossip" layer to ask "Who has hash X?"
        // OR (Simpler for PoC): We assume we know the provider (Node A) via the Blockchain.

        // Simulating: "We found Node A on Substrate, here is their ticket"
        println!("⚠️  Miss. Requesting from network...");

        // In a real implementation, you'd use iroh-gossip to find the provider.
        // For now, return None to trigger a rebuild (which then seeds the network).
        Ok(None)
    }

    async fn put(&self, hash: &str, source_path: PathBuf) -> Result<PathBuf, CacheError> {
        let client = self.node.client();

        // 1. Import file into Iroh (Makes it available to the network)
        let abs_path = source_path.canonicalize()?;
        let progress = client.blobs().add_from_path(abs_path).await?;
        let outcome = progress.finish().await?;

        println!("📢 Seeding Blob: {}", outcome.hash);

        // The file is now hosted via QUIC to anyone who asks for this hash.
        Ok(source_path)
    }
}
