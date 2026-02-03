//! Logs streaming command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use std::path::Path;

pub async fn run(config_path: &str, node: &str, follow: bool, lines: u32) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    if follow {
        println!("Streaming logs from {} (Ctrl+C to stop)...", node);
        client
            .stream_logs_with_callback(lines, |line| {
                println!("{}", line);
                true // Continue streaming
            })
            .await?;
    } else {
        let log_lines = client.get_logs(lines).await?;
        for line in log_lines {
            println!("{}", line);
        }
    }

    Ok(())
}
