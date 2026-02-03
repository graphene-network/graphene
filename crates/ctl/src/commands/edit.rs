//! Edit configuration in $EDITOR

pub async fn run(_config_path: &str, node: &str, resource: &str) -> anyhow::Result<()> {
    match resource {
        "config" => {
            println!("Editing configuration for node {}", node);
            // TODO: Implement:
            // 1. Get current config
            // 2. Write to temp file
            // 3. Open in $EDITOR
            // 4. On save, apply the new config
        }
        _ => {
            anyhow::bail!("Unknown resource: {}. Use 'config'", resource);
        }
    }

    anyhow::bail!("Edit not yet implemented")
}
