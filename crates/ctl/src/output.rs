//! Output formatting utilities

#![allow(dead_code)]

use graphene_node::management::{
    protocol::{CapabilityInfo, MetricsSnapshot, NodeStatus},
    NodeConfig,
};
use serde::Serialize;

/// Output format for CLI commands
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Yaml,
}

/// Format a serializable value according to the output format
pub fn format_output<T: Serialize>(value: &T, format: OutputFormat) -> String {
    match format {
        OutputFormat::Text => {
            // For text format, caller should use specific formatters
            serde_yaml::to_string(value).unwrap_or_else(|_| "Error formatting output".to_string())
        }
        OutputFormat::Json => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
        }
        OutputFormat::Yaml => serde_yaml::to_string(value).unwrap_or_else(|_| "".to_string()),
    }
}

/// Format node status as human-readable text
pub fn format_status_text(status: &NodeStatus) -> String {
    let mut out = String::new();

    out.push_str(&format!("Node: {}\n", status.node_id));
    out.push_str(&format!("State: {}\n", status.state));
    out.push_str(&format!(
        "Uptime: {}\n",
        format_duration(status.uptime_secs)
    ));
    out.push('\n');

    out.push_str("Jobs:\n");
    out.push_str(&format!("  Active: {}\n", status.jobs_active));
    out.push_str(&format!("  Completed: {}\n", status.jobs_completed));
    out.push('\n');

    if let Some(stake) = &status.stake {
        out.push_str(&format!(
            "Stake: {} GRAPHENE at {}\n",
            stake.amount, stake.address
        ));
        if let Some(unbonds_at) = stake.unbonds_at {
            out.push_str(&format!("  Unbonds at: {}\n", format_timestamp(unbonds_at)));
        }
        out.push('\n');
    }

    if !status.active_channels.is_empty() {
        out.push_str(&format!(
            "Active Channels: {}\n",
            status.active_channels.len()
        ));
        out.push('\n');
    }

    out.push_str("System:\n");
    out.push_str(&format!("  Version: {}\n", status.system.node_version));
    out.push_str(&format!("  OS: {}\n", status.system.os_version));
    out.push_str(&format!("  vCPUs: {}\n", status.system.vcpus));
    out.push_str(&format!("  Memory: {} MB\n", status.system.memory_mb));
    out.push_str(&format!("  Disk: {}%\n", status.system.disk_usage_pct));
    out.push_str(&format!(
        "  Attestation: {}\n",
        if status.system.attestation_valid {
            "Valid"
        } else {
            "Invalid"
        }
    ));

    out
}

/// Format metrics snapshot as human-readable text
pub fn format_metrics_text(metrics: &MetricsSnapshot) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "Metrics (as of {})\n\n",
        format_timestamp(metrics.timestamp)
    ));

    out.push_str("Jobs:\n");
    out.push_str(&format!("  Total: {}\n", metrics.jobs_total));
    out.push_str(&format!("  Failed: {}\n", metrics.jobs_failed));
    out.push_str(&format!(
        "  Avg Duration: {}ms\n",
        metrics.avg_job_duration_ms
    ));
    out.push('\n');

    out.push_str("Resources:\n");
    out.push_str(&format!("  CPU: {:.1}%\n", metrics.cpu_usage_pct));
    out.push_str(&format!("  Memory: {:.1}%\n", metrics.memory_usage_pct));
    out.push_str(&format!(
        "  Network In: {}\n",
        format_bytes(metrics.network_bytes_in)
    ));
    out.push_str(&format!(
        "  Network Out: {}\n",
        format_bytes(metrics.network_bytes_out)
    ));
    out.push('\n');

    let earnings = metrics.earnings_micros as f64 / 1_000_000.0;
    out.push_str(&format!("Earnings: {:.6} GRAPHENE\n", earnings));

    out
}

/// Format capabilities list as a table
pub fn format_capabilities_text(caps: &[CapabilityInfo]) -> String {
    if caps.is_empty() {
        return "No capabilities found.\n".to_string();
    }

    let mut out = String::new();
    out.push_str("PREFIX     ROLE      CREATED              EXPIRES\n");
    out.push_str("─────────  ────────  ───────────────────  ───────────────────\n");

    for cap in caps {
        let expires = cap
            .expires_at
            .map(format_timestamp)
            .unwrap_or_else(|| "never".to_string());
        out.push_str(&format!(
            "{:<9}  {:<8}  {:<19}  {}\n",
            &cap.prefix,
            format!("{:?}", cap.role).to_lowercase(),
            format_timestamp(cap.created_at),
            expires
        ));
    }

    out
}

/// Format node config as YAML (most readable for hierarchical data)
pub fn format_config_text(config: &NodeConfig) -> String {
    serde_yaml::to_string(config).unwrap_or_else(|_| "Error formatting config".to_string())
}

/// Format duration in human-readable form (e.g., "2d 5h 30m 12s")
pub fn format_duration(secs: u64) -> String {
    if secs == 0 {
        return "0s".to_string();
    }

    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }

    parts.join(" ")
}

/// Format bytes in human-readable form (e.g., "1.5 GB")
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format Unix timestamp as human-readable date/time
pub fn format_timestamp(epoch_secs: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    let _datetime = UNIX_EPOCH + Duration::from_secs(epoch_secs);

    // Simple formatting without external crate
    // Returns ISO-8601 style: "2024-01-15 10:30:00"
    let secs_since_epoch = epoch_secs;
    let days_since_epoch = secs_since_epoch / 86400;
    let time_of_day = secs_since_epoch % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days_since_epoch);

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day)
fn days_to_ymd(days: u64) -> (i32, u32, u32) {
    // Algorithm from Howard Hinnant's date algorithms
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
        assert_eq!(format_duration(90061), "1d 1h 1m 1s");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1572864), "1.50 MB");
        assert_eq!(format_bytes(1610612736), "1.50 GB");
    }

    #[test]
    fn test_format_timestamp() {
        // 2024-01-15 00:00:00 UTC = 1705276800
        let ts = format_timestamp(1705276800);
        assert!(ts.starts_with("2024-01-15"));
    }
}
