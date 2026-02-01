use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;
use std::fs;

// Configuration Constants
const KERNEL_PATH: &str = "./resources/vmlinux";
const ROOTFS_PATH: &str = "./resources/rootfs.ext4";
const FIRECRACKER_BIN: &str = "firecracker"; // Ensure this is in your $PATH
const SOCK_PATH: &str = "/tmp/firecracker.socket";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🦀 Talos Worker Node Starting...");

    // 1. CLEANUP: Remove old socket if it exists
    if std::path::Path::new(SOCK_PATH).exists() {
        fs::remove_file(SOCK_PATH)?;
    }

    // 2. THE "BUILDER" PHASE (Mocked for PoC)
    // In the real version, this generates the code.ext4 from the input script.
    // Here, we verify our "Base Layers" exist.
    check_resources()?;

    // 3. SPAWN FIRECRACKER PROCESS
    // We launch the VMM in the background, listening on the Unix socket.
    println!("🔥 Spawning Firecracker VMM...");
    let mut vmm_process = Command::new(FIRECRACKER_BIN)
        .arg("--api-sock")
        .arg(SOCK_PATH)
        .stdout(Stdio::inherit()) // Pipe VMM logs to our console
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Failed to start Firecracker. Is it installed?");

    // Give it a moment to initialize the socket
    sleep(Duration::from_millis(500)).await;

    // 4. CONFIGURE THE MACHINE (The "Sandwich")
    // We use a helper client to talk to the socket.
    println!("⚙️ Configuring VM Layers...");
    let client = FirecrackerClient::new(SOCK_PATH.to_string());

    // A. Set Machine Specs (1 vCPU, 128MB RAM)
    client.configure_machine(1, 128).await?;

    // B. Mount the Kernel (Layer 1)
    // "boot_args" tells the kernel to use the serial console so we can see output.
    client.set_boot_source(
        KERNEL_PATH,
        "console=ttyS0 reboot=k panic=1 pci=off"
    ).await?;

    // C. Mount the RootFS (Layer 2)
    client.attach_drive("rootfs", ROOTFS_PATH, true, false).await?;

    // (Future: D. Mount the Code Drive - Layer 3)
    // client.attach_drive("user_code", "./code.ext4", false, true).await?;

    // 5. IGNITION
    println!("🚀 Booting JIT Unikernel...");
    client.instance_start().await?;

    println!("✅ VM is running! (Press Ctrl+C to stop host, VM runs until completion)");

    // Wait for the VMM process to end (or user interrupt)
    vmm_process.wait()?;

    Ok(())
}

// --- Helper Utilities (The "SDK" Wrapper) ---

struct FirecrackerClient {
    socket_path: String,
    client: reqwest::Client,
}

impl FirecrackerClient {
    fn new(socket_path: String) -> Self {
        // Note: In a production Rust binary, you would use `hyper` with `unix_socket`.
        // For this PoC, we will shell out to `curl` for the config commands because
        // configuring async unix sockets in Rust requires verbose boilerplate (hyper-util).
        // This keeps the PoC readable and robust.
        Self {
            socket_path,
            client: reqwest::Client::new()
        }
    }

    async fn configure_machine(&self, vcpu: u8, mem_mib: u16) -> Result<(), Box<dyn std::error::Error>> {
        let json = format!(r#"{{ "vcpu_count": {}, "mem_size_mib": {} }}"#, vcpu, mem_mib);
        self.send_command("machine-config", &json).await
    }

    async fn set_boot_source(&self, kernel_path: &str, boot_args: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Get absolute path for Firecracker
        let abs_kernel = fs::canonicalize(kernel_path)?.to_string_lossy().to_string();

        let json = format!(
            r#"{{ "kernel_image_path": "{}", "boot_args": "{}" }}"#,
            abs_kernel, boot_args
        );
        self.send_command("boot-source", &json).await
    }

    async fn attach_drive(&self, drive_id: &str, path: &str, is_root: bool, is_read_only: bool) -> Result<(), Box<dyn std::error::Error>> {
        let abs_path = fs::canonicalize(path)?.to_string_lossy().to_string();

        let json = format!(
            r#"{{ "drive_id": "{}", "path_on_host": "{}", "is_root_device": {}, "is_read_only": {} }}"#,
            drive_id, abs_path, is_root, is_read_only
        );
        // Drives are PUT to /drives/{drive_id}
        self.send_command(&format!("drives/{}", drive_id), &json).await
    }

    async fn instance_start(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.send_command("actions", r#"{ "action_type": "InstanceStart" }"#).await
    }

    // The "curl" wrapper for simplicity in PoC
    async fn send_command(&self, endpoint: &str, json_body: &str) -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new("curl")
            .arg("--unix-socket")
            .arg(&self.socket_path)
            .arg("-i")
            .arg("-X")
            .arg("PUT")
            .arg(format!("http://localhost/{}", endpoint))
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-d")
            .arg(json_body)
            .output()?;

        if !status.status.success() {
            return Err(format!("Firecracker Error: {:?}", String::from_utf8_lossy(&status.stdout)).into());
        }
        Ok(())
    }
}

fn check_resources() -> Result<(), Box<dyn std::error::Error>> {
    if !std::path::Path::new(KERNEL_PATH).exists() {
        return Err("Missing 'vmlinux'. Download a hello-world kernel first.".into());
    }
    if !std::path::Path::new(ROOTFS_PATH).exists() {
        return Err("Missing 'rootfs.ext4'. Download a hello-world rootfs first.".into());
    }
    Ok(())
}

