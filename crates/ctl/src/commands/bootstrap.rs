//! Bootstrap command - connect to a new node and save credentials

/// Validate node addresses for bootstrap.
pub fn validate_node_addresses(nodes: &[String]) -> Result<(), String> {
    if nodes.is_empty() {
        return Err("At least one node address is required".to_string());
    }

    for node in nodes {
        if node.is_empty() {
            return Err("Node address cannot be empty".to_string());
        }
        // Basic validation - could be IP:port, hostname:port, or node ID
        // More detailed validation happens during actual connection
    }

    Ok(())
}

/// Check if bootstrap is supported (stub implementation).
pub fn check_bootstrap_support() -> Result<(), String> {
    Err("Bootstrap not yet implemented. Use cloud-init or console access to get initial credentials.".to_string())
}

pub async fn run(nodes: Vec<String>, output: String) -> anyhow::Result<()> {
    validate_node_addresses(&nodes).map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("Bootstrapping connection to nodes: {:?}", nodes);
    println!("Output will be saved to: {}", output);

    // TODO(#132): Implement actual bootstrap:
    // 1. Connect to node via Iroh
    // 2. Perform initial handshake
    // 3. Retrieve admin capability token
    // 4. Save to config file

    check_bootstrap_support().map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_node_addresses_valid() {
        assert!(validate_node_addresses(&["192.168.1.100".to_string()]).is_ok());
        assert!(validate_node_addresses(&["node.example.com".to_string()]).is_ok());
        assert!(validate_node_addresses(&[
            "192.168.1.100".to_string(),
            "192.168.1.101".to_string()
        ])
        .is_ok());
    }

    #[test]
    fn test_validate_node_addresses_empty_list() {
        let empty: Vec<String> = vec![];
        assert!(validate_node_addresses(&empty).is_err());
    }

    #[test]
    fn test_validate_node_addresses_empty_string() {
        assert!(validate_node_addresses(&["".to_string()]).is_err());
        assert!(validate_node_addresses(&["192.168.1.100".to_string(), "".to_string()]).is_err());
    }

    #[test]
    fn test_check_bootstrap_support() {
        // Bootstrap is not yet implemented
        assert!(check_bootstrap_support().is_err());
    }

    #[test]
    fn test_check_bootstrap_support_error_message() {
        let err = check_bootstrap_support().unwrap_err();
        assert!(err.contains("not yet implemented"));
        assert!(err.contains("cloud-init") || err.contains("console"));
    }
}
