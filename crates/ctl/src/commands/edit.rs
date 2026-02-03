//! Edit configuration in $EDITOR

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use monad_node::management::{ManagementRequest, ManagementResponse, NodeConfig};
use std::path::Path;
use std::process::Command;

pub async fn run(config_path: &str, node: &str, resource: &str) -> anyhow::Result<()> {
    match resource {
        "config" => edit_config(config_path, node).await,
        _ => anyhow::bail!("Unknown resource: {}. Use 'config'", resource),
    }
}

async fn edit_config(config_path: &str, node: &str) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    // 1. Get current config
    let response = client.request(ManagementRequest::GetConfig).await?;
    let current_config = match response {
        ManagementResponse::Config(c) => c,
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    };

    // 2. Write to temp file
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!("graphene-config-{}.yaml", node));
    std::fs::write(&temp_path, serde_yaml::to_string(&current_config)?)?;

    // 3. Open in $EDITOR
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(&editor).arg(&temp_path).status()?;

    if !status.success() {
        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);
        anyhow::bail!("Editor exited with non-zero status");
    }

    // 4. Read modified config
    let modified_content = std::fs::read_to_string(&temp_path)?;
    let _ = std::fs::remove_file(&temp_path); // Clean up

    let new_config: NodeConfig = serde_yaml::from_str(&modified_content)?;

    // 5. Validate
    new_config
        .validate()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // 6. Apply
    let response = client
        .request(ManagementRequest::ApplyConfig {
            config: Box::new(new_config),
        })
        .await?;

    match response {
        ManagementResponse::Ok => println!("Configuration applied successfully"),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}
