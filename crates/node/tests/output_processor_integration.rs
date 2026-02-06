use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use graphene_node::crypto::{
    ChannelKeys, CryptoProvider, DefaultCryptoProvider, EncryptionDirection,
};
use graphene_node::executor::output::{DefaultOutputProcessor, OutputProcessor};
use graphene_node::executor::runner::VmmOutput;
use graphene_node::executor::ExecutionRequest;
use graphene_node::p2p::messages::{JobManifest, ResultDeliveryMode};
use graphene_node::p2p::protocol::types::JobAssets;

#[tokio::test]
async fn output_processor_encrypts_and_preserves_metrics() {
    let user_secret = [7u8; 32];
    let worker_secret = [8u8; 32];
    let channel_pda = [9u8; 32];

    let user_signing = ed25519_dalek::SigningKey::from_bytes(&user_secret);
    let worker_signing = ed25519_dalek::SigningKey::from_bytes(&worker_secret);
    let user_public = user_signing.verifying_key().to_bytes();
    let worker_public = worker_signing.verifying_key().to_bytes();

    let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();
    let worker_keys = ChannelKeys::derive(&worker_secret, &user_public, &channel_pda).unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let output_dir = tempdir.path().join("output");
    std::fs::create_dir_all(&output_dir).unwrap();
    std::fs::write(output_dir.join("result.txt"), b"hello output").unwrap();

    let request = ExecutionRequest::new(
        "output-job",
        JobManifest {
            vcpu: 1,
            memory_mb: 256,
            timeout_ms: 1000,
            kernel: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        },
        JobAssets::inline(vec![], None),
        [0u8; 32],
        channel_pda,
        user_public,
        ResultDeliveryMode::Sync,
    );

    let crypto = Arc::new(DefaultCryptoProvider);
    let processor = DefaultOutputProcessor::new(crypto.clone());

    let duration = Duration::from_millis(250);
    let vmm_output = VmmOutput::new(0, b"stdout".to_vec(), b"stderr".to_vec(), duration, false);
    let result = processor
        .process(
            tempdir.path(),
            vmm_output.stdout.clone(),
            vmm_output.stderr.clone(),
            vmm_output.exit_code,
            vmm_output.duration,
            &request,
            &worker_keys,
        )
        .await
        .unwrap();

    assert_eq!(result.exit_code, vmm_output.exit_code);
    assert_eq!(result.duration, vmm_output.duration);
    assert_eq!(
        result.result_hash,
        iroh_blobs::Hash::new(&result.encrypted_result)
    );

    let decrypted_result = crypto
        .decrypt_job_blob(
            &graphene_node::crypto::EncryptedBlob::from_bytes(&result.encrypted_result).unwrap(),
            &user_keys,
            &request.job_id,
            EncryptionDirection::Output,
        )
        .unwrap();

    let decrypted_stdout = crypto
        .decrypt_job_blob(
            &graphene_node::crypto::EncryptedBlob::from_bytes(&result.encrypted_stdout).unwrap(),
            &user_keys,
            &request.job_id,
            EncryptionDirection::Output,
        )
        .unwrap();

    let decrypted_stderr = crypto
        .decrypt_job_blob(
            &graphene_node::crypto::EncryptedBlob::from_bytes(&result.encrypted_stderr).unwrap(),
            &user_keys,
            &request.job_id,
            EncryptionDirection::Output,
        )
        .unwrap();

    assert_eq!(decrypted_stdout, vmm_output.stdout);
    assert_eq!(decrypted_stderr, vmm_output.stderr);

    let mut archive = tar::Archive::new(std::io::Cursor::new(decrypted_result));
    let mut entries = archive.entries().unwrap();
    let mut found = false;

    while let Some(Ok(mut entry)) = entries.next() {
        if entry.path().unwrap().ends_with("result.txt") {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).unwrap();
            assert_eq!(contents, "hello output");
            found = true;
        }
    }

    assert!(found);
}
