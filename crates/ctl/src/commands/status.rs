//! Status command

pub async fn run(_config_path: &str, node: &str, watch: bool) -> anyhow::Result<()> {
    println!("Getting status from node {}", node);

    if watch {
        println!("(watch mode - would refresh continuously)");
    }

    // TODO: Connect to node and get status
    // Display formatted status output

    Ok(())
}
