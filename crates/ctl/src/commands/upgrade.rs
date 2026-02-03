//! OS upgrade command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use monad_node::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

/// Upgrade action to perform
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeAction {
    /// Apply a previously staged upgrade
    Apply,
    /// Download and stage an upgrade from URL
    Download(String),
}

/// Validate upgrade parameters and determine the action.
///
/// Returns an error if neither --image nor --apply is specified.
/// If --apply is specified, it takes precedence over --image.
pub fn validate_upgrade_params(
    image: Option<String>,
    apply: bool,
) -> Result<UpgradeAction, String> {
    if apply {
        Ok(UpgradeAction::Apply)
    } else if let Some(url) = image {
        Ok(UpgradeAction::Download(url))
    } else {
        Err("Specify --image URL to download, or --apply to apply staged upgrade".to_string())
    }
}

pub async fn run(
    config_path: &str,
    node: &str,
    image: Option<String>,
    apply: bool,
) -> anyhow::Result<()> {
    let action = validate_upgrade_params(image, apply).map_err(|e| anyhow::anyhow!("{}", e))?;

    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    match action {
        UpgradeAction::Apply => {
            println!("Applying staged upgrade on node {} (will reboot)", node);
            let response = client.request(ManagementRequest::ApplyUpgrade).await?;
            match response {
                ManagementResponse::Ok => println!("Upgrade applied, node rebooting..."),
                ManagementResponse::Error { code, message } => {
                    anyhow::bail!("{}: {}", code, message)
                }
                _ => anyhow::bail!("Unexpected response type"),
            }
        }
        UpgradeAction::Download(url) => {
            println!("Downloading upgrade image from {} to node {}", url, node);
            let response = client
                .request(ManagementRequest::Upgrade { image_url: url })
                .await?;
            match response {
                ManagementResponse::Ok => {
                    println!("Upgrade staged. Run 'graphenectl upgrade --apply' to install.")
                }
                ManagementResponse::Error { code, message } => {
                    anyhow::bail!("{}: {}", code, message)
                }
                _ => anyhow::bail!("Unexpected response type"),
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_upgrade_params_apply() {
        assert_eq!(
            validate_upgrade_params(None, true),
            Ok(UpgradeAction::Apply)
        );
    }

    #[test]
    fn test_validate_upgrade_params_apply_takes_precedence() {
        // --apply takes precedence over --image
        assert_eq!(
            validate_upgrade_params(Some("https://example.com/image".to_string()), true),
            Ok(UpgradeAction::Apply)
        );
    }

    #[test]
    fn test_validate_upgrade_params_download() {
        let url = "https://example.com/image.tar.gz".to_string();
        assert_eq!(
            validate_upgrade_params(Some(url.clone()), false),
            Ok(UpgradeAction::Download(url))
        );
    }

    #[test]
    fn test_validate_upgrade_params_neither() {
        let result = validate_upgrade_params(None, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_upgrade_params_error_message() {
        let err = validate_upgrade_params(None, false).unwrap_err();
        assert!(err.contains("--image"));
        assert!(err.contains("--apply"));
    }
}
