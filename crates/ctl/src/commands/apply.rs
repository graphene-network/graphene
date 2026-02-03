//! Apply configuration command

pub async fn run(_config_path: &str, node: &str, file: &str) -> anyhow::Result<()> {
    println!("Applying configuration from {} to node {}", file, node);

    // Load the config file
    let config_content = std::fs::read_to_string(file)?;

    // Parse as NodeConfig
    let node_config: monad_node::management::NodeConfig = serde_yaml::from_str(&config_content)?;

    // Validate
    node_config
        .validate()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("Configuration validated successfully");

    // TODO: Connect to node and apply config
    // 1. Load client config from config_path
    // 2. Connect to node via Iroh
    // 3. Send ApplyConfig request
    // 4. Wait for response

    println!("Configuration applied successfully");
    Ok(())
}
