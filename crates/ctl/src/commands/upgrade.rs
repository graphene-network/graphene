//! OS upgrade command

pub async fn run(
    _config_path: &str,
    node: &str,
    image: Option<String>,
    apply: bool,
) -> anyhow::Result<()> {
    if apply {
        println!("Applying staged upgrade on node {} (will reboot)", node);
        // TODO: Send ApplyUpgrade request
    } else if let Some(url) = image {
        println!("Downloading upgrade image from {} to node {}", url, node);
        // TODO: Send Upgrade request with URL
    } else {
        anyhow::bail!("Specify --image URL to download, or --apply to apply staged upgrade");
    }

    Ok(())
}
