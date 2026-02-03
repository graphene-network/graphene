//! Client config management commands

use crate::ConfigAction;

pub async fn run(config_path: &str, action: ConfigAction) -> anyhow::Result<()> {
    match action {
        ConfigAction::Add {
            name,
            node_id,
            capability,
            endpoint,
        } => {
            println!("Adding node '{}' to config at {}", name, config_path);
            println!("  Node ID: {}", node_id);
            println!("  Capability: {}...", &capability[..20.min(capability.len())]);
            if let Some(ep) = endpoint {
                println!("  Endpoint: {}", ep);
            }
            // TODO: Update config file
        }
        ConfigAction::Remove { name } => {
            println!("Removing node '{}' from config at {}", name, config_path);
            // TODO: Update config file
        }
        ConfigAction::List => {
            println!("Configured nodes in {}:", config_path);
            // TODO: Read and display config file
        }
    }

    Ok(())
}
