//! Metrics command

pub async fn run(_config_path: &str, node: &str) -> anyhow::Result<()> {
    println!("Getting metrics from node {}", node);

    // TODO: Connect to node and get metrics snapshot

    Ok(())
}
