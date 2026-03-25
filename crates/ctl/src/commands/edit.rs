//! Edit configuration in $EDITOR

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use opencapsule_node::http::management::{ManagementRequest, ManagementResponse, NodeConfig};
use std::path::Path;
use std::process::Command;

/// Supported resource types for the edit command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditResource {
    Config,
}

/// Parse a resource string into an EditResource enum.
pub fn parse_edit_resource(resource: &str) -> Result<EditResource, String> {
    match resource {
        "config" => Ok(EditResource::Config),
        _ => Err(format!("Unknown resource: {}. Use 'config'", resource)),
    }
}

/// Get the editor command from environment or use default.
pub fn get_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string())
}

/// Generate a temp file path for editing a node's config.
pub fn temp_config_path(node: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("opencapsule-config-{}.yaml", node))
}

pub async fn run(config_path: &str, node: &str, resource: &str) -> anyhow::Result<()> {
    match parse_edit_resource(resource).map_err(|e| anyhow::anyhow!("{}", e))? {
        EditResource::Config => edit_config(config_path, node).await,
    }
}

async fn edit_config(config_path: &str, node: &str) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    // 1. Get current config
    let response = client.request(ManagementRequest::GetConfig).await?;
    let current_config = match response {
        ManagementResponse::Config(c) => c,
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    };

    // 2. Write to temp file
    let temp_path = temp_config_path(node);
    std::fs::write(&temp_path, serde_yaml::to_string(&current_config)?)?;

    // 3. Open in $EDITOR
    let editor = get_editor();
    let status = Command::new(&editor).arg(&temp_path).status()?;

    if !status.success() {
        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);
        anyhow::bail!("Editor exited with non-zero status");
    }

    // 4. Read modified config
    let modified_content = std::fs::read_to_string(&temp_path)?;
    let _ = std::fs::remove_file(&temp_path); // Clean up

    let new_config: NodeConfig = serde_yaml::from_str(&modified_content)?;

    // 5. Validate
    new_config
        .validate()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // 6. Apply
    let response = client
        .request(ManagementRequest::ApplyConfig {
            config: Box::new(new_config),
        })
        .await?;

    match response {
        ManagementResponse::Ok => println!("Configuration applied successfully"),
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_edit_resource_config() {
        assert_eq!(parse_edit_resource("config"), Ok(EditResource::Config));
    }

    #[test]
    fn test_parse_edit_resource_invalid() {
        assert!(parse_edit_resource("status").is_err());
        assert!(parse_edit_resource("").is_err());
        assert!(parse_edit_resource("Config").is_err()); // Case sensitive
    }

    #[test]
    fn test_parse_edit_resource_error_message() {
        let err = parse_edit_resource("invalid").unwrap_err();
        assert!(err.contains("Unknown resource"));
        assert!(err.contains("config"));
    }

    #[test]
    fn test_get_editor_default() {
        // When EDITOR is not set, should return "vi"
        std::env::remove_var("EDITOR");
        assert_eq!(get_editor(), "vi");
    }

    #[test]
    fn test_temp_config_path() {
        let path = temp_config_path("my-node");
        assert!(path
            .to_string_lossy()
            .contains("opencapsule-config-my-node.yaml"));
    }

    #[test]
    fn test_temp_config_path_special_chars() {
        // Node names with special characters should be included as-is
        let path = temp_config_path("node-123");
        assert!(path.to_string_lossy().contains("node-123"));
    }
}
