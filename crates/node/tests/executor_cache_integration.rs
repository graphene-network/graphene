use std::collections::HashMap;
use std::sync::Arc;

use ed25519_dalek::SigningKey;
use monad_node::cache::{BuildCache, MockBuildCache};
use monad_node::crypto::{ChannelKeys, CryptoProvider, EncryptionDirection, MockCryptoProvider};
use monad_node::executor::drive::mock::MockDriveBuilder;
use monad_node::executor::output::MockOutputProcessor;
use monad_node::executor::runner::MockRunner;
use monad_node::executor::{DefaultJobExecutor, ExecutionRequest, JobExecutor};
use monad_node::p2p::messages::{JobManifest, ResultDeliveryMode};
use monad_node::p2p::mock::MockGrapheneNode;
use monad_node::p2p::protocol::types::JobAssets;

/// Verifies the executor pulls the kernel from the build cache and hands that
/// path to the runner, covering the cache ↔ executor boundary described in the
/// whitepaper.
#[tokio::test]
async fn executor_uses_cached_kernel_when_available() {
    // Deterministic channel keys for encryption/decryption.
    let user_secret = [1u8; 32];
    let worker_secret = [2u8; 32];
    let channel_pda = [3u8; 32];

    let user_signing = SigningKey::from_bytes(&user_secret);
    let worker_signing = SigningKey::from_bytes(&worker_secret);
    let user_public = user_signing.verifying_key().to_bytes();
    let worker_public = worker_signing.verifying_key().to_bytes();

    let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();
    let worker_keys = ChannelKeys::derive(&worker_secret, &user_public, &channel_pda).unwrap();

    // Encrypt inline code the same way the client would.
    let crypto = Arc::new(MockCryptoProvider::working());
    let job_id = "cache-hit-job";
    let plaintext_code = br#"print("hello from cache")"#;
    let encrypted_blob = crypto
        .encrypt_job_blob(
            plaintext_code,
            &user_keys,
            job_id,
            EncryptionDirection::Input,
        )
        .unwrap();
    let encrypted_code = encrypted_blob.to_bytes();

    // Pre-populate the build cache with the code hash for this job.
    let cache = Arc::new(MockBuildCache::new());
    let code_hash = blake3::hash(&encrypted_code);
    let cached_kernel = std::env::temp_dir().join("cached-python-3-12.unik");
    let _blob_hash = cache
        .store(
            "python:3.12",
            &[],
            code_hash.as_bytes(),
            cached_kernel.clone(),
        )
        .await
        .unwrap();

    // Wire up the executor with mocks.
    let drive_builder = Arc::new(MockDriveBuilder::new());
    let runner = Arc::new(MockRunner::success());
    let output = Arc::new(MockOutputProcessor::working());
    let network = Arc::new(MockGrapheneNode::new());

    let executor = DefaultJobExecutor::new(
        drive_builder.clone(),
        runner.clone(),
        output,
        crypto.clone(),
        network,
        cache.clone(),
        worker_secret,
    );

    // Build the request using inline encrypted assets.
    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5_000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let assets = JobAssets::inline(encrypted_code.clone(), None);
    let request = ExecutionRequest::new(
        job_id,
        manifest,
        assets,
        [0u8; 32], // ephemeral pubkey (unused in this path)
        channel_pda,
        user_public,
        ResultDeliveryMode::Sync,
    )
    .with_client_node_id([9u8; 32]); // exercise client id plumbing

    // Run the executor end-to-end; it should reuse the cached kernel.
    let result = executor.execute(request).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(cache.entry_count(), 1);
    assert_eq!(drive_builder.prepare_count(), 1);

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].kernel_path, cached_kernel.display().to_string());

    // Ensure decryption actually used the worker-side keys.
    let decrypted = crypto
        .decrypt_job_blob(
            &encrypted_blob,
            &worker_keys,
            job_id,
            EncryptionDirection::Input,
        )
        .unwrap();
    assert_eq!(decrypted, plaintext_code);
}
