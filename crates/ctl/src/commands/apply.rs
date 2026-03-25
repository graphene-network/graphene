//! Apply configuration command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use graphene_node::http::management::{ManagementRequest, ManagementResponse, NodeConfig};
use std::path::Path;

/// Load and parse a NodeConfig from YAML content.
pub fn parse_node_config(content: &str) -> Result<NodeConfig, String> {
    serde_yaml::from_str(content).map_err(|e| format!("Failed to parse config: {}", e))
}

/// Validate a NodeConfig.
pub fn validate_node_config(config: &NodeConfig) -> Result<(), String> {
    config.validate().map_err(|e| e.to_string())
}

pub async fn run(config_path: &str, node: &str, file: &str) -> anyhow::Result<()> {
    // Load and validate the node config file
    let config_content = std::fs::read_to_string(file)?;
    let node_config = parse_node_config(&config_content).map_err(|e| anyhow::anyhow!("{}", e))?;
    validate_node_config(&node_config).map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("Configuration validated successfully");

    // Load client config and connect
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    // Apply the config
    let response = client
        .request(ManagementRequest::ApplyConfig {
            config: Box::new(node_config),
        })
        .await?;

    match response {
        ManagementResponse::Ok => println!("Configuration applied successfully"),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_node_config_valid() {
        // NodeConfig::default() should serialize to valid YAML
        let default_config = NodeConfig::default();
        let yaml = serde_yaml::to_string(&default_config).unwrap();
        let parsed = parse_node_config(&yaml);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_parse_node_config_invalid_yaml() {
        let invalid = "this is not: valid: yaml: [";
        let result = parse_node_config(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_node_config_empty_parses_to_default() {
        // Empty YAML parses to default values (serde behavior)
        let result = parse_node_config("");
        // This actually succeeds because NodeConfig has Default
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_node_config_syntax_error() {
        // YAML with unclosed brackets should fail
        let syntax_error = "key: [unclosed";
        let result = parse_node_config(syntax_error);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse"));
    }

    #[test]
    fn test_validate_node_config_default() {
        // Default config should be valid
        let config = NodeConfig::default();
        assert!(validate_node_config(&config).is_ok());
    }

    #[test]
    fn test_parse_and_validate_roundtrip() {
        let config = NodeConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed = parse_node_config(&yaml).unwrap();
        assert!(validate_node_config(&parsed).is_ok());
    }
}
