//! Get resource command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use crate::output::{format_config_text, format_output, format_status_text, OutputFormat};
use monad_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

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

    match resource {
        "config" => {
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
        "status" => {
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
        _ => anyhow::bail!("Unknown resource: {}. Use 'config' or 'status'", resource),
    }

    Ok(())
}
