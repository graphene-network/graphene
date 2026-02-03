//! Integration tests for builder VM network isolation.
//!
//! These tests verify that nftables rules are correctly created and enforced
//! for ephemeral builder VMs.
//!
//! Run with: `sudo cargo test -p monad_node --features integration-tests -- ephemeral_network`
//!
//! Requirements:
//! - Linux with nftables (`nft` command)
//! - CAP_NET_ADMIN capability or root privileges
//! - Feature flag: `--features integration-tests`

#![cfg(all(feature = "integration-tests", target_os = "linux"))]

use monad_node::ephemeral::{
    default_egress_allowlist, EgressEntry, LinuxNetworkIsolator, NetworkIsolator,
};
use std::process::Command;

/// Helper to check if we have nftables available and sufficient permissions.
fn check_nftables_available() -> bool {
    Command::new("nft")
        .arg("list")
        .arg("tables")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Helper to check if a specific chain exists in nftables.
fn chain_exists(chain_name: &str) -> bool {
    let output = Command::new("nft")
        .args(["list", "chain", "inet", "ephemeral_filter", chain_name])
        .output();

    output.map(|o| o.status.success()).unwrap_or(false)
}

/// Helper to get the rules in a chain.
fn get_chain_rules(chain_name: &str) -> Option<String> {
    let output = Command::new("nft")
        .args(["list", "chain", "inet", "ephemeral_filter", chain_name])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

mod test_nftables_rules {
    use super::*;

    #[tokio::test]
    async fn test_chain_created_for_tap() {
        if !check_nftables_available() {
            eprintln!("Skipping test: nftables not available or no permissions");
            return;
        }

        let isolator = LinuxNetworkIsolator::new();
        let vm_id = "test-chain-create";

        // Create TAP - this may fail without root, which is expected
        let tap_result = isolator.create_tap(vm_id).await;
        if tap_result.is_err() {
            eprintln!("Skipping test: cannot create TAP device (need root)");
            return;
        }
        let tap_config = tap_result.unwrap();

        // Apply allowlist
        let allowlist = vec![EgressEntry::https("example.com")];
        let result = isolator
            .apply_allowlist(&tap_config.tap_name, &allowlist)
            .await;

        if result.is_err() {
            // Clean up TAP and skip
            let _ = isolator.teardown(&tap_config.tap_name).await;
            eprintln!("Skipping test: cannot apply nftables rules");
            return;
        }

        // Verify chain was created
        let chain_name = format!("ephemeral_{}", tap_config.tap_name.replace('-', "_"));
        assert!(
            chain_exists(&chain_name),
            "Chain {} should exist",
            chain_name
        );

        // Clean up
        let _ = isolator.teardown(&tap_config.tap_name).await;

        // Verify chain was removed
        assert!(
            !chain_exists(&chain_name),
            "Chain {} should be removed after teardown",
            chain_name
        );
    }

    #[tokio::test]
    async fn test_rules_block_rfc1918_ranges() {
        if !check_nftables_available() {
            eprintln!("Skipping test: nftables not available or no permissions");
            return;
        }

        let isolator = LinuxNetworkIsolator::new();
        let vm_id = "test-rfc1918";

        let tap_result = isolator.create_tap(vm_id).await;
        if tap_result.is_err() {
            eprintln!("Skipping test: cannot create TAP device (need root)");
            return;
        }
        let tap_config = tap_result.unwrap();

        let allowlist = vec![EgressEntry::https("example.com")];
        if isolator
            .apply_allowlist(&tap_config.tap_name, &allowlist)
            .await
            .is_err()
        {
            let _ = isolator.teardown(&tap_config.tap_name).await;
            eprintln!("Skipping test: cannot apply nftables rules");
            return;
        }

        // Get rules and verify RFC1918 blocks are present
        let chain_name = format!("ephemeral_{}", tap_config.tap_name.replace('-', "_"));
        let rules = get_chain_rules(&chain_name).unwrap_or_default();

        // Check that private IP ranges are blocked
        assert!(
            rules.contains("10.0.0.0/8") && rules.contains("drop"),
            "Should block 10.0.0.0/8"
        );
        assert!(
            rules.contains("172.16.0.0/12") && rules.contains("drop"),
            "Should block 172.16.0.0/12"
        );
        assert!(
            rules.contains("192.168.0.0/16") && rules.contains("drop"),
            "Should block 192.168.0.0/16"
        );
        assert!(
            rules.contains("127.0.0.0/8") && rules.contains("drop"),
            "Should block 127.0.0.0/8"
        );

        // Clean up
        let _ = isolator.teardown(&tap_config.tap_name).await;
    }

    #[tokio::test]
    async fn test_rules_allow_specific_port() {
        if !check_nftables_available() {
            eprintln!("Skipping test: nftables not available or no permissions");
            return;
        }

        let isolator = LinuxNetworkIsolator::new();
        let vm_id = "test-port-filter";

        let tap_result = isolator.create_tap(vm_id).await;
        if tap_result.is_err() {
            eprintln!("Skipping test: cannot create TAP device (need root)");
            return;
        }
        let tap_config = tap_result.unwrap();

        // Use a specific port
        let allowlist = vec![EgressEntry::new("1.1.1.1", 8080, "tcp")];
        if isolator
            .apply_allowlist(&tap_config.tap_name, &allowlist)
            .await
            .is_err()
        {
            let _ = isolator.teardown(&tap_config.tap_name).await;
            eprintln!("Skipping test: cannot apply nftables rules");
            return;
        }

        // Get rules and verify port-specific rule is present
        let chain_name = format!("ephemeral_{}", tap_config.tap_name.replace('-', "_"));
        let rules = get_chain_rules(&chain_name).unwrap_or_default();

        // Check that the rule includes the port
        assert!(
            rules.contains("dport 8080") && rules.contains("accept"),
            "Should have port-specific accept rule for 8080. Rules:\n{}",
            rules
        );

        // Clean up
        let _ = isolator.teardown(&tap_config.tap_name).await;
    }

    #[tokio::test]
    async fn test_rules_cleaned_up_after_teardown() {
        if !check_nftables_available() {
            eprintln!("Skipping test: nftables not available or no permissions");
            return;
        }

        let isolator = LinuxNetworkIsolator::new();
        let vm_id = "test-cleanup";

        let tap_result = isolator.create_tap(vm_id).await;
        if tap_result.is_err() {
            eprintln!("Skipping test: cannot create TAP device (need root)");
            return;
        }
        let tap_config = tap_result.unwrap();
        let chain_name = format!("ephemeral_{}", tap_config.tap_name.replace('-', "_"));

        let allowlist = vec![EgressEntry::https("example.com")];
        if isolator
            .apply_allowlist(&tap_config.tap_name, &allowlist)
            .await
            .is_err()
        {
            let _ = isolator.teardown(&tap_config.tap_name).await;
            eprintln!("Skipping test: cannot apply nftables rules");
            return;
        }

        // Chain should exist
        assert!(
            chain_exists(&chain_name),
            "Chain should exist before teardown"
        );

        // Teardown
        let _ = isolator.teardown(&tap_config.tap_name).await;

        // Chain should be gone
        assert!(
            !chain_exists(&chain_name),
            "Chain should not exist after teardown"
        );
    }
}

mod test_dns_resolution {
    use super::*;

    #[test]
    fn test_ip_address_passes_through() {
        // This doesn't need root - just testing the resolution logic
        let _isolator = LinuxNetworkIsolator::new();

        // IP addresses should pass through unchanged (tested via the public API indirectly)
        // The resolve_hostname method is private, so we test behavior through apply_allowlist
        // For now, this is a placeholder - the actual test would need network access
    }

    #[test]
    fn test_default_egress_allowlist_resolves() {
        let allowlist = default_egress_allowlist();

        // Verify all entries have valid structure
        for entry in &allowlist {
            assert!(!entry.host.is_empty(), "Host should not be empty");
            assert_eq!(entry.port, 443, "Default port should be 443");
            assert_eq!(entry.protocol, "tcp", "Default protocol should be tcp");
        }

        // Verify expected hosts are present
        let hosts: Vec<&str> = allowlist.iter().map(|e| e.host.as_str()).collect();
        assert!(hosts.contains(&"pypi.org"), "Should contain pypi.org");
        assert!(hosts.contains(&"crates.io"), "Should contain crates.io");
        assert!(hosts.contains(&"github.com"), "Should contain github.com");
    }
}

mod test_egress_entry {
    use super::*;

    #[test]
    fn test_egress_entry_from_str() {
        let entry: EgressEntry = "example.com".into();
        assert_eq!(entry.host, "example.com");
        assert_eq!(entry.port, 443);
        assert_eq!(entry.protocol, "tcp");
    }

    #[test]
    fn test_egress_entry_https() {
        let entry = EgressEntry::https("secure.example.com");
        assert_eq!(entry.host, "secure.example.com");
        assert_eq!(entry.port, 443);
        assert_eq!(entry.protocol, "tcp");
    }

    #[test]
    fn test_egress_entry_custom() {
        let entry = EgressEntry::new("api.example.com", 8080, "tcp");
        assert_eq!(entry.host, "api.example.com");
        assert_eq!(entry.port, 8080);
        assert_eq!(entry.protocol, "tcp");
    }

    #[test]
    fn test_egress_entry_udp() {
        let entry = EgressEntry::new("dns.example.com", 53, "udp");
        assert_eq!(entry.host, "dns.example.com");
        assert_eq!(entry.port, 53);
        assert_eq!(entry.protocol, "udp");
    }
}

mod test_egress_rule_conversion {
    use monad_node::p2p::messages::EgressRule;

    use super::*;

    #[test]
    fn test_from_egress_rule_reference() {
        let rule = EgressRule {
            host: "api.example.com".to_string(),
            port: 8443,
            protocol: "tcp".to_string(),
        };

        let entry: EgressEntry = (&rule).into();
        assert_eq!(entry.host, "api.example.com");
        assert_eq!(entry.port, 8443);
        assert_eq!(entry.protocol, "tcp");
    }

    #[test]
    fn test_from_egress_rule_owned() {
        let rule = EgressRule {
            host: "api.example.com".to_string(),
            port: 443,
            protocol: "tcp".to_string(),
        };

        let entry: EgressEntry = rule.into();
        assert_eq!(entry.host, "api.example.com");
        assert_eq!(entry.port, 443);
        assert_eq!(entry.protocol, "tcp");
    }
}
