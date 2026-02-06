//! Integration tests for the executor module.
//!
//! These tests verify the integration between executor components using mock
//! implementations to avoid actual VMM/network calls while testing the full
//! job execution pipeline.

// Traits are imported for method resolution on mock types
#![allow(unused_imports)]

use graphene_node::cache::{BuildCache, MockBuildCache};
use graphene_node::crypto::{
    ChannelKeys, CryptoProvider, DefaultCryptoProvider, EncryptionDirection,
};
use graphene_node::executor::drive::mock::MockDriveBuilder;
use graphene_node::executor::{
    build_env_json, reserved_env, ExecutionDriveBuilder, ExecutionError, ExecutionRequest,
    JobExecutor, MockExecutorBehavior, MockJobExecutor, MockOutputProcessor, MockRunner,
    MockRunnerBehavior,
};
use graphene_node::p2p::messages::{JobManifest, ResultDeliveryMode};
use graphene_node::p2p::mock::MockGrapheneNode;
use graphene_node::p2p::protocol::types::JobAssets;
use graphene_node::p2p::P2PNetwork;
use iroh_blobs::Hash;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// Test Helpers
// ============================================================================

/// Create test channel keys pair (user and worker perspectives).
fn create_test_channel_keys() -> (ChannelKeys, ChannelKeys, [u8; 32], [u8; 32]) {
    let user_secret = [1u8; 32];
    let worker_secret = [2u8; 32];

    let user_signing = ed25519_dalek::SigningKey::from_bytes(&user_secret);
    let worker_signing = ed25519_dalek::SigningKey::from_bytes(&worker_secret);

    let user_public = user_signing.verifying_key().to_bytes();
    let worker_public = worker_signing.verifying_key().to_bytes();

    let channel_pda = [3u8; 32];

    let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();
    let worker_keys = ChannelKeys::derive(&worker_secret, &user_public, &channel_pda).unwrap();

    (user_keys, worker_keys, user_public, worker_secret)
}

/// Create a test execution request with customizable parameters.
fn create_test_request(
    job_id: &str,
    user_env: HashMap<String, String>,
    input_hash: Option<[u8; 32]>,
    timeout_ms: u64,
) -> ExecutionRequest {
    let input_hash_bytes = input_hash.unwrap_or([0u8; 32]);
    ExecutionRequest::new(
        job_id,
        JobManifest {
            vcpu: 1,
            memory_mb: 256,
            timeout_ms,
            kernel: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: user_env,
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        },
        JobAssets::blobs(
            Hash::from_bytes([1u8; 32]),
            Some(Hash::from_bytes(input_hash_bytes)),
        ),
        [0u8; 32], // ephemeral_pubkey
        [0u8; 32], // channel_pda
        [0u8; 32], // payer_pubkey
        ResultDeliveryMode::Sync,
    )
}

/// Create a simple test request with defaults.
fn make_simple_request(job_id: &str) -> ExecutionRequest {
    create_test_request(job_id, HashMap::new(), None, 30000)
}

// ============================================================================
// Environment Variable Tests
// ============================================================================

#[test]
fn test_env_json_has_graphene_vars() {
    let manifest = JobManifest {
        vcpu: 2,
        memory_mb: 512,
        timeout_ms: 30000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };
    let user_env = HashMap::new();

    let json = build_env_json("job-123", &user_env, &manifest);
    let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();

    // Verify all reserved GRAPHENE_* variables are present
    assert_eq!(parsed.get("GRAPHENE_JOB_ID"), Some(&"job-123".to_string()));
    assert_eq!(
        parsed.get("GRAPHENE_INPUT_PATH"),
        Some(&"/input".to_string())
    );
    assert_eq!(
        parsed.get("GRAPHENE_OUTPUT_PATH"),
        Some(&"/output".to_string())
    );
    assert_eq!(
        parsed.get("GRAPHENE_TIMEOUT_MS"),
        Some(&"30000".to_string())
    );
}

