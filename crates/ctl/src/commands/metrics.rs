//! Metrics command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use crate::output::{format_metrics_text, format_output, OutputFormat};
use monad_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

pub async fn run(config_path: &str, node: &str, output_format: OutputFormat) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client.request(ManagementRequest::GetMetrics).await?;

    match response {
        ManagementResponse::Metrics(metrics) => match output_format {
            OutputFormat::Text => print!("{}", format_metrics_text(&metrics)),
            _ => println!("{}", format_output(&metrics, output_format)),
        },
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}
