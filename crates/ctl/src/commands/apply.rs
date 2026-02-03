//! Apply configuration command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use monad_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

pub async fn run(config_path: &str, node: &str, file: &str) -> anyhow::Result<()> {
    // Load and validate the node config file
    let config_content = std::fs::read_to_string(file)?;
    let node_config: monad_node::management::NodeConfig = serde_yaml::from_str(&config_content)?;
    node_config
        .validate()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("Configuration validated successfully");

    // Load client config and connect
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    // Apply the config
    let response = client
        .request(ManagementRequest::ApplyConfig {
            config: Box::new(node_config),
        })
        .await?;

    match response {
        ManagementResponse::Ok => println!("Configuration applied successfully"),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}
