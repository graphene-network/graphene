use super::types::{Virtualizer, VmmError};
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;

// Behaviors we can simulate for testing
#[derive(Clone)]
pub enum MockBehavior {
    HappyPath,
    BootFailure,  // Fails immediately on start()
    KernelPanic,  // Starts, runs for 1s, then returns Error
    InfiniteLoop, // Never exits (tests your timeout logic)
}

pub struct MockVirtualizer {
    behavior: MockBehavior,
    is_running: bool,
    config_set: bool,
}

impl MockVirtualizer {
    pub fn new(behavior: MockBehavior) -> Self {
        Self {
            behavior,
            is_running: false,
            config_set: false,
        }
    }
}

#[async_trait]
impl Virtualizer for MockVirtualizer {
    async fn configure(&mut self, _vcpu: u8, _mem: u16) -> Result<(), VmmError> {
        println!("🤖 [MOCK] Configured VCPU/RAM");
        self.config_set = true;
        Ok(())
    }

    async fn set_boot_source(&mut self, path: PathBuf, _args: String) -> Result<(), VmmError> {
        if !path.exists() {
            return Err(VmmError::ConfigError("Kernel path does not exist".into()));
        }
        println!("🤖 [MOCK] Kernel set to: {:?}", path);
        Ok(())
    }

    async fn attach_drive(
        &mut self,
        id: &str,
        path: PathBuf,
        _root: bool,
        _ro: bool,
    ) -> Result<(), VmmError> {
        println!("🤖 [MOCK] Drive '{}' attached from {:?}", id, path);
        Ok(())
    }

    async fn start(&mut self) -> Result<(), VmmError> {
        if !self.config_set {
            return Err(VmmError::BootError("Machine not configured".into()));
        }

        match self.behavior {
            MockBehavior::BootFailure => {
                println!("🤖 [MOCK] Simulating Boot Failure...");
                Err(VmmError::BootError("Simulated boot failure".into()))
            }
            _ => {
                println!("🤖 [MOCK] System Starting...");
                self.is_running = true;
                Ok(())
            }
        }
    }

    async fn wait(&mut self) -> Result<(), VmmError> {
        if !self.is_running {
            return Ok(());
        }

        match self.behavior {
            MockBehavior::HappyPath => {
                sleep(Duration::from_millis(500)).await;
                println!("🤖 [MOCK] VM Exited Successfully");
                Ok(())
            }
            MockBehavior::KernelPanic => {
                sleep(Duration::from_millis(300)).await;
                println!("🤖 [MOCK] 🔥 KERNEL PANIC 🔥");
                Err(VmmError::Crash("Simulated Kernel Panic".into()))
            }
            MockBehavior::InfiniteLoop => {
                // Sleep forever (caller must implement timeout)
                std::future::pending::<()>().await;
                Ok(())
            }
            _ => Ok(()), // Already handled in start
        }
    }

    async fn shutdown(&mut self) -> Result<(), VmmError> {
        println!("🤖 [MOCK] Force Kill Signal Sent");
        self.is_running = false;
        Ok(())
    }
}
