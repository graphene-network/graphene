//! graphenectl - Remote management CLI library for Graphene nodes
//!
//! This library provides the core functionality for the graphenectl CLI,
//! including client, config, and output modules.

use clap::Subcommand;

pub mod client;
pub mod commands;
pub mod config;
pub mod output;

// Re-export commonly used types
pub use client::{ClientError, ClientOptions, ManagementClient};
pub use config::{ClientConfig, NodeEntry};
pub use output::OutputFormat;

/// Capability management subcommands
#[derive(Subcommand, Clone, Debug)]
pub enum CapAction {
    /// Generate new capability token
    Generate {
        /// Role (admin, operator, reader)
        #[arg(long, default_value = "reader")]
        role: String,

        /// TTL in days (0 for no expiry)
        #[arg(long)]
        ttl: Option<u32>,
    },

    /// List capabilities
    List,

    /// Revoke capability by prefix
    Revoke {
        /// Token prefix to revoke
        prefix: String,
    },
}

/// Config management subcommands
#[derive(Subcommand, Clone, Debug)]
pub enum ConfigAction {
    /// Add a node to config
    Add {
        /// Node name
        name: String,

        /// Node URL (e.g. http://192.168.1.100:3000)
        #[arg(long)]
        url: String,

        /// Capability token
        #[arg(long)]
        capability: String,
    },

    /// Remove a node from config
    Remove {
        /// Node name
        name: String,
    },

    /// List configured nodes
    List,
}

/// Shell expansion utilities
pub mod shellexpand {
    /// Expand ~ to home directory
    pub fn tilde(path: &str) -> std::borrow::Cow<'_, str> {
        if path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                return std::borrow::Cow::Owned(format!("{}{}", home.display(), &path[1..]));
            }
        }
        std::borrow::Cow::Borrowed(path)
    }
}

/// Require node to be specified
pub fn require_node(node: &Option<String>) -> anyhow::Result<String> {
    node.clone().ok_or_else(|| {
        anyhow::anyhow!("No node specified. Use --node or set GRAPHENE_NODE env var")
    })
}

/// Parse output format string
pub fn parse_output_format(s: &str) -> OutputFormat {
    match s.to_lowercase().as_str() {
        "json" => OutputFormat::Json,
        "yaml" => OutputFormat::Yaml,
        _ => OutputFormat::Text,
    }
}
