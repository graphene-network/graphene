use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use monad_node::executor::runner::{FirecrackerRunner, FirecrackerRunnerConfig, VmmRunner};
use monad_node::p2p::messages::JobManifest;
use monad_node::vmm::{FirecrackerConfig, Virtualizer, VmmError};

struct TestVirtualizer {
    socket_path: std::path::PathBuf,
}

#[async_trait::async_trait]
impl Virtualizer for TestVirtualizer {
    async fn configure(&mut self, _vcpu: u8, _mem_mib: u16) -> Result<(), VmmError> {
        Ok(())
    }

    async fn set_boot_source(
        &mut self,
        _kernel_path: std::path::PathBuf,
        _boot_args: String,
        _initrd_path: Option<std::path::PathBuf>,
    ) -> Result<(), VmmError> {
        Ok(())
    }

    async fn attach_drive(
        &mut self,
        _drive_id: &str,
        _path: std::path::PathBuf,
        _is_root: bool,
        _read_only: bool,
    ) -> Result<(), VmmError> {
        Ok(())
    }

    async fn start(&mut self) -> Result<(), VmmError> {
        Ok(())
    }

    async fn wait(&mut self) -> Result<(), VmmError> {
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), VmmError> {
        Ok(())
    }
}

impl Drop for TestVirtualizer {
    fn drop(&mut self) {
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

#[derive(Default)]
struct RuntimeState {
    runtime_exists: bool,
    log_exists: bool,
    log_path: Option<std::path::PathBuf>,
    socket_path: Option<std::path::PathBuf>,
}

#[tokio::test]
async fn firecracker_runner_creates_and_cleans_runtime_paths() {
    let tempdir = tempfile::tempdir().unwrap();
    let runtime_dir = tempdir.path().join("runtime");

    let state = Arc::new(Mutex::new(RuntimeState::default()));
    let state_clone = Arc::clone(&state);

    let runner = FirecrackerRunner::with_virtualizer_factory(
        FirecrackerRunnerConfig::new().with_runtime_dir(&runtime_dir),
        move |config: FirecrackerConfig| {
            let state = Arc::clone(&state_clone);
            async move {
                let socket_path = config
                    .runtime_dir
                    .join(format!("firecracker-{}.sock", config.instance_id));

                if let Some(parent) = socket_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::File::create(&socket_path)?;

                let log_path = config
                    .log_path
                    .clone()
                    .ok_or_else(|| VmmError::RuntimeError("log path missing".to_string()))?;

                let mut guard = state.lock().unwrap();
                guard.runtime_exists = config.runtime_dir.exists();
                guard.log_exists = log_path.exists();
                guard.log_path = Some(log_path);
                guard.socket_path = Some(socket_path.clone());

                Ok(TestVirtualizer { socket_path })
            }
        },
    );

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 1000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let kernel_path = tempdir.path().join("kernel");
    let drive_path = tempdir.path().join("drive");
    std::fs::write(&kernel_path, b"kernel").unwrap();
    std::fs::write(&drive_path, b"drive").unwrap();

    let result = runner
        .run(&kernel_path, &drive_path, &manifest, "console=ttyS0")
        .await;

    assert!(result.is_ok());

    let guard = state.lock().unwrap();
    assert!(guard.runtime_exists);
    assert!(guard.log_exists);

    let log_path = guard.log_path.clone().unwrap();
    let socket_path = guard.socket_path.clone().unwrap();
    drop(guard);

    assert!(runtime_dir.exists());
    assert!(!log_path.exists());
    assert!(!socket_path.exists());
}
