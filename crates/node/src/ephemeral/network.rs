//! Network isolation for ephemeral builder VMs.
//!
//! Provides TAP device management and firewall rules to restrict VM network access
//! to only allowlisted destinations while blocking private IP ranges.

use async_trait::async_trait;
use std::net::ToSocketAddrs;
use std::process::Command;
use tracing::{debug, error, info, warn};

use super::{
    EgressEntry, NetworkError, NetworkIsolator, NetworkStats, Protocol, TapConfig,
    BLOCKED_IP_RANGES,
};

/// A resolved egress entry with IP address instead of hostname.
struct ResolvedEgress {
    ip: String,
    port: u16,
    protocol: Protocol,
}

/// Linux-based network isolator using TAP devices and nftables.
///
/// Requires CAP_NET_ADMIN capability or root privileges.
pub struct LinuxNetworkIsolator {
    /// Prefix for nftables chain names
    chain_prefix: String,
}

impl LinuxNetworkIsolator {
    /// Create a new Linux network isolator.
    pub fn new() -> Self {
        Self {
            chain_prefix: "ephemeral".to_string(),
        }
    }

    /// Create with a custom chain prefix.
    pub fn with_chain_prefix(chain_prefix: impl Into<String>) -> Self {
        Self {
            chain_prefix: chain_prefix.into(),
        }
    }

    /// Get the nftables chain name for a TAP device.
    fn chain_name(&self, tap_name: &str) -> String {
        format!("{}_{}", self.chain_prefix, tap_name.replace('-', "_"))
    }

    /// Resolve a hostname to IP addresses.
    fn resolve_hostname(&self, hostname: &str) -> Result<Vec<String>, NetworkError> {
        // Handle already-IP addresses
        if hostname.parse::<std::net::IpAddr>().is_ok() {
            return Ok(vec![hostname.to_string()]);
        }

        // Resolve hostname to IPs
        let addrs = format!("{}:443", hostname)
            .to_socket_addrs()
            .map_err(|e| NetworkError::DnsResolutionFailed(format!("{}: {}", hostname, e)))?;

        let ips: Vec<String> = addrs.map(|addr| addr.ip().to_string()).collect();

        if ips.is_empty() {
            return Err(NetworkError::DnsResolutionFailed(format!(
                "{}: no addresses found",
                hostname
            )));
        }

        debug!("Resolved {} to {:?}", hostname, ips);
        Ok(ips)
    }

    /// Execute a shell command and return success/failure.
    fn run_command(&self, cmd: &str, args: &[&str]) -> Result<(), NetworkError> {
        let output = Command::new(cmd)
            .args(args)
            .output()
            .map_err(NetworkError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Command failed: {} {:?}: {}", cmd, args, stderr);
            return Err(NetworkError::FirewallError(format!(
                "{} {:?} failed: {}",
                cmd, args, stderr
            )));
        }

        Ok(())
    }

