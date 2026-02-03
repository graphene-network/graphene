//! Worker lifecycle commands

pub async fn register(_config_path: &str, node: &str, stake: u64) -> anyhow::Result<()> {
    println!("Registering node {} with stake {}", node, stake);
    // TODO: Send Register request
    Ok(())
}

pub async fn unregister(_config_path: &str, node: &str) -> anyhow::Result<()> {
    println!("Unregistering node {} (begins 14-day unbonding)", node);
    // TODO: Send Unregister request
    Ok(())
}

pub async fn join(_config_path: &str, node: &str) -> anyhow::Result<()> {
    println!("Node {} joining network", node);
    // TODO: Send Join request
    Ok(())
}

pub async fn drain(_config_path: &str, node: &str) -> anyhow::Result<()> {
    println!("Node {} entering drain mode", node);
    // TODO: Send Drain request
    Ok(())
}

pub async fn undrain(_config_path: &str, node: &str) -> anyhow::Result<()> {
    println!("Node {} exiting drain mode", node);
    // TODO: Send Undrain request
    Ok(())
}
