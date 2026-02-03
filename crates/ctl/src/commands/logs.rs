//! Logs streaming command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use std::path::Path;

/// Default number of log lines to fetch.
pub const DEFAULT_LINES: u32 = 100;

/// Maximum reasonable number of log lines to request.
pub const MAX_LINES: u32 = 10000;

/// Validate and normalize the lines parameter.
pub fn normalize_lines(lines: u32) -> u32 {
    if lines == 0 {
        DEFAULT_LINES
    } else {
        lines.min(MAX_LINES)
    }
}

pub async fn run(config_path: &str, node: &str, follow: bool, lines: u32) -> anyhow::Result<()> {
    let lines = normalize_lines(lines);

    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    if follow {
        println!("Streaming logs from {} (Ctrl+C to stop)...", node);
        client
            .stream_logs_with_callback(lines, |line| {
                println!("{}", line);
                true // Continue streaming
            })
            .await?;
    } else {
        let log_lines = client.get_logs(lines).await?;
        for line in log_lines {
            println!("{}", line);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_lines_normal() {
        assert_eq!(normalize_lines(100), 100);
        assert_eq!(normalize_lines(500), 500);
    }

    #[test]
    fn test_normalize_lines_zero() {
        // Zero should use default
        assert_eq!(normalize_lines(0), DEFAULT_LINES);
    }

    #[test]
    fn test_normalize_lines_exceeds_max() {
        // Values exceeding max should be clamped
        assert_eq!(normalize_lines(MAX_LINES + 1), MAX_LINES);
        assert_eq!(normalize_lines(u32::MAX), MAX_LINES);
    }

    #[test]
    fn test_normalize_lines_at_max() {
        assert_eq!(normalize_lines(MAX_LINES), MAX_LINES);
    }

    #[test]
    fn test_default_lines_constant() {
        assert_eq!(DEFAULT_LINES, 100);
    }

    #[test]
    fn test_max_lines_constant() {
        assert_eq!(MAX_LINES, 10000);
    }
}
