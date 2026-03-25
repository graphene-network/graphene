//! Status command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use crate::output::{format_output, format_status_text, OutputFormat};
use opencapsule_node::http::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

/// Check if watch mode is supported.
///
/// Watch mode requires screen refresh capabilities which are not yet implemented.
pub fn check_watch_mode(watch: bool) -> Result<(), String> {
    if watch {
        Err("Watch mode not yet implemented".to_string())
    } else {
        Ok(())
    }
}

pub async fn run(
    config_path: &str,
    node: &str,
    watch: bool,
    output_format: OutputFormat,
) -> anyhow::Result<()> {
    check_watch_mode(watch).map_err(|e| anyhow::anyhow!("{}", e))?;

    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client.request(ManagementRequest::GetStatus).await?;

    match response {
        ManagementResponse::Status(status) => match output_format {
            OutputFormat::Text => print!("{}", format_status_text(&status)),
            _ => println!("{}", format_output(&status, output_format)),
        },
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_watch_mode_disabled() {
        assert!(check_watch_mode(false).is_ok());
    }

    #[test]
    fn test_check_watch_mode_enabled() {
        assert!(check_watch_mode(true).is_err());
    }

    #[test]
    fn test_check_watch_mode_error_message() {
        let err = check_watch_mode(true).unwrap_err();
        assert!(err.contains("Watch mode"));
        assert!(err.contains("not yet implemented"));
    }
}
