//! Worker lifecycle commands

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use graphene_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

/// Minimum stake amount in GRAPHENE.
pub const MIN_STAKE: u64 = 1;

/// Maximum stake amount in GRAPHENE (to prevent overflow issues).
pub const MAX_STAKE: u64 = 1_000_000_000;

/// Unbonding period in days (from WHITEPAPER.md Section 12.4).
pub const UNBONDING_PERIOD_DAYS: u32 = 14;

/// Validate the stake amount.
pub fn validate_stake(stake: u64) -> Result<(), String> {
    if stake < MIN_STAKE {
        Err(format!(
            "Stake amount must be at least {} GRAPHENE",
            MIN_STAKE
        ))
    } else if stake > MAX_STAKE {
        Err(format!("Stake amount cannot exceed {} GRAPHENE", MAX_STAKE))
    } else {
        Ok(())
    }
}

/// Format the registration success message.
pub fn format_register_message(node: &str, stake: u64) -> String {
    format!("Node {} registered with {} GRAPHENE stake", node, stake)
}

/// Format the unregister success message.
pub fn format_unregister_message(node: &str) -> String {
    format!(
        "Node {} unregistering ({}-day unbonding period started)",
        node, UNBONDING_PERIOD_DAYS
    )
}

pub async fn register(config_path: &str, node: &str, stake: u64) -> anyhow::Result<()> {
    validate_stake(stake).map_err(|e| anyhow::anyhow!("{}", e))?;

    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client
        .request(ManagementRequest::Register {
            stake_amount: stake,
        })
        .await?;

    match response {
        ManagementResponse::Ok => println!("{}", format_register_message(node, stake)),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }
    Ok(())
}

pub async fn unregister(config_path: &str, node: &str) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client.request(ManagementRequest::Unregister).await?;

    match response {
        ManagementResponse::Ok => println!("{}", format_unregister_message(node)),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }
    Ok(())
}

pub async fn join(config_path: &str, node: &str) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client.request(ManagementRequest::Join).await?;

    match response {
        ManagementResponse::Ok => println!("Node {} joined network", node),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }
    Ok(())
}

pub async fn drain(config_path: &str, node: &str) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client.request(ManagementRequest::Drain).await?;

    match response {
        ManagementResponse::Ok => println!("Node {} entering drain mode", node),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }
    Ok(())
}

pub async fn undrain(config_path: &str, node: &str) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client.request(ManagementRequest::Undrain).await?;

    match response {
        ManagementResponse::Ok => println!("Node {} exiting drain mode", node),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_stake_valid() {
        assert!(validate_stake(100).is_ok());
        assert!(validate_stake(MIN_STAKE).is_ok());
        assert!(validate_stake(MAX_STAKE).is_ok());
    }

    #[test]
    fn test_validate_stake_zero() {
        assert!(validate_stake(0).is_err());
    }

    #[test]
    fn test_validate_stake_exceeds_max() {
        assert!(validate_stake(MAX_STAKE + 1).is_err());
    }

    #[test]
    fn test_validate_stake_error_messages() {
        let err = validate_stake(0).unwrap_err();
        assert!(err.contains("at least"));

        let err = validate_stake(MAX_STAKE + 1).unwrap_err();
        assert!(err.contains("cannot exceed"));
    }

    #[test]
    fn test_format_register_message() {
        let msg = format_register_message("my-node", 500);
        assert!(msg.contains("my-node"));
        assert!(msg.contains("500"));
        assert!(msg.contains("GRAPHENE"));
    }

    #[test]
    fn test_format_unregister_message() {
        let msg = format_unregister_message("my-node");
        assert!(msg.contains("my-node"));
        assert!(msg.contains("14-day")); // Unbonding period
        assert!(msg.contains("unbonding"));
    }

    #[test]
    fn test_unbonding_period_constant() {
        assert_eq!(UNBONDING_PERIOD_DAYS, 14);
    }

    #[test]
    fn test_min_stake_constant() {
        assert_eq!(MIN_STAKE, 1);
    }

    #[test]
    fn test_max_stake_constant() {
        assert_eq!(MAX_STAKE, 1_000_000_000);
    }
}
