use std::collections::HashMap;
use std::sync::Arc;

use ed25519_dalek::SigningKey;
use graphene_node::cache::{BuildCache, MockBuildCache};
use graphene_node::crypto::{ChannelKeys, CryptoProvider, EncryptionDirection, MockCryptoProvider};
use graphene_node::executor::drive::mock::MockDriveBuilder;
use graphene_node::executor::output::MockOutputProcessor;
use graphene_node::executor::runner::MockRunner;
use graphene_node::executor::{DefaultJobExecutor, ExecutionRequest, JobExecutor};
use graphene_node::p2p::messages::{JobManifest, ResultDeliveryMode};
use graphene_node::p2p::mock::MockGrapheneNode;
use graphene_node::p2p::protocol::types::JobAssets;

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
        runtime: "python:3.12".to_string(),
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

#[tokio::test]
async fn executor_cache_miss_returns_build_error() {
    let user_secret = [11u8; 32];
    let worker_secret = [22u8; 32];
    let channel_pda = [33u8; 32];

    let user_signing = SigningKey::from_bytes(&user_secret);
    let worker_signing = SigningKey::from_bytes(&worker_secret);
    let user_public = user_signing.verifying_key().to_bytes();
    let worker_public = worker_signing.verifying_key().to_bytes();

    let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();

    let crypto = Arc::new(MockCryptoProvider::working());
    let job_id = "cache-miss-job";
    let plaintext_code = b"print('cache miss')";
    let encrypted_blob = crypto
        .encrypt_job_blob(
            plaintext_code,
            &user_keys,
            job_id,
            EncryptionDirection::Input,
        )
        .unwrap();
    let encrypted_code = encrypted_blob.to_bytes();

    let cache = Arc::new(MockBuildCache::new());
    let drive_builder = Arc::new(MockDriveBuilder::new());
    let runner = Arc::new(MockRunner::success());
    let output = Arc::new(MockOutputProcessor::working());
    let network = Arc::new(MockGrapheneNode::new());

    let executor = DefaultJobExecutor::new(
        drive_builder,
        runner,
        output,
        crypto,
        network,
        cache,
        worker_secret,
    );

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5_000,
        runtime: "cache-test:1".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let assets = JobAssets::inline(encrypted_code, None);
    let request = ExecutionRequest::new(
        job_id,
        manifest,
        assets,
        [0u8; 32],
        channel_pda,
        user_public,
        ResultDeliveryMode::Sync,
    );

    let result = executor.execute(request).await;
    assert!(matches!(
        result,
        Err(graphene_node::executor::ExecutionError::BuildFailed(_))
    ));
}

#[tokio::test]
async fn executor_cache_key_changes_when_code_hash_changes() {
    let user_secret = [4u8; 32];
    let worker_secret = [5u8; 32];
    let channel_pda = [6u8; 32];

    let user_signing = SigningKey::from_bytes(&user_secret);
    let worker_signing = SigningKey::from_bytes(&worker_secret);
    let user_public = user_signing.verifying_key().to_bytes();
    let worker_public = worker_signing.verifying_key().to_bytes();

    let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();

    let crypto = Arc::new(MockCryptoProvider::working());
    let cache = Arc::new(MockBuildCache::new());

    // Store a cached kernel for code A.
    let job_id_a = "cache-key-a";
    let encrypted_a = crypto
        .encrypt_job_blob(
            b"print('code A')",
            &user_keys,
            job_id_a,
            EncryptionDirection::Input,
        )
        .unwrap()
        .to_bytes();
    let code_hash_a = blake3::hash(&encrypted_a);
    cache
        .store(
            "cache-test:2",
            &[],
            code_hash_a.as_bytes(),
            std::env::temp_dir().join("cached-kernel-A.unik"),
        )
        .await
        .unwrap();

    // Build an execution request with different code (code B).
    let job_id_b = "cache-key-b";
    let encrypted_b = crypto
        .encrypt_job_blob(
            b"print('code B')",
            &user_keys,
            job_id_b,
            EncryptionDirection::Input,
        )
        .unwrap()
        .to_bytes();

    let drive_builder = Arc::new(MockDriveBuilder::new());
    let runner = Arc::new(MockRunner::success());
    let output = Arc::new(MockOutputProcessor::working());
    let network = Arc::new(MockGrapheneNode::new());

    let executor = DefaultJobExecutor::new(
        drive_builder,
        runner,
        output,
        crypto,
        network,
        cache.clone(),
        worker_secret,
    );

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5_000,
        runtime: "cache-test:2".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let assets = JobAssets::inline(encrypted_b, None);
    let request = ExecutionRequest::new(
        job_id_b,
        manifest,
        assets,
        [0u8; 32],
        channel_pda,
        user_public,
        ResultDeliveryMode::Sync,
    );

    let result = executor.execute(request).await;
    assert!(matches!(
        result,
        Err(graphene_node::executor::ExecutionError::BuildFailed(_))
    ));
    assert_eq!(cache.entry_count(), 1);
}
