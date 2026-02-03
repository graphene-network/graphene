use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// Network protocol for egress rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Protocol {
    /// TCP protocol (default).
    #[default]
    Tcp,
    /// UDP protocol.
    Udp,
}

impl Protocol {
    /// Returns the protocol as a lowercase string for nftables.
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<&str> for Protocol {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "udp" => Protocol::Udp,
            _ => Protocol::Tcp,
        }
    }
}

impl From<String> for Protocol {
    fn from(s: String) -> Self {
        Protocol::from(s.as_str())
    }
}

/// An allowed egress destination with port/protocol constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressEntry {
    /// Hostname or IP address.
    pub host: String,
    /// Port number (default: 443).
    pub port: u16,
    /// Protocol (default: TCP).
    pub protocol: Protocol,
}

impl EgressEntry {
    /// Create a new egress entry with specified host, port, and protocol.
    pub fn new(host: impl Into<String>, port: u16, protocol: Protocol) -> Self {
        Self {
            host: host.into(),
            port,
            protocol,
        }
    }

    /// Create an HTTPS egress entry (TCP port 443).
    pub fn https(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 443,
            protocol: Protocol::Tcp,
        }
    }

    /// Create a TCP egress entry with custom port.
    pub fn tcp(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            protocol: Protocol::Tcp,
        }
    }

    /// Create a UDP egress entry with custom port.
    pub fn udp(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            protocol: Protocol::Udp,
        }
    }
}

impl From<&str> for EgressEntry {
    fn from(host: &str) -> Self {
        Self {
            host: host.to_string(),
            port: 443,
            protocol: Protocol::Tcp,
        }
    }
}

impl From<String> for EgressEntry {
    fn from(host: String) -> Self {
        Self {
            host,
            port: 443,
            protocol: Protocol::Tcp,
        }
    }
}

impl From<&crate::p2p::messages::EgressRule> for EgressEntry {
    fn from(rule: &crate::p2p::messages::EgressRule) -> Self {
        Self {
            host: rule.host.clone(),
            port: rule.port,
            protocol: Protocol::from(rule.protocol.as_str()),
        }
    }
}

impl From<crate::p2p::messages::EgressRule> for EgressEntry {
    fn from(rule: crate::p2p::messages::EgressRule) -> Self {
        Self {
            host: rule.host,
            port: rule.port,
            protocol: Protocol::from(rule.protocol.as_str()),
        }
    }
}

/// Resource limits for ephemeral builder VMs.
///
/// Default values follow Whitepaper §4.2 specifications:
/// - 5 minute timeout
/// - 4 GB memory
/// - 10 GB disk
/// - 2 vCPUs
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum build duration before timeout
    pub timeout: Duration,
    /// Memory limit in MiB
    pub memory_mib: u16,
    /// Disk limit in MiB
    pub disk_mib: u32,
    /// Number of virtual CPUs
    pub vcpu: u8,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300), // 5 minutes
            memory_mib: 4096,                  // 4 GB
            disk_mib: 10240,                   // 10 GB
            vcpu: 2,
        }
    }
}

impl ResourceLimits {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn memory_mib(mut self, memory_mib: u16) -> Self {
        self.memory_mib = memory_mib;
        self
    }

    pub fn disk_mib(mut self, disk_mib: u32) -> Self {
        self.disk_mib = disk_mib;
        self
    }

    pub fn vcpu(mut self, vcpu: u8) -> Self {
        self.vcpu = vcpu;
        self
    }
}

/// Request to build a unikernel from source.
#[derive(Debug, Clone)]
pub struct BuildRequest {
    /// Unique identifier for this build
    pub build_id: String,
    /// Contents of the Dockerfile
    pub dockerfile: String,
    /// Optional Kraftfile for Unikraft configuration
    pub kraftfile: Option<String>,
    /// Path to tarball containing source code
    pub code_tarball: PathBuf,
    /// List of allowed egress destinations with port/protocol
    pub egress_allowlist: Vec<EgressEntry>,
    /// Resource limits for this build
    pub limits: ResourceLimits,
}

impl BuildRequest {
    pub fn new(build_id: impl Into<String>, dockerfile: impl Into<String>) -> Self {
        Self {
            build_id: build_id.into(),
            dockerfile: dockerfile.into(),
            kraftfile: None,
            code_tarball: PathBuf::new(),
            egress_allowlist: Vec::new(),
            limits: ResourceLimits::default(),
        }
    }

    pub fn kraftfile(mut self, kraftfile: impl Into<String>) -> Self {
        self.kraftfile = Some(kraftfile.into());
        self
    }

    pub fn code_tarball(mut self, path: impl Into<PathBuf>) -> Self {
        self.code_tarball = path.into();
        self
    }

    pub fn egress_allowlist(mut self, allowlist: Vec<EgressEntry>) -> Self {
        self.egress_allowlist = allowlist;
        self
    }

    pub fn limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = limits;
        self
    }
}

/// Result of a successful build.
#[derive(Debug, Clone)]
pub struct BuildResult {
    /// Path to the built unikernel binary
    pub unikernel_path: PathBuf,
    /// Total build duration
    pub build_duration: Duration,
    /// Build logs (stdout + stderr)
    pub logs: String,
    /// Cache key for this build (content-addressable hash)
    pub cache_key: String,
}

