//! Reboot command

pub async fn run(_config_path: &str, node: &str, force: bool) -> anyhow::Result<()> {
    if !force {
        println!("Are you sure you want to reboot node {}? Use --force to confirm.", node);
        anyhow::bail!("Reboot cancelled. Use --force to confirm.");
    }

    println!("Rebooting node {}", node);
    // TODO: Send Reboot request

    Ok(())
}
