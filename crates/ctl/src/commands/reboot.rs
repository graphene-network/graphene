//! Reboot command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use monad_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

pub async fn run(config_path: &str, node: &str, force: bool) -> anyhow::Result<()> {
    if !force {
        println!(
            "Are you sure you want to reboot node {}? Use --force to confirm.",
            node
        );
        anyhow::bail!("Reboot cancelled. Use --force to confirm.");
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