/// Configuration for a TAP network device.
#[derive(Debug, Clone)]
pub struct TapConfig {
    /// Name of the TAP device (e.g., "tap-build-abc123")
    pub tap_name: String,
    /// IP address assigned to the TAP device (host side)
    pub host_ip: String,
    /// IP address for the guest VM
    pub guest_ip: String,
    /// Netmask (e.g., "255.255.255.0")
    pub netmask: String,
    /// Gateway IP for the guest
    pub gateway: String,
}

impl TapConfig {
    /// Create a new TapConfig with the given VM ID.
    ///
    /// Uses a /30 subnet allocation for minimal IP address waste.
    /// The host gets .1 and guest gets .2 within the subnet.
    pub fn for_vm(vm_id: &str) -> Self {
        // Use last 8 chars of vm_id for uniqueness
        let short_id = if vm_id.len() > 8 {
            &vm_id[vm_id.len() - 8..]
        } else {
            vm_id
        };

        Self {
            tap_name: format!("tap-{}", short_id),
            host_ip: "172.16.0.1".to_string(),
            guest_ip: "172.16.0.2".to_string(),
            netmask: "255.255.255.252".to_string(), // /30
            gateway: "172.16.0.1".to_string(),
        }
    }
}

/// Configuration for the ephemeral builder.
#[derive(Debug, Clone)]
pub struct EphemeralBuilderConfig {
    /// Path to Firecracker binary
    pub firecracker_bin: PathBuf,
    /// Path to builder kernel (vmlinux-builder)
    pub kernel_path: PathBuf,
    /// Path to builder rootfs (rootfs-builder.ext4)
    pub rootfs_path: PathBuf,
    /// Directory for runtime files (sockets, logs, temp drives)
    pub runtime_dir: PathBuf,
    /// Default resource limits
    pub default_limits: ResourceLimits,
}

impl EphemeralBuilderConfig {
    pub fn new(
        firecracker_bin: impl Into<PathBuf>,
        kernel_path: impl Into<PathBuf>,
        rootfs_path: impl Into<PathBuf>,
        runtime_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            firecracker_bin: firecracker_bin.into(),
            kernel_path: kernel_path.into(),
            rootfs_path: rootfs_path.into(),
            runtime_dir: runtime_dir.into(),
            default_limits: ResourceLimits::default(),
        }
    }

    pub fn default_limits(mut self, limits: ResourceLimits) -> Self {
        self.default_limits = limits;
        self
    }
}

/// Default package mirror hostnames for the egress allowlist.
///
/// These are the hostnames only - use `default_egress_allowlist()` to get
/// the full `EgressEntry` list with port/protocol.
pub const DEFAULT_EGRESS_HOSTS: &[&str] = &[
    // Python
    "pypi.org",
    "files.pythonhosted.org",
    // Node.js
    "registry.npmjs.org",
    // Rust
    "crates.io",
    "static.crates.io",
    // Go
    "proxy.golang.org",
    // GitHub (for git dependencies)
    "github.com",
    "raw.githubusercontent.com",
];

/// Returns the default egress allowlist as `EgressEntry` values.
///
/// All entries use TCP port 443 (HTTPS).
pub fn default_egress_allowlist() -> Vec<EgressEntry> {
    DEFAULT_EGRESS_HOSTS
        .iter()
        .map(|&host| EgressEntry::https(host))
        .collect()
}

/// Blocked private IP ranges (RFC1918 + loopback).
pub const BLOCKED_IP_RANGES: &[&str] = &[
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "127.0.0.0/8",
];

/// Network traffic statistics captured from nftables counters.
///
/// These statistics are queried from nftables named counters before VM teardown
/// to measure actual network usage for cost calculation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkStats {
    /// Bytes transmitted from VM to external networks (egress).
    pub egress_bytes: u64,
    /// Bytes received from external networks to VM (ingress).
    pub ingress_bytes: u64,
    /// Packets transmitted from VM (for debugging/monitoring).
    pub egress_packets: u64,
    /// Packets received by VM (for debugging/monitoring).
    pub ingress_packets: u64,
}

impl NetworkStats {
    /// Creates a new NetworkStats with the given values.
    pub fn new(
        egress_bytes: u64,
        ingress_bytes: u64,
        egress_packets: u64,
        ingress_packets: u64,
    ) -> Self {
        Self {
            egress_bytes,
            ingress_bytes,
            egress_packets,
            ingress_packets,
        }
    }

    /// Parses nftables counter output to extract packets and bytes.
    ///
    /// Expected format: `counter <name> { packets <N> bytes <M> }`
    ///
    /// # Returns
    /// A tuple of (packets, bytes) on success, or an error message on failure.
    pub fn parse_counter_output(output: &str) -> Result<(u64, u64), String> {
        // Look for the pattern: packets <N> bytes <M>
        let packets = output
            .split_whitespace()
            .skip_while(|&s| s != "packets")
            .nth(1)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| "failed to parse packets from counter output".to_string())?;

        let bytes = output
            .split_whitespace()
            .skip_while(|&s| s != "bytes")
            .nth(1)
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| "failed to parse bytes from counter output".to_string())?;

        Ok((packets, bytes))
    }
}
