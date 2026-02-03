//! OS upgrade command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use monad_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

pub async fn run(
    config_path: &str,
    node: &str,
    image: Option<String>,
    apply: bool,
) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    if apply {
        println!("Applying staged upgrade on node {} (will reboot)", node);
        let response = client.request(ManagementRequest::ApplyUpgrade).await?;
        match response {
            ManagementResponse::Ok => println!("Upgrade applied, node rebooting..."),
            ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
            _ => anyhow::bail!("Unexpected response type"),
        }
    } else if let Some(url) = image {
        println!("Downloading upgrade image from {} to node {}", url, node);
        let response = client
            .request(ManagementRequest::Upgrade { image_url: url })
            .await?;
        match response {
            ManagementResponse::Ok => {
                println!("Upgrade staged. Run 'graphenectl upgrade --apply' to install.")
            }
            ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
            _ => anyhow::bail!("Unexpected response type"),
        }
    } else {
        anyhow::bail!("Specify --image URL to download, or --apply to apply staged upgrade");
    }

    Ok(())
}
