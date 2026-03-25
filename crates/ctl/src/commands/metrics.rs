//! Metrics command

use crate::client::{ClientOptions, ManagementClient};
use crate::config::ClientConfig;
use crate::output::{format_metrics_text, format_output, OutputFormat};
use opencapsule_node::http::management::{ManagementRequest, ManagementResponse};
use std::path::Path;

pub async fn run(config_path: &str, node: &str, output_format: OutputFormat) -> anyhow::Result<()> {
    let config = ClientConfig::load(Path::new(config_path))?;
    let entry = config
        .get_node(node)
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node))?;
    let client = ManagementClient::from_config(entry, ClientOptions::default())?;

    let response = client.request(ManagementRequest::GetMetrics).await?;

    match response {
        ManagementResponse::Metrics(metrics) => match output_format {
            OutputFormat::Text => print!("{}", format_metrics_text(&metrics)),
            _ => println!("{}", format_output(&metrics, output_format)),
        },
        ManagementResponse::Error { code, message } => anyhow::bail!("{}: {}", code, message),
        _ => anyhow::bail!("Unexpected response type"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencapsule_node::http::management::MetricsSnapshot;

    fn create_test_metrics() -> MetricsSnapshot {
        MetricsSnapshot {
            timestamp: 1705276800,
            jobs_total: 1000,
            jobs_failed: 10,
            avg_job_duration_ms: 500,
            cpu_usage_pct: 45.5,
            memory_usage_pct: 60.0,
            network_bytes_in: 1024 * 1024 * 100, // 100 MB
            network_bytes_out: 1024 * 1024 * 50, // 50 MB
            earnings_micros: 1000000,
        }
    }

    #[test]
    fn test_format_metrics_text_output() {
        let metrics = create_test_metrics();
        let text = format_metrics_text(&metrics);

        // Should contain key metrics
        assert!(text.contains("Jobs:"));
        assert!(text.contains("Total: 1000"));
        assert!(text.contains("Failed: 10"));
        assert!(text.contains("CPU:"));
        assert!(text.contains("Memory:"));
        assert!(text.contains("Earnings:"));
    }

    #[test]
    fn test_format_metrics_json_output() {
        let metrics = create_test_metrics();
        let json = format_output(&metrics, OutputFormat::Json);

        // Should be valid JSON
        assert!(json.starts_with("{"));
        assert!(json.contains("\"jobs_total\""));
        assert!(json.contains("1000"));
    }

    #[test]
    fn test_format_metrics_yaml_output() {
        let metrics = create_test_metrics();
        let yaml = format_output(&metrics, OutputFormat::Yaml);

        // Should be valid YAML
        assert!(yaml.contains("jobs_total:"));
        assert!(yaml.contains("1000"));
    }
}
