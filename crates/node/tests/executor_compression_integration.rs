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
use graphene_node::p2p::protocol::types::{Compression, JobAssets};

fn make_channel_keys() -> (ChannelKeys, [u8; 32], [u8; 32]) {
    let user_secret = [21u8; 32];
    let worker_secret = [22u8; 32];
    let channel_pda = [23u8; 32];

    let user_signing = SigningKey::from_bytes(&user_secret);
    let worker_signing = SigningKey::from_bytes(&worker_secret);
    let user_public = user_signing.verifying_key().to_bytes();
    let worker_public = worker_signing.verifying_key().to_bytes();

    let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();

    (user_keys, user_public, worker_secret)
}

fn make_manifest(runtime: &str) -> JobManifest {
    JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5_000,
        runtime: runtime.to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    }
}

#[tokio::test]
async fn executor_decompresses_inline_zstd_assets() {
    let (user_keys, user_public, worker_secret) = make_channel_keys();
    let channel_pda = [23u8; 32];

    let plaintext_code = vec![b'a'; 2048];
    let plaintext_input = vec![b'b'; 1024];

    let compressed_code = zstd::encode_all(plaintext_code.as_slice(), 3).unwrap();
    let compressed_input = zstd::encode_all(plaintext_input.as_slice(), 3).unwrap();

    let crypto = Arc::new(MockCryptoProvider::working());
    let encrypted_code = crypto
        .encrypt_job_blob(
            &compressed_code,
            &user_keys,
            "job-inline",
            EncryptionDirection::Input,
        )
        .unwrap()
        .to_bytes();
    let encrypted_input = crypto
        .encrypt_job_blob(
            &compressed_input,
            &user_keys,
            "job-inline",
            EncryptionDirection::Input,
        )
        .unwrap()
        .to_bytes();

    let cache = Arc::new(MockBuildCache::new());
    let requirements: &[String] = &[];
    let code_hash = blake3::hash(&encrypted_code);
    cache
        .store(
            "zstd-inline:1",
            requirements,
            code_hash.as_bytes(),
            std::env::temp_dir().join("zstd-inline.unik"),
        )
        .await
        .unwrap();

    let drive_builder = Arc::new(MockDriveBuilder::new());
    let runner = Arc::new(MockRunner::success());
    let output = Arc::new(MockOutputProcessor::working());
    let network = Arc::new(MockGrapheneNode::new());

    let executor = DefaultJobExecutor::new(
        drive_builder.clone(),
        runner,
        output,
        crypto,
        network,
        cache,
        worker_secret,
    );

    let assets = JobAssets::inline(encrypted_code, Some(encrypted_input))
        .with_compression(Compression::Zstd);

    let request = ExecutionRequest::new(
        "job-inline",
        make_manifest("zstd-inline:1"),
        assets,
        [0u8; 32],
        channel_pda,
        user_public,
        ResultDeliveryMode::Sync,
    );

    let result = executor.execute(request).await.unwrap();
    assert_eq!(result.exit_code, 0);

    let spy = drive_builder.spy.lock().unwrap();
    assert_eq!(spy.last_code_size, Some(plaintext_code.len()));
    assert_eq!(spy.last_input_size, Some(plaintext_input.len()));
}

#[tokio::test]
async fn executor_decompresses_blob_zstd_assets() {
    let (user_keys, user_public, worker_secret) = make_channel_keys();
    let channel_pda = [23u8; 32];

    let plaintext_code = vec![b'c'; 2048];
    let plaintext_input = vec![b'd'; 1024];

    let compressed_code = zstd::encode_all(plaintext_code.as_slice(), 3).unwrap();
    let compressed_input = zstd::encode_all(plaintext_input.as_slice(), 3).unwrap();

    let crypto = Arc::new(MockCryptoProvider::working());
    let encrypted_code = crypto
        .encrypt_job_blob(
            &compressed_code,
            &user_keys,
            "job-blob",
            EncryptionDirection::Input,
        )
        .unwrap()
        .to_bytes();
    let encrypted_input = crypto
        .encrypt_job_blob(
            &compressed_input,
            &user_keys,
            "job-blob",
            EncryptionDirection::Input,
        )
        .unwrap()
        .to_bytes();

    let network = Arc::new(MockGrapheneNode::new());
    let code_hash = iroh_blobs::Hash::new(&encrypted_code);
    let input_hash = iroh_blobs::Hash::new(&encrypted_input);
    network.inject_blob(code_hash, encrypted_code.clone());
    network.inject_blob(input_hash, encrypted_input.clone());

    let cache = Arc::new(MockBuildCache::new());
    let requirements: &[String] = &[];
    cache
        .store(
            "zstd-blob:1",
            requirements,
            code_hash.as_bytes(),
            std::env::temp_dir().join("zstd-blob.unik"),
        )
        .await
        .unwrap();

    let drive_builder = Arc::new(MockDriveBuilder::new());
    let runner = Arc::new(MockRunner::success());
    let output = Arc::new(MockOutputProcessor::working());

    let executor = DefaultJobExecutor::new(
        drive_builder.clone(),
        runner,
        output,
        crypto,
        network,
        cache,
        worker_secret,
    );

    let assets = JobAssets::blobs(code_hash, Some(input_hash)).with_compression(Compression::Zstd);

    let request = ExecutionRequest::new(
        "job-blob",
        make_manifest("zstd-blob:1"),
        assets,
        [0u8; 32],
        channel_pda,
        user_public,
        ResultDeliveryMode::Sync,
    );

    let result = executor.execute(request).await.unwrap();
    assert_eq!(result.exit_code, 0);

    let spy = drive_builder.spy.lock().unwrap();
    assert_eq!(spy.last_code_size, Some(plaintext_code.len()));
    assert_eq!(spy.last_input_size, Some(plaintext_input.len()));
}
