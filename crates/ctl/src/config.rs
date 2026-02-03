//! Client configuration file handling

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// graphenectl client configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientConfig {
    /// Configured nodes
    #[serde(default)]
    pub nodes: HashMap<String, NodeEntry>,
}

/// Configuration for a single node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEntry {
    /// Node ID (ed25519 public key)
    pub node_id: String,
    /// Capability token for authentication
    pub capability: String,
    /// Optional direct endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl ClientConfig {
    /// Load config from file
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Save config to file
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get a node by name
    pub fn get_node(&self, name: &str) -> Option<&NodeEntry> {
        self.nodes.get(name)
    }

    /// Add or update a node
    pub fn set_node(&mut self, name: String, entry: NodeEntry) {
        self.nodes.insert(name, entry);
    }

    /// Remove a node
    pub fn remove_node(&mut self, name: &str) -> Option<NodeEntry> {
        self.nodes.remove(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let mut config = ClientConfig::default();
        config.set_node(
            "test-node".to_string(),
            NodeEntry {
                node_id: "ed25519:abc123".to_string(),
                capability: "graphene-cap:v1:admin:...".to_string(),
                endpoint: Some("192.168.1.100:9000".to_string()),
            },
        );

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: ClientConfig = serde_yaml::from_str(&yaml).unwrap();

        assert!(parsed.get_node("test-node").is_some());
    }
}