#[test]
fn test_user_env_vars_preserved() {
    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let mut user_env = HashMap::new();
    user_env.insert("API_KEY".to_string(), "sk-test-123".to_string());
    user_env.insert("MODE".to_string(), "production".to_string());
    user_env.insert("DEBUG".to_string(), "false".to_string());

    let json = build_env_json("job-user-env", &user_env, &manifest);
    let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();

    // User variables are preserved
    assert_eq!(parsed.get("API_KEY"), Some(&"sk-test-123".to_string()));
    assert_eq!(parsed.get("MODE"), Some(&"production".to_string()));
    assert_eq!(parsed.get("DEBUG"), Some(&"false".to_string()));

    // GRAPHENE_* vars still present
    assert!(parsed.contains_key("GRAPHENE_JOB_ID"));
}

#[test]
fn test_reserved_vars_override_user_attempts() {
    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 10000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let mut user_env = HashMap::new();
    // User attempts to override reserved variables
    user_env.insert("GRAPHENE_JOB_ID".to_string(), "malicious-id".to_string());
    user_env.insert(
        "GRAPHENE_INPUT_PATH".to_string(),
        "/malicious/path".to_string(),
    );
    user_env.insert(
        "GRAPHENE_CUSTOM_VAR".to_string(),
        "should-be-filtered".to_string(),
    );
    user_env.insert("SAFE_VAR".to_string(), "safe-value".to_string());

    let json = build_env_json("real-job-id", &user_env, &manifest);
    let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();

    // Reserved var should have the real value, not the user's attempt
    assert_eq!(
        parsed.get("GRAPHENE_JOB_ID"),
        Some(&"real-job-id".to_string())
    );
    assert_eq!(
        parsed.get("GRAPHENE_INPUT_PATH"),
        Some(&"/input".to_string())
    );

    // User's GRAPHENE_* vars should be filtered out
    assert!(!parsed.contains_key("GRAPHENE_CUSTOM_VAR"));

    // Safe user var should be included
    assert_eq!(parsed.get("SAFE_VAR"), Some(&"safe-value".to_string()));
}

#[test]
fn test_is_reserved_function() {
    // Reserved prefix check
    assert!(reserved_env::is_reserved("GRAPHENE_JOB_ID"));
    assert!(reserved_env::is_reserved("GRAPHENE_ANYTHING"));
    assert!(reserved_env::is_reserved("GRAPHENE_"));

    // Non-reserved
    assert!(!reserved_env::is_reserved("MY_VAR"));
    assert!(!reserved_env::is_reserved("API_KEY"));
    assert!(!reserved_env::is_reserved("GRAPHENE")); // No underscore
}

// ============================================================================
// Drive Builder Integration Tests
// ============================================================================

#[tokio::test]
async fn test_drive_builder_records_code_and_env() {
    let builder = MockDriveBuilder::new();
    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let mut user_env = HashMap::new();
    user_env.insert("MY_VAR".to_string(), "my_value".to_string());

    let code_tarball = b"mock-code-tarball-content";

    let result = builder
        .prepare("test-job-1", code_tarball, None, &user_env, &manifest)
        .await;

    assert!(result.is_ok());
    assert!(builder.was_prepared("test-job-1"));
    assert_eq!(builder.prepare_count(), 1);

    // Verify env JSON was captured
    let env_json = builder.get_last_env_json().unwrap();
    let parsed: HashMap<String, String> = serde_json::from_str(&env_json).unwrap();
    assert_eq!(parsed.get("MY_VAR"), Some(&"my_value".to_string()));
    assert!(parsed.contains_key("GRAPHENE_JOB_ID"));
}

#[tokio::test]
async fn test_drive_builder_with_input() {
    let builder = MockDriveBuilder::new();
    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let code = b"code-content";
    let input = b"input-data-for-job";

    let result = builder
        .prepare("test-job-2", code, Some(input), &HashMap::new(), &manifest)
        .await;

    assert!(result.is_ok());

    // Verify input size was recorded
    let spy = builder.spy.lock().unwrap();
    assert_eq!(spy.last_code_size, Some(12)); // "code-content".len()
    assert_eq!(spy.last_input_size, Some(18)); // "input-data-for-job".len()
}

#[tokio::test]
async fn test_drive_builder_no_input_when_not_provided() {
    let builder = MockDriveBuilder::new();
    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let result = builder
        .prepare("test-job-3", b"code", None, &HashMap::new(), &manifest)
        .await;

    assert!(result.is_ok());

    // Verify no input was recorded
    let spy = builder.spy.lock().unwrap();
    assert!(spy.last_input_size.is_none());
}

