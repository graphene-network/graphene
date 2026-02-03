//! Capability management commands

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use crate::output::format_capabilities_text;
use crate::CapAction;
use monad_node::management::{ManagementRequest, ManagementResponse, Role};
use std::path::Path;

pub async fn run(config_path: &str, node: &str, action: CapAction) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    match action {
        CapAction::Generate { role, ttl } => {
            let role = match role.to_lowercase().as_str() {
                "admin" => Role::Admin,
                "operator" => Role::Operator,
                "reader" => Role::Reader,
                _ => anyhow::bail!("Invalid role: {}. Use admin, operator, or reader", role),
            };

            let response = client
                .request(ManagementRequest::GenerateCapability {
                    role,
                    ttl_days: ttl,
                })
                .await?;

            match response {
                ManagementResponse::Capability(token) => {
                    println!("Generated capability token:");
                    println!("{}", token);
                }
                ManagementResponse::Error { code, message } => {
                    anyhow::bail!("{}: {}", code, message)
                }
                _ => anyhow::bail!("Unexpected response type"),
            }
        }
        CapAction::List => {
            let response = client.request(ManagementRequest::ListCapabilities).await?;
            match response {
                ManagementResponse::Capabilities(caps) => {
                    print!("{}", format_capabilities_text(&caps));
                }
                ManagementResponse::Error { code, message } => {
                    anyhow::bail!("{}: {}", code, message)
                }
                _ => anyhow::bail!("Unexpected response type"),
            }
        }
        CapAction::Revoke { prefix } => {
            let response = client
                .request(ManagementRequest::RevokeCapability {
                    token_prefix: prefix.clone(),
                })
                .await?;
            match response {
                ManagementResponse::Ok => println!("Revoked capability: {}", prefix),
                ManagementResponse::Error { code, message } => {
                    anyhow::bail!("{}: {}", code, message)
                }
                _ => anyhow::bail!("Unexpected response type"),
            }
        }
    }

    Ok(())
}
