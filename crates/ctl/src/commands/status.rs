//! Status command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use crate::output::{format_output, format_status_text, OutputFormat};
use monad_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

pub async fn run(
    config_path: &str,
    node: &str,
    watch: bool,
    output_format: OutputFormat,
) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    if watch {
        // TODO: Implement watch mode with screen refresh
        anyhow::bail!("Watch mode not yet implemented");
    }

    let response = client.request(ManagementRequest::GetStatus).await?;

    match response {
        ManagementResponse::Status(status) => match output_format {
            OutputFormat::Text => print!("{}", format_status_text(&status)),
            _ => println!("{}", format_output(&status, output_format)),
        },
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}
