//! Client configuration file handling

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// graphenectl client configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientConfig {
    /// Default node to use when --node is not specified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_node: Option<String>,
    /// Configured nodes
    #[serde(default)]
    pub nodes: HashMap<String, NodeEntry>,
}

/// Configuration for a single node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEntry {
    /// Worker URL (e.g., "http://192.168.1.100:9000")
    pub url: String,
    /// Capability token for authentication
    pub capability: String,
    /// Optional description for this node
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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

    /// Save config to file with secure permissions
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;

        // Set restrictive permissions on Unix (0600 = owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Get a node by name
    pub fn get_node(&self, name: &str) -> Option<&NodeEntry> {
        self.nodes.get(name)
    }

    /// Get a node by name, or fall back to default node
    pub fn get_node_or_default(&self, name: Option<&str>) -> Option<&NodeEntry> {
        if let Some(name) = name {
            self.nodes.get(name)
        } else if let Some(default) = &self.default_node {
            self.nodes.get(default)
        } else {
            None
        }
    }

    /// Add or update a node
    pub fn set_node(&mut self, name: String, entry: NodeEntry) {
        self.nodes.insert(name, entry);
    }

    /// Remove a node
    pub fn remove_node(&mut self, name: &str) -> Option<NodeEntry> {
        self.nodes.remove(name)
    }

    /// Set the default node (returns false if node doesn't exist)
    pub fn set_default(&mut self, name: &str) -> bool {
        if self.nodes.contains_key(name) {
            self.default_node = Some(name.to_string());
            true
        } else {
            false
        }
    }

    /// Clear the default node
    pub fn clear_default(&mut self) {
        self.default_node = None;
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
                url: "http://192.168.1.100:9000".to_string(),
                capability: "graphene-cap:v1:admin:...".to_string(),
                description: Some("Test node".to_string()),
            },
        );
        config.set_default("test-node");

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: ClientConfig = serde_yaml::from_str(&yaml).unwrap();

        assert!(parsed.get_node("test-node").is_some());
        assert_eq!(parsed.default_node, Some("test-node".to_string()));
    }

    #[test]
    fn test_get_node_or_default() {
        let mut config = ClientConfig::default();
        config.set_node(
            "node1".to_string(),
            NodeEntry {
                url: "http://host1:9000".to_string(),
                capability: "cap1".to_string(),
                description: None,
            },
        );
        config.set_node(
            "node2".to_string(),
            NodeEntry {
                url: "http://host2:9000".to_string(),
                capability: "cap2".to_string(),
                description: None,
            },
        );
        config.set_default("node1");

        // Explicit name takes precedence
        assert_eq!(
            config.get_node_or_default(Some("node2")).unwrap().url,
            "http://host2:9000"
        );

        // Falls back to default when no name given
        assert_eq!(
            config.get_node_or_default(None).unwrap().url,
            "http://host1:9000"
        );
    }

    #[test]
    fn test_set_default_nonexistent() {
        let mut config = ClientConfig::default();
        assert!(!config.set_default("nonexistent"));
        assert!(config.default_node.is_none());
    }
}