    /// Create nftables rules for the TAP device.
    fn setup_nftables(
        &self,
        tap_name: &str,
        resolved_entries: &[ResolvedEgress],
    ) -> Result<(), NetworkError> {
        let chain = self.chain_name(tap_name);
        let egress_counter = self.egress_counter_name(tap_name);
        let ingress_counter = self.ingress_counter_name(tap_name);

        // Create table if it doesn't exist (idempotent)
        let _ = self.run_command("nft", &["add", "table", "inet", "ephemeral_filter"]);

        // Create named counters for traffic metering
        if let Err(e) = self.run_command(
            "nft",
            &[
                "add",
                "counter",
                "inet",
                "ephemeral_filter",
                &egress_counter,
            ],
        ) {
            warn!("Failed to create egress counter: {}", e);
        }
        if let Err(e) = self.run_command(
            "nft",
            &[
                "add",
                "counter",
                "inet",
                "ephemeral_filter",
                &ingress_counter,
            ],
        ) {
            warn!("Failed to create ingress counter: {}", e);
        }

        // Create chain for this TAP
        self.run_command(
            "nft",
            &[
                "add",
                "chain",
                "inet",
                "ephemeral_filter",
                &chain,
                "{ type filter hook forward priority 0; policy drop; }",
            ],
        )?;

        // Allow established/related connections with ingress counter (return traffic from external -> VM)
        self.run_command(
            "nft",
            &[
                "add",
                "rule",
                "inet",
                "ephemeral_filter",
                &chain,
                "oifname",
                tap_name,
                "ct",
                "state",
                "established,related",
                "counter",
                "name",
                &ingress_counter,
                "accept",
            ],
        )?;

        // Block RFC1918 and loopback ranges
        for range in BLOCKED_IP_RANGES {
            self.run_command(
                "nft",
                &[
                    "add",
                    "rule",
                    "inet",
                    "ephemeral_filter",
                    &chain,
                    "iifname",
                    tap_name,
                    "ip",
                    "daddr",
                    range,
                    "drop",
                ],
            )?;
        }

        // Allow traffic to allowlisted IPs with port/protocol filtering and egress counter
        for entry in resolved_entries {
            let port_str = entry.port.to_string();
            self.run_command(
                "nft",
                &[
                    "add",
                    "rule",
                    "inet",
                    "ephemeral_filter",
                    &chain,
                    "iifname",
                    tap_name,
                    "ip",
                    "daddr",
                    &entry.ip,
                    entry.protocol.as_str(),
                    "dport",
                    &port_str,
                    "counter",
                    "name",
                    &egress_counter,
                    "accept",
                ],
            )?;
        }

        // Allow DNS (UDP 53) for initial resolution with egress counter
        self.run_command(
            "nft",
            &[
                "add",
                "rule",
                "inet",
                "ephemeral_filter",
                &chain,
                "iifname",
                tap_name,
                "udp",
                "dport",
                "53",
                "counter",
                "name",
                &egress_counter,
                "accept",
            ],
        )?;

        // Log and drop everything else
        self.run_command(
            "nft",
            &[
                "add",
                "rule",
                "inet",
                "ephemeral_filter",
                &chain,
                "iifname",
                tap_name,
                "log",
                "prefix",
                &format!("\"[{} DROP] \"", tap_name),
                "drop",
            ],
        )?;

        info!("Applied nftables rules for {} with counters", tap_name);
        Ok(())
    }

    /// Remove nftables chain for a TAP device.
    fn teardown_nftables(&self, tap_name: &str) -> Result<(), NetworkError> {
        let chain = self.chain_name(tap_name);

        // Flush and delete chain
        let _ = self.run_command(
            "nft",
            &["flush", "chain", "inet", "ephemeral_filter", &chain],
        );
        let _ = self.run_command(
            "nft",
            &["delete", "chain", "inet", "ephemeral_filter", &chain],
        );

        // Delete named counters
        let egress_counter = format!("egress_{}", tap_name.replace('-', "_"));
        let ingress_counter = format!("ingress_{}", tap_name.replace('-', "_"));
        let _ = self.run_command(
            "nft",
            &[
                "delete",
                "counter",
                "inet",
                "ephemeral_filter",
                &egress_counter,
            ],
        );
        let _ = self.run_command(
            "nft",
            &[
                "delete",
                "counter",
                "inet",
                "ephemeral_filter",
                &ingress_counter,
            ],
        );

        debug!("Removed nftables chain and counters for {}", tap_name);
        Ok(())
    }

    /// Get the counter name for egress traffic.
    fn egress_counter_name(&self, tap_name: &str) -> String {
        format!("egress_{}", tap_name.replace('-', "_"))
    }

    /// Get the counter name for ingress traffic.
    fn ingress_counter_name(&self, tap_name: &str) -> String {
        format!("ingress_{}", tap_name.replace('-', "_"))
    }

    /// Query a named counter and return (packets, bytes).
    fn query_counter(&self, counter_name: &str) -> Result<(u64, u64), NetworkError> {
        let output = Command::new("nft")
            .args(["list", "counter", "inet", "ephemeral_filter", counter_name])
            .output()
            .map_err(NetworkError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to query counter {}: {}", counter_name, stderr);
            return Ok((0, 0));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        NetworkStats::parse_counter_output(&stdout).map_err(|e| {
            warn!("Failed to parse counter output for {}: {}", counter_name, e);
            NetworkError::FirewallError(format!("counter parse error: {}", e))
        })
    }
}

impl Default for LinuxNetworkIsolator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkIsolator for LinuxNetworkIsolator {
    async fn create_tap(&self, vm_id: &str) -> Result<TapConfig, NetworkError> {
        let config = TapConfig::for_vm(vm_id);

        // Create TAP device
        self.run_command("ip", &["tuntap", "add", &config.tap_name, "mode", "tap"])
            .map_err(|e| {
                NetworkError::TapCreationFailed(format!(
                    "Failed to create TAP {}: {}",
                    config.tap_name, e
                ))
            })?;

        // Configure IP address
        let cidr = format!("{}/30", config.host_ip);
        self.run_command("ip", &["addr", "add", &cidr, "dev", &config.tap_name])
            .map_err(|e| {
                // Clean up TAP on failure
                let _ = self.run_command("ip", &["link", "delete", &config.tap_name]);
                NetworkError::IpConfigFailed(format!(
                    "Failed to configure IP for {}: {}",
                    config.tap_name, e
                ))
            })?;

        // Bring interface up
        self.run_command("ip", &["link", "set", &config.tap_name, "up"])
            .map_err(|e| {
                // Clean up TAP on failure
                let _ = self.run_command("ip", &["link", "delete", &config.tap_name]);
                NetworkError::IpConfigFailed(format!(
                    "Failed to bring up {}: {}",
                    config.tap_name, e
                ))
            })?;

        // Enable IP forwarding for this interface
        let _ = self.run_command(
            "sysctl",
            &[
                "-w",
                &format!("net.ipv4.conf.{}.forwarding=1", config.tap_name),
            ],
        );

        info!(
            "Created TAP device {} with IP {}",
            config.tap_name, config.host_ip
        );
        Ok(config)
    }

