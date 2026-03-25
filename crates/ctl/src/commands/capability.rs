//! Capability management commands

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use crate::output::format_capabilities_text;
use crate::CapAction;
use opencapsule_node::http::management::{ManagementRequest, ManagementResponse, Role};
use std::path::Path;

/// Parse a role string into a Role enum.
///
/// Accepts case-insensitive strings: "admin", "operator", "reader".
pub fn parse_role(role: &str) -> Result<Role, String> {
    match role.to_lowercase().as_str() {
        "admin" => Ok(Role::Admin),
        "operator" => Ok(Role::Operator),
        "reader" => Ok(Role::Reader),
        _ => Err(format!(
            "Invalid role: {}. Use admin, operator, or reader",
            role
        )),
    }
}

pub async fn run(config_path: &str, node: &str, action: CapAction) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    match action {
        CapAction::Generate { role, ttl } => {
            let role = parse_role(&role).map_err(|e| anyhow::anyhow!("{}", e))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_role_admin() {
        assert!(matches!(parse_role("admin"), Ok(Role::Admin)));
    }

    #[test]
    fn test_parse_role_operator() {
        assert!(matches!(parse_role("operator"), Ok(Role::Operator)));
    }

    #[test]
    fn test_parse_role_reader() {
        assert!(matches!(parse_role("reader"), Ok(Role::Reader)));
    }

    #[test]
    fn test_parse_role_case_insensitive() {
        assert!(matches!(parse_role("ADMIN"), Ok(Role::Admin)));
        assert!(matches!(parse_role("Admin"), Ok(Role::Admin)));
        assert!(matches!(parse_role("OPERATOR"), Ok(Role::Operator)));
        assert!(matches!(parse_role("Operator"), Ok(Role::Operator)));
        assert!(matches!(parse_role("READER"), Ok(Role::Reader)));
        assert!(matches!(parse_role("Reader"), Ok(Role::Reader)));
    }

    #[test]
    fn test_parse_role_invalid() {
        assert!(parse_role("superuser").is_err());
        assert!(parse_role("root").is_err());
        assert!(parse_role("").is_err());
        assert!(parse_role("adminn").is_err()); // Typo
    }

    #[test]
    fn test_parse_role_error_message() {
        let err = parse_role("invalid").unwrap_err();
        assert!(err.contains("Invalid role"));
        assert!(err.contains("admin"));
        assert!(err.contains("operator"));
        assert!(err.contains("reader"));
    }
}
