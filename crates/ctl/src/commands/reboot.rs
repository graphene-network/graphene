//! Reboot command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use monad_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

/// Validate that the force flag is set for reboot.
///
/// Rebooting is a destructive operation that requires explicit confirmation.
pub fn require_force(force: bool) -> Result<(), String> {
    if force {
        Ok(())
    } else {
        Err("Reboot cancelled. Use --force to confirm.".to_string())
    }
}

pub async fn run(config_path: &str, node: &str, force: bool) -> anyhow::Result<()> {
    if let Err(e) = require_force(force) {
        println!(
            "Are you sure you want to reboot node {}? Use --force to confirm.",
            node
        );
        anyhow::bail!("{}", e);
    }

    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client.request(ManagementRequest::Reboot).await?;

    match response {
        ManagementResponse::Ok => println!("Reboot initiated for node {}", node),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_require_force_with_force() {
        assert!(require_force(true).is_ok());
    }

    #[test]
    fn test_require_force_without_force() {
        assert!(require_force(false).is_err());
    }

    #[test]
    fn test_require_force_error_message() {
        let err = require_force(false).unwrap_err();
        assert!(err.contains("--force"));
        assert!(err.contains("cancelled"));
    }
}
