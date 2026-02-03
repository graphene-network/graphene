//! Network isolation for ephemeral builder VMs.
//!
//! Provides TAP device management and firewall rules to restrict VM network access
//! to only allowlisted destinations while blocking private IP ranges.

use async_trait::async_trait;
use std::net::ToSocketAddrs;
use std::process::Command;
use tracing::{debug, error, info, warn};

use super::{EgressEntry, NetworkError, NetworkIsolator, TapConfig, BLOCKED_IP_RANGES};

/// A resolved egress entry with IP address instead of hostname.
struct ResolvedEgress {
    ip: String,
    port: u16,
    protocol: String,
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

        // Create table if it doesn't exist (idempotent)
        let _ = self.run_command("nft", &["add", "table", "inet", "ephemeral_filter"]);

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

        // Allow established connections (return traffic)
        self.run_command(
            "nft",
            &[
                "add",
                "rule",
                "inet",
                "ephemeral_filter",
                &chain,
                "ct",
                "state",
                "established,related",
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

        // Allow traffic to allowlisted IPs with port/protocol filtering
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
                    &entry.protocol,
                    "dport",
                    &port_str,
                    "accept",
                ],
            )?;
        }

        // Allow DNS (UDP 53) for initial resolution
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

        info!("Applied nftables rules for {}", tap_name);
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

        debug!("Removed nftables chain for {}", tap_name);
        Ok(())
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
                            protocol: entry.protocol.clone(),
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