    async fn apply_allowlist(
        &self,
        tap_name: &str,
        allowlist: &[EgressEntry],
    ) -> Result<(), NetworkError> {
        // Resolve all hostnames to IPs, preserving port/protocol
        let mut resolved_entries = Vec::new();
        for entry in allowlist {
            match self.resolve_hostname(&entry.host) {
                Ok(ips) => {
                    for ip in ips {
                        resolved_entries.push(ResolvedEgress {
                            ip,
                            port: entry.port,
                            protocol: entry.protocol,
                        });
                    }
                }
                Err(e) => {
                    warn!("Failed to resolve {}: {}", entry.host, e);
                    // Continue with other hosts rather than failing entirely
                }
            }
        }

        if resolved_entries.is_empty() && !allowlist.is_empty() {
            return Err(NetworkError::DnsResolutionFailed(
                "Could not resolve any allowlisted hosts".into(),
            ));
        }

        // Setup nftables rules
        self.setup_nftables(tap_name, &resolved_entries)?;

        info!(
            "Applied allowlist for {}: {} entries -> {} rules",
            tap_name,
            allowlist.len(),
            resolved_entries.len()
        );
        Ok(())
    }

    async fn get_network_stats(&self, tap_name: &str) -> Result<NetworkStats, NetworkError> {
        let egress_counter = self.egress_counter_name(tap_name);
        let ingress_counter = self.ingress_counter_name(tap_name);

        // Query egress counter (VM -> external)
        let (egress_packets, egress_bytes) = match self.query_counter(&egress_counter) {
            Ok(stats) => stats,
            Err(e) => {
                warn!("Failed to query egress counter for {}: {}", tap_name, e);
                (0, 0)
            }
        };

        // Query ingress counter (external -> VM)
        let (ingress_packets, ingress_bytes) = match self.query_counter(&ingress_counter) {
            Ok(stats) => stats,
            Err(e) => {
                warn!("Failed to query ingress counter for {}: {}", tap_name, e);
                (0, 0)
            }
        };

        debug!(
            "Network stats for {}: egress={}B/{}pkts, ingress={}B/{}pkts",
            tap_name, egress_bytes, egress_packets, ingress_bytes, ingress_packets
        );

        Ok(NetworkStats::new(
            egress_bytes,
            ingress_bytes,
            egress_packets,
            ingress_packets,
        ))
    }

    async fn teardown(&self, tap_name: &str) -> Result<(), NetworkError> {
        // Remove firewall rules first
        if let Err(e) = self.teardown_nftables(tap_name) {
            warn!("Failed to remove nftables rules for {}: {}", tap_name, e);
        }

        // Delete TAP device
        self.run_command("ip", &["link", "delete", tap_name])
            .map_err(|e| {
                NetworkError::TeardownFailed(format!("Failed to delete TAP {}: {}", tap_name, e))
            })?;

        info!("Torn down TAP device {}", tap_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_name_generation() {
        let isolator = LinuxNetworkIsolator::new();
        assert_eq!(
            isolator.chain_name("tap-build-123"),
            "ephemeral_tap_build_123"
        );
    }

    #[test]
    fn resolve_ip_address() {
        let isolator = LinuxNetworkIsolator::new();
        let ips = isolator.resolve_hostname("1.1.1.1").unwrap();
        assert_eq!(ips, vec!["1.1.1.1"]);
    }

    #[test]
    fn tap_config_generation() {
        let config = TapConfig::for_vm("build-abc12345");
        assert_eq!(config.tap_name, "tap-abc12345");
        assert_eq!(config.netmask, "255.255.255.252");
    }
}
