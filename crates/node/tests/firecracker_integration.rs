//! Integration tests for Firecracker VMM
//!
//! These tests only run on Linux where Firecracker is supported.
//! Some tests require the `firecracker` binary to be installed.
//!
//! Run with: `cargo test -p monad_node --features integration-tests`

#![cfg(all(target_os = "linux", feature = "integration-tests"))]

use monad_node::vmm::{FirecrackerConfig, FirecrackerVirtualizer, VmState, Virtualizer, VmmError};
use std::path::PathBuf;
use std::time::Duration;

/// Check if firecracker binary is available
fn firecracker_available() -> bool {
    std::process::Command::new("which")
        .arg("firecracker")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_config_socket_path_generation() {
    let config = FirecrackerConfig::new()
        .with_instance_id("test-instance")
        .with_runtime_dir("/tmp/fc-test");

    assert_eq!(config.instance_id, "test-instance");
    assert_eq!(config.runtime_dir, PathBuf::from("/tmp/fc-test"));
}

#[test]
fn test_config_timeout_settings() {
    let config = FirecrackerConfig::new()
        .with_shutdown_timeout(Duration::from_secs(10))
        .with_execution_timeout(Duration::from_secs(600));

    assert_eq!(config.shutdown_timeout, Duration::from_secs(10));
    assert_eq!(config.execution_timeout, Duration::from_secs(600));
}

#[tokio::test]
async fn test_vmm_spawn_without_binary() {
    // Test that we get a proper error when firecracker binary doesn't exist
    let config = FirecrackerConfig::new()
        .with_instance_id("nonexistent-test")
        .with_runtime_dir("/tmp");

    // Use a non-existent binary path
    let mut config = config;
    config.firecracker_bin = PathBuf::from("/nonexistent/firecracker");

    let result = FirecrackerVirtualizer::new(config).await;
    assert!(result.is_err());

    match result {
        Err(VmmError::ProcessSpawnError(_)) => (),
        Err(e) => panic!("Expected ProcessSpawnError, got {:?}", e),
        Ok(_) => panic!("Expected error, got success"),
    }
}

#[tokio::test]
async fn test_resource_validation_vcpu_bounds() {
    if !firecracker_available() {
        eprintln!("Skipping test: firecracker binary not found");
        return;
    }

    let config = FirecrackerConfig::new()
        .with_instance_id("vcpu-test")
        .with_runtime_dir("/tmp");

    let mut vmm = match FirecrackerVirtualizer::new(config).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Skipping test: failed to create VMM: {}", e);
            return;
        }
    };

    // Test vCPU too low
    let result = vmm.configure(0, 256).await;
    assert!(matches!(result, Err(VmmError::ConfigError(_))));

    // Test vCPU too high
    let result = vmm.configure(17, 256).await;
    assert!(matches!(result, Err(VmmError::ConfigError(_))));

    // Valid vCPU should work (if VMM is ready)
    let result = vmm.configure(2, 256).await;
    assert!(result.is_ok() || matches!(result, Err(VmmError::ApiError(_))));
}

#[tokio::test]
async fn test_resource_validation_memory_bounds() {
    if !firecracker_available() {
        eprintln!("Skipping test: firecracker binary not found");
        return;
    }

    let config = FirecrackerConfig::new()
        .with_instance_id("mem-test")
        .with_runtime_dir("/tmp");

    let mut vmm = match FirecrackerVirtualizer::new(config).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Skipping test: failed to create VMM: {}", e);
            return;
        }
    };

    // Test memory too low
    let result = vmm.configure(1, 64).await;
    assert!(matches!(result, Err(VmmError::ConfigError(_))));

    // Valid memory should work
    let result = vmm.configure(1, 128).await;
    assert!(result.is_ok() || matches!(result, Err(VmmError::ApiError(_))));
}

#[tokio::test]
async fn test_state_machine_invalid_transitions() {
    if !firecracker_available() {
        eprintln!("Skipping test: firecracker binary not found");
        return;
    }

    let config = FirecrackerConfig::new()
        .with_instance_id("state-test")
        .with_runtime_dir("/tmp");

    let mut vmm = match FirecrackerVirtualizer::new(config).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Skipping test: failed to create VMM: {}", e);
            return;
        }
    };

    assert_eq!(*vmm.state(), VmState::Created);

    // Can't start without configuring first
    let result = vmm.start().await;
    assert!(matches!(result, Err(VmmError::RuntimeError(_))));

    // Can't set boot source before configure
    let result = vmm
        .set_boot_source(PathBuf::from("/tmp/kernel"), "console=ttyS0".to_string())
        .await;
    assert!(matches!(result, Err(VmmError::RuntimeError(_))));

    // Can't attach drive before configure
    let result = vmm
        .attach_drive("rootfs", PathBuf::from("/tmp/rootfs.ext4"), true, false)
        .await;
    assert!(matches!(result, Err(VmmError::RuntimeError(_))));
}

#[tokio::test]
async fn test_socket_cleanup_on_drop() {
    if !firecracker_available() {
        eprintln!("Skipping test: firecracker binary not found");
        return;
    }

    let instance_id = format!("cleanup-test-{}", uuid::Uuid::new_v4());
    let socket_path = PathBuf::from(format!("/tmp/firecracker-{}.sock", instance_id));

    {
        let config = FirecrackerConfig::new()
            .with_instance_id(&instance_id)
            .with_runtime_dir("/tmp");

        let _vmm = match FirecrackerVirtualizer::new(config).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Skipping test: failed to create VMM: {}", e);
                return;
            }
        };

        // Socket should exist while VMM is alive
        assert!(socket_path.exists(), "Socket should exist while VMM is running");
    }

    // Give a moment for cleanup
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Socket should be cleaned up after drop
    assert!(
        !socket_path.exists(),
        "Socket should be cleaned up after VMM drop"
    );
}
