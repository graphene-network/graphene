//! Capability management commands

use crate::CapAction;

pub async fn run(_config_path: &str, node: &str, action: CapAction) -> anyhow::Result<()> {
    match action {
        CapAction::Generate { role, ttl } => {
            println!("Generating {} capability for node {}", role, node);
            if let Some(days) = ttl {
                println!("  TTL: {} days", days);
            }
            // TODO: Send GenerateCapability request
        }
        CapAction::List => {
            println!("Listing capabilities for node {}", node);
            // TODO: Send ListCapabilities request
        }
        CapAction::Revoke { prefix } => {
            println!("Revoking capability {} on node {}", prefix, node);
            // TODO: Send RevokeCapability request
        }
    }

    Ok(())
}
