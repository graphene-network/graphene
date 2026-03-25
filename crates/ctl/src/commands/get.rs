//! Get resource command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use crate::output::{format_config_text, format_output, format_status_text, OutputFormat};
use opencapsule_node::http::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

/// Supported resource types for the get command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Config,
    Status,
}

/// Parse a resource string into a Resource enum.
pub fn parse_resource(resource: &str) -> Result<Resource, String> {
    match resource {
        "config" => Ok(Resource::Config),
        "status" => Ok(Resource::Status),
        _ => Err(format!(
            "Unknown resource: {}. Use 'config' or 'status'",
            resource
        )),
    }
}

pub async fn run(
    config_path: &str,
    node: &str,
    resource: &str,
    output_format: OutputFormat,
) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    match parse_resource(resource).map_err(|e| anyhow::anyhow!("{}", e))? {
        Resource::Config => {
            let response = client.request(ManagementRequest::GetConfig).await?;
            match response {
                ManagementResponse::Config(cfg) => match output_format {
                    OutputFormat::Text => print!("{}", format_config_text(&cfg)),
                    _ => println!("{}", format_output(&cfg, output_format)),
                },
                ManagementResponse::Error { code, message } => {
                    anyhow::bail!("{}: {}", code, message)
                }
                _ => anyhow::bail!("Unexpected response type"),
            }
        }
        Resource::Status => {
            let response = client.request(ManagementRequest::GetStatus).await?;
            match response {
                ManagementResponse::Status(status) => match output_format {
                    OutputFormat::Text => print!("{}", format_status_text(&status)),
                    _ => println!("{}", format_output(&status, output_format)),
                },
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
    fn test_parse_resource_config() {
        assert_eq!(parse_resource("config"), Ok(Resource::Config));
    }

    #[test]
    fn test_parse_resource_status() {
        assert_eq!(parse_resource("status"), Ok(Resource::Status));
    }

    #[test]
    fn test_parse_resource_invalid() {
        assert!(parse_resource("logs").is_err());
        assert!(parse_resource("metrics").is_err());
        assert!(parse_resource("").is_err());
        assert!(parse_resource("Config").is_err()); // Case sensitive
        assert!(parse_resource("STATUS").is_err()); // Case sensitive
    }

    #[test]
    fn test_parse_resource_error_message() {
        let err = parse_resource("invalid").unwrap_err();
        assert!(err.contains("Unknown resource"));
        assert!(err.contains("config"));
        assert!(err.contains("status"));
    }
}
