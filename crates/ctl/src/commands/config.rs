//! Client config management commands

use crate::ConfigAction;

/// Truncate a capability token for display.
pub fn truncate_capability(capability: &str, max_len: usize) -> String {
    if capability.len() <= max_len {
        capability.to_string()
    } else {
        format!("{}...", &capability[..max_len])
    }
}

/// Validate a node name.
pub fn validate_node_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Node name cannot be empty".to_string());
    }
    if name.len() > 64 {
        return Err("Node name cannot exceed 64 characters".to_string());
    }
    // Allow alphanumeric, hyphens, and underscores
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(
            "Node name can only contain alphanumeric characters, hyphens, and underscores"
                .to_string(),
        );
    }
    Ok(())
}

pub async fn run(config_path: &str, action: ConfigAction) -> anyhow::Result<()> {
    match action {
        ConfigAction::Add {
            name,
            node_id,
            capability,
            endpoint,
        } => {
            validate_node_name(&name).map_err(|e| anyhow::anyhow!("{}", e))?;

            println!("Adding node '{}' to config at {}", name, config_path);
            println!("  Node ID: {}", node_id);
            println!("  Capability: {}", truncate_capability(&capability, 20));
            if let Some(ep) = endpoint {
                println!("  Endpoint: {}", ep);
            }
            // TODO(#130): Update config file
        }
        ConfigAction::Remove { name } => {
            println!("Removing node '{}' from config at {}", name, config_path);
            // TODO(#130): Update config file
        }
        ConfigAction::List => {
            println!("Configured nodes in {}:", config_path);
            // TODO(#130): Read and display config file
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_capability_short() {
        assert_eq!(truncate_capability("short", 20), "short");
    }

    #[test]
    fn test_truncate_capability_exact() {
        let cap = "12345678901234567890"; // 20 chars
        assert_eq!(truncate_capability(cap, 20), cap);
    }

    #[test]
    fn test_truncate_capability_long() {
        let cap = "123456789012345678901234567890"; // 30 chars
        assert_eq!(truncate_capability(cap, 20), "12345678901234567890...");
    }

    #[test]
    fn test_validate_node_name_valid() {
        assert!(validate_node_name("my-node").is_ok());
        assert!(validate_node_name("node_123").is_ok());
        assert!(validate_node_name("Node1").is_ok());
        assert!(validate_node_name("a").is_ok());
    }

    #[test]
    fn test_validate_node_name_empty() {
        assert!(validate_node_name("").is_err());
    }

    #[test]
    fn test_validate_node_name_too_long() {
        let long_name = "a".repeat(65);
        assert!(validate_node_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_node_name_max_length() {
        let max_name = "a".repeat(64);
        assert!(validate_node_name(&max_name).is_ok());
    }

    #[test]
    fn test_validate_node_name_invalid_chars() {
        assert!(validate_node_name("node.name").is_err()); // dot
        assert!(validate_node_name("node name").is_err()); // space
        assert!(validate_node_name("node/name").is_err()); // slash
        assert!(validate_node_name("node@name").is_err()); // at
    }
}