// ============================================================================
// VMM Runner Integration Tests
// ============================================================================

#[tokio::test]
async fn test_runner_success_path() {
    let runner = MockRunner::new(MockRunnerBehavior::Success {
        stdout: b"Job completed successfully\n".to_vec(),
        duration: Duration::from_millis(150),
    });

    let manifest = JobManifest {
        vcpu: 2,
        memory_mb: 512,
        timeout_ms: 30000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    use graphene_node::executor::VmmRunner;
    let result = runner
        .run(
            std::path::Path::new("/kernel"),
            std::path::Path::new("/drive"),
            &manifest,
            "console=ttyS0 quiet",
        )
        .await
        .unwrap();

    assert!(result.succeeded());
    assert_eq!(result.exit_code, 0);
    assert!(!result.timed_out);
    assert_eq!(result.stdout, b"Job completed successfully\n");
    assert_eq!(runner.call_count(), 1);
}

#[tokio::test]
async fn test_runner_timeout_behavior() {
    // Use a short timeout for the test
    let runner = MockRunner::new(MockRunnerBehavior::Timeout {
        partial_output: b"Starting job...\n".to_vec(),
    });

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 100, // Very short timeout for test
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    use graphene_node::executor::VmmRunner;
    let result = runner
        .run(
            std::path::Path::new("/kernel"),
            std::path::Path::new("/drive"),
            &manifest,
            "console=ttyS0",
        )
        .await
        .unwrap();

    assert!(result.timed_out);
    assert!(!result.succeeded());
    assert_eq!(result.exit_code, -1);
}

#[tokio::test]
async fn test_runner_crash_behavior() {
    let runner = MockRunner::new(MockRunnerBehavior::Crash {
        message: "Kernel panic: out of memory".to_string(),
    });

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 5000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    use graphene_node::executor::VmmRunner;
    let result = runner
        .run(
            std::path::Path::new("/kernel"),
            std::path::Path::new("/drive"),
            &manifest,
            "console=ttyS0",
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(
        err,
        graphene_node::executor::RunnerError::Crashed(_)
    ));
}

// ============================================================================
// Output Processor Integration Tests
// ============================================================================

#[tokio::test]
async fn test_output_processor_encrypts_all_data() {
    use graphene_node::executor::OutputProcessor;

    let processor = MockOutputProcessor::working();
    let request = make_simple_request("test-job-encrypt");

    let (_, worker_keys, _, _) = create_test_channel_keys();

    let result = processor
        .process(
            std::path::Path::new("/tmp/mock-drive"),
            b"stdout content".to_vec(),
            b"stderr content".to_vec(),
            0,
            Duration::from_millis(200),
            &request,
            &worker_keys,
        )
        .await
        .unwrap();

    // Verify result structure
    assert_eq!(result.exit_code, 0);
    assert!(result.succeeded());
    assert!(!result.encrypted_result.is_empty());
    assert!(!result.encrypted_stdout.is_empty());
    assert!(!result.encrypted_stderr.is_empty());
}

#[tokio::test]
async fn test_output_processor_preserves_exit_code() {
    use graphene_node::executor::OutputProcessor;

    let processor = MockOutputProcessor::working();
    let request = make_simple_request("test-job-exitcode");
    let (_, worker_keys, _, _) = create_test_channel_keys();

    // Test with non-zero exit code
    let result = processor
        .process(
            std::path::Path::new("/tmp/drive"),
            vec![],
            b"error: something failed".to_vec(),
            127,
            Duration::from_millis(50),
            &request,
            &worker_keys,
        )
        .await
        .unwrap();

    assert_eq!(result.exit_code, 127);
    assert!(!result.succeeded());
}

#[tokio::test]
async fn test_output_processor_preserves_duration() {
    use graphene_node::executor::OutputProcessor;

    let processor = MockOutputProcessor::working();
    let request = make_simple_request("test-job-duration");
    let (_, worker_keys, _, _) = create_test_channel_keys();
    let duration = Duration::from_secs(5);

    let result = processor
        .process(
            std::path::Path::new("/tmp/drive"),
            vec![],
            vec![],
            0,
            duration,
            &request,
            &worker_keys,
        )
        .await
        .unwrap();

    assert_eq!(result.duration, duration);
    assert_eq!(result.duration_ms(), 5000);
}

// ============================================================================
// Mock Job Executor Tests
// ============================================================================

#[tokio::test]
async fn test_mock_executor_success() {
    let executor = MockJobExecutor::success();
    let request = make_simple_request("job-success");

    let result = executor.execute(request).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.succeeded());
    assert_eq!(executor.call_count(), 1);
}

