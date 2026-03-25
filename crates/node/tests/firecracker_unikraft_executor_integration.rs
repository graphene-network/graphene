//! Full Firecracker + Unikraft + executor E2E test.
//!
//! Requires:
//! - Linux with /dev/kvm available
//! - firecracker binary in PATH
//! - cpio binary in PATH (for initrd creation)
//! - Prebuilt Unikraft kernel at ~/.opencapsule/cache/kernels/python-3.12_fc-x86_64
//!
//! Run with: `cargo test -p opencapsule_node --features e2e-tests --test firecracker_unikraft_executor_integration`
//!
//! This test runs in the e2e-test.yml workflow which builds kernels beforehand.

#![cfg(all(target_os = "linux", feature = "e2e-tests"))]

use opencapsule_node::cache::MockBuildCache;
use opencapsule_node::crypto::{ChannelKeys, CryptoProvider, DefaultCryptoProvider, EncryptedBlob};
use opencapsule_node::executor::drive::linux::LinuxDriveBuilder;
use opencapsule_node::executor::output::DefaultOutputProcessor;
use opencapsule_node::executor::runner::{FirecrackerRunner, FirecrackerRunnerConfig};
use opencapsule_node::executor::{ExecutionRequest, JobExecutor};
use opencapsule_node::p2p::messages::{JobManifest, ResultDeliveryMode};
use opencapsule_node::p2p::mock::MockOpenCapsuleNode;
use opencapsule_node::p2p::protocol::types::JobAssets;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use uuid::Uuid;

struct EnvGuard {
    key: &'static str,
    prev: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let prev = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.prev {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn command_available(cmd: &str, arg: &str) -> bool {
    Command::new(cmd)
        .arg(arg)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn kvm_available() -> bool {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .is_ok()
}

fn kernel_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".opencapsule/cache/kernels/python-3.12_fc-x86_64");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn create_test_channel_keys() -> (ChannelKeys, [u8; 32], [u8; 32]) {
    let user_secret = [1u8; 32];
    let worker_secret = [2u8; 32];
    let user_signing = ed25519_dalek::SigningKey::from_bytes(&user_secret);
    let worker_signing = ed25519_dalek::SigningKey::from_bytes(&worker_secret);
    let user_public = user_signing.verifying_key().to_bytes();
    let worker_public = worker_signing.verifying_key().to_bytes();
    let channel_pda = [3u8; 32];

    let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();
    (user_keys, user_public, worker_secret)
}

#[tokio::test]
async fn firecracker_unikraft_executor_runs_python_job() {
    assert!(
        command_available("firecracker", "--version"),
        "firecracker not available in PATH"
    );
    assert!(
        command_available("cpio", "--version"),
        "cpio not available in PATH"
    );
    assert!(kvm_available(), "/dev/kvm not available");
    let _kernel_path = kernel_path()
        .expect("prebuilt kernel not found at ~/.opencapsule/cache/kernels/python-3.12_fc-x86_64");

    let _keep_serial = EnvGuard::set("OPENCAPSULE_KEEP_SERIAL_LOG", "1");

    let (user_keys, user_public, worker_secret) = create_test_channel_keys();
    let crypto = Arc::new(DefaultCryptoProvider);

    let code = b"import sys\nprint('benchmark-run')\nsys.exit(0)\n";
    let job_id = Uuid::new_v4().to_string();
    let encrypted = crypto
        .encrypt_job_blob(
            code,
            &user_keys,
            &job_id,
            opencapsule_node::crypto::EncryptionDirection::Input,
        )
        .expect("encrypt code");

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 30_000,
        runtime: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: std::collections::HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let assets = JobAssets::inline(encrypted.to_bytes(), None);
    let request = ExecutionRequest::new(
        job_id.clone(),
        manifest,
        assets,
        [0u8; 32],
        [3u8; 32],
        user_public,
        ResultDeliveryMode::Sync,
    );

    let runtime_dir = tempfile::tempdir().expect("runtime dir");
    let runner =
        FirecrackerRunner::new(FirecrackerRunnerConfig::new().with_runtime_dir(runtime_dir.path()));

    let executor = opencapsule_node::executor::DefaultJobExecutor::new(
        Arc::new(LinuxDriveBuilder::with_defaults()),
        Arc::new(runner),
        Arc::new(DefaultOutputProcessor::new(Arc::clone(&crypto))),
        Arc::clone(&crypto),
        Arc::new(MockOpenCapsuleNode::new()),
        Arc::new(MockBuildCache::new()),
        worker_secret,
    );

    let result = executor.execute(request).await.expect("execute job");
    assert_eq!(result.exit_code, 0, "expected exit_code=0");
    assert!(result.duration.as_millis() > 0, "duration should be >0");

    let encrypted_stdout =
        EncryptedBlob::from_bytes(&result.encrypted_stdout).expect("stdout encrypted blob");
    let stdout = crypto
        .decrypt_job_blob(
            &encrypted_stdout,
            &user_keys,
            &job_id,
            opencapsule_node::crypto::EncryptionDirection::Output,
        )
        .expect("decrypt stdout");
    let stdout_str = String::from_utf8_lossy(&stdout);
    assert!(
        stdout_str.contains("benchmark-run"),
        "stdout missing benchmark-run: {}",
        stdout_str
    );
}