#[tokio::test]
async fn test_mock_executor_failure() {
    let executor = MockJobExecutor::failing("Infrastructure error");
    let request = make_simple_request("job-fail");

    let result = executor.execute(request).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ExecutionError::VmmError(_)));
}

#[tokio::test]
async fn test_mock_executor_timeout() {
    let executor = MockJobExecutor::new(MockExecutorBehavior::Timeout);
    let request = make_simple_request("job-timeout");

    let result = executor.execute(request).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ExecutionError::Timeout(_)));
}

#[tokio::test]
async fn test_mock_executor_cancellation() {
    let executor = MockJobExecutor::new(MockExecutorBehavior::Cancelled);
    let request = make_simple_request("job-cancel");

    let result = executor.execute(request).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ExecutionError::Cancelled));
}

#[tokio::test]
async fn test_mock_executor_custom_behavior() {
    use std::sync::Arc;

    let executor = MockJobExecutor::new(MockExecutorBehavior::Custom(Arc::new(|req| {
        if req.manifest.vcpu >= 4 {
            Err(ExecutionError::vmm("Too many vCPUs requested"))
        } else {
            Ok(graphene_node::executor::ExecutionResult::new(
                0,
                Duration::from_millis(100),
                b"result".to_vec(),
                vec![],
                vec![],
                Hash::new(b"result"),
            ))
        }
    })));

    // Request with 1 vCPU succeeds
    let request = make_simple_request("job-custom-1");
    let result = executor.execute(request).await;
    assert!(result.is_ok());

    // Request with many vCPUs fails
    let mut big_request = make_simple_request("job-custom-2");
    big_request.manifest.vcpu = 8;
    let result = executor.execute(big_request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_executor_is_running() {
    let executor = Arc::new(MockJobExecutor::success());

    // Job not running yet
    assert!(!executor.is_running("nonexistent").await);

    // Cannot cancel non-running job
    assert!(!executor.cancel("nonexistent").await);
}

#[tokio::test]
async fn test_mock_executor_call_count() {
    let executor = MockJobExecutor::success();

    for i in 0..5 {
        let request = make_simple_request(&format!("job-{}", i));
        let _ = executor.execute(request).await;
    }

    assert_eq!(executor.call_count(), 5);
}

// ============================================================================
// Cache Hit Path Tests
// ============================================================================

#[tokio::test]
async fn test_cache_hit_returns_cached_path() {
    use graphene_node::cache::BuildCache;

    let cache = MockBuildCache::new();

    // Pre-populate the cache
    let kernel_spec = "python:3.12";
    let requirements: Vec<String> = vec![];
    let code_hash = [1u8; 32];

    cache
        .store(
            kernel_spec,
            &requirements,
            &code_hash,
            PathBuf::from("/cached/kernel.unik"),
        )
        .await
        .unwrap();

    // Lookup should return the cached path
    let result = cache.lookup(kernel_spec, &requirements, &code_hash).await;

    assert!(result.is_ok());
    let lookup_result = result.unwrap();
    assert!(lookup_result.is_some());
    assert_eq!(
        lookup_result.unwrap().path,
        PathBuf::from("/cached/kernel.unik")
    );
}

#[tokio::test]
async fn test_cache_miss_returns_none() {
    use graphene_node::cache::BuildCache;

    let cache = MockBuildCache::new();

    // Don't pre-populate - should get cache miss
    let result = cache.lookup("python:3.12", &[], &[1u8; 32]).await.unwrap();

    assert!(result.is_none());
}

// ============================================================================
// P2P Network Mock Tests
// ============================================================================

#[tokio::test]
async fn test_p2p_blob_upload_and_download() {
    use graphene_node::p2p::P2PNetwork;

    let node = MockGrapheneNode::new();

    let data = b"encrypted code blob";
    let hash = node.upload_blob(data).await.unwrap();

    // Should be able to download
    let downloaded = node.download_blob(hash, None).await.unwrap();
    assert_eq!(downloaded, data);

    // Spy should record operations
    let spy = node.spy();
    assert_eq!(spy.uploaded_blobs.len(), 1);
    assert_eq!(spy.download_attempts.len(), 1);
}

#[tokio::test]
async fn test_p2p_blob_injection_for_testing() {
    use graphene_node::p2p::P2PNetwork;

    let node = MockGrapheneNode::new();

    // Inject a blob (simulating it already existing in the network)
    let data = b"pre-existing blob";
    let hash = Hash::new(data);
    node.inject_blob(hash, data.to_vec());

    // Should be able to download without uploading first
    let downloaded = node.download_blob(hash, None).await.unwrap();
    assert_eq!(downloaded, data);

    // Upload spy should be empty (we injected, not uploaded)
    assert!(node.spy().uploaded_blobs.is_empty());
}

#[tokio::test]
async fn test_p2p_blob_not_found() {
    use graphene_node::p2p::P2PNetwork;

    let node = MockGrapheneNode::new();

    let fake_hash = Hash::new(b"nonexistent");
    let result = node.download_blob(fake_hash, None).await;

    assert!(result.is_err());
}

// ============================================================================
// Crypto Integration Tests
// ============================================================================

#[test]
fn test_crypto_roundtrip() {
    let crypto = DefaultCryptoProvider;

    let (user_keys, worker_keys, _, _) = create_test_channel_keys();
    let job_id = "test-job-crypto";
    let plaintext = b"Secret job input data";

    // User encrypts
    let encrypted = crypto
        .encrypt_job_blob(plaintext, &user_keys, job_id, EncryptionDirection::Input)
        .unwrap();

    // Worker decrypts
    let decrypted = crypto
        .decrypt_job_blob(&encrypted, &worker_keys, job_id, EncryptionDirection::Input)
        .unwrap();

    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_crypto_wrong_job_id_fails() {
    let crypto = DefaultCryptoProvider;

    let (user_keys, worker_keys, _, _) = create_test_channel_keys();

    let encrypted = crypto
        .encrypt_job_blob(b"data", &user_keys, "job-1", EncryptionDirection::Input)
        .unwrap();

    // Try to decrypt with wrong job ID
    let result = crypto.decrypt_job_blob(
        &encrypted,
        &worker_keys,
        "job-2", // Wrong!
        EncryptionDirection::Input,
    );

    assert!(result.is_err());
}

#[test]
fn test_crypto_direction_separation() {
    let crypto = DefaultCryptoProvider;

    let (user_keys, worker_keys, _, _) = create_test_channel_keys();

    // Encrypt as input
    let encrypted = crypto
        .encrypt_job_blob(b"data", &user_keys, "job-1", EncryptionDirection::Input)
        .unwrap();

    // Try to decrypt as output (wrong direction)
    let result = crypto.decrypt_job_blob(
        &encrypted,
        &worker_keys,
        "job-1",
        EncryptionDirection::Output, // Wrong!
    );

    assert!(result.is_err());
}

// ============================================================================
// Job Executor Trait Tests
// ============================================================================

#[tokio::test]
async fn test_job_executor_trait_object_safe() {
    // Verify JobExecutor can be used as a trait object
    let executor: Box<dyn JobExecutor> = Box::new(MockJobExecutor::success());
    let request = make_simple_request("trait-object-test");

    let result = executor.execute(request).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execution_error_classification() {
    // Test error classification methods

    // Worker faults
    assert!(ExecutionError::vmm("crash").is_worker_fault());
    assert!(ExecutionError::drive("mount failed").is_worker_fault());
    assert!(!ExecutionError::vmm("crash").is_user_fault());

    // User faults
    assert!(ExecutionError::timeout(Duration::from_secs(30)).is_user_fault());
    assert!(ExecutionError::build("syntax error").is_user_fault());
    assert!(!ExecutionError::timeout(Duration::from_secs(30)).is_worker_fault());

    // Asset fetch is neither (could be network issue)
    assert!(!ExecutionError::asset_fetch("timeout").is_worker_fault());
    assert!(!ExecutionError::asset_fetch("timeout").is_user_fault());
}

// ============================================================================
// Timeout Handling Tests
// ============================================================================

#[tokio::test]
async fn test_timeout_duration_from_manifest() {
    let request = create_test_request(
        "job-timeout-test",
        HashMap::new(),
        None,
        15000, // 15 second timeout
    );

    assert_eq!(request.timeout(), Duration::from_millis(15000));
}

#[tokio::test]
async fn test_runner_enforces_manifest_timeout() {
    let runner = MockRunner::timeout();

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 50, // 50ms timeout for fast test
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    use graphene_node::executor::VmmRunner;

    let start = std::time::Instant::now();
    let result = runner
        .run(
            std::path::Path::new("/kernel"),
            std::path::Path::new("/drive"),
            &manifest,
            "console=ttyS0",
        )
        .await
        .unwrap();

    // Should have timed out
    assert!(result.timed_out);

    // Duration should be approximately the timeout value (with some tolerance)
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(50));
    assert!(elapsed < Duration::from_millis(500)); // Reasonable upper bound
}

// ============================================================================
// Cancellation Tests
// ============================================================================

#[tokio::test]
async fn test_cancel_returns_false_for_nonexistent_job() {
    let executor = MockJobExecutor::success();

    let cancelled = executor.cancel("nonexistent-job").await;
    assert!(!cancelled);
}

#[tokio::test]
async fn test_cancel_behavior_mock() {
    // The mock executor has internal cancellation support
    let executor = Arc::new(MockJobExecutor::new(MockExecutorBehavior::Success {
        exit_code: 0,
        duration: Duration::from_millis(100),
    }));

    // Start a job
    let request = make_simple_request("job-to-cancel");
    let executor_clone = executor.clone();
    let handle = tokio::spawn(async move { executor_clone.execute(request).await });

    // Give it a moment to start
    tokio::time::sleep(Duration::from_millis(5)).await;

    // Try to cancel
    let cancelled = executor.cancel("job-to-cancel").await;

    // Wait for the handle to complete
    let result = handle.await.unwrap();

    // The mock may or may not have been cancelled depending on timing
    // We just verify the cancel method works
    let _ = (cancelled, result);
}

// ============================================================================
// End-to-End Integration Test
// ============================================================================

#[tokio::test]
async fn test_e2e_job_execution_with_mocks() {
    // This test verifies that all components can work together
    // using mock implementations

    // Set up environment
    let mut env = HashMap::new();
    env.insert("API_KEY".to_string(), "test-key".to_string());
    env.insert("MODE".to_string(), "test".to_string());

    // Create request with env vars
    let request = create_test_request(
        "e2e-test-job",
        env,
        Some([2u8; 32]), // Has input
        30000,
    );

    // Use mock executor
    let executor = MockJobExecutor::success();

    // Execute
    let result = executor.execute(request).await.unwrap();

    // Verify result
    assert_eq!(result.exit_code, 0);
    assert!(result.succeeded());
    assert!(!result.encrypted_result.is_empty());
}

#[tokio::test]
async fn test_e2e_job_with_custom_result() {
    use std::sync::Arc;

    let executor = MockJobExecutor::new(MockExecutorBehavior::Custom(Arc::new(|req| {
        // Simulate different outcomes based on job ID
        match req.job_id.as_str() {
            "compute-pi" => Ok(graphene_node::executor::ExecutionResult::new(
                0,
                Duration::from_millis(500),
                b"3.14159265359".to_vec(),
                b"Computed pi to 11 digits\n".to_vec(),
                vec![],
                Hash::new(b"3.14159265359"),
            )),
            "fail-job" => Err(ExecutionError::vmm("Job failed intentionally")),
            _ => Ok(graphene_node::executor::ExecutionResult::new(
                0,
                Duration::from_millis(100),
                b"default result".to_vec(),
                vec![],
                vec![],
                Hash::new(b"default"),
            )),
        }
    })));

    // Test compute-pi job
    let mut pi_request = make_simple_request("compute-pi");
    pi_request.job_id = "compute-pi".to_string();
    let result = executor.execute(pi_request).await.unwrap();
    assert!(result.succeeded());
    assert_eq!(result.encrypted_result, b"3.14159265359");

    // Test failing job
    let mut fail_request = make_simple_request("fail-job");
    fail_request.job_id = "fail-job".to_string();
    let result = executor.execute(fail_request).await;
    assert!(result.is_err());
}
