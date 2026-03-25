//! opencapsulectl - Remote management CLI for OpenCapsule nodes
//!
//! Provides API-based management for shell-less OpenCapsule Node OS.
//!
//! # Usage
//!
//! ```bash
//! # Bootstrap - get initial credentials
//! opencapsulectl bootstrap --nodes 192.168.1.100
//!
//! # Configuration management
//! opencapsulectl apply -f node-config.toml
//! opencapsulectl get config
//!
//! # Status and monitoring
//! opencapsulectl status
//! opencapsulectl logs --follow
//!
//! # Worker lifecycle
//! opencapsulectl register --stake 100
//! opencapsulectl join
//! opencapsulectl drain
//! ```

use clap::{Parser, Subcommand};
use opencapsulectl::{
    commands, parse_output_format, require_node, shellexpand, CapAction, ConfigAction, OutputFormat,
};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser)]
#[command(name = "opencapsulectl")]
#[command(about = "Remote management CLI for OpenCapsule nodes")]
#[command(version)]
struct Cli {
    /// Path to config file
    #[arg(long, env = "OPENCAPSULE_CONFIG", default_value = "~/.opencapsule/config")]
    config: String,

    /// Node name from config (or node ID if not in config)
    #[arg(short, long, env = "OPENCAPSULE_NODE")]
    node: Option<String>,

    /// Output format (json, yaml, text)
    #[arg(short, long, default_value = "text")]
    output: String,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bootstrap connection to a new node
    Bootstrap {
        /// Node addresses to connect to
        #[arg(long, value_delimiter = ',')]
        nodes: Vec<String>,

        /// Output file for credentials
        #[arg(long, default_value = "~/.opencapsule/config")]
        output: String,
    },

    /// Apply configuration from file
    Apply {
        /// Configuration file path
        #[arg(short, long)]
        file: String,
    },

    /// Get current configuration
    #[command(name = "get")]
    Get {
        /// What to get (config, status)
        resource: String,

        /// Output as YAML
        #[arg(short = 'o', long, value_parser = ["yaml", "json"])]
        output: Option<String>,
    },

    /// Edit configuration in $EDITOR
    Edit {
        /// What to edit (config)
        resource: String,
    },

    /// Get node status
    Status {
        /// Watch status continuously
        #[arg(short, long)]
        watch: bool,
    },

    /// Stream node logs
    Logs {
        /// Follow log output
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show
        #[arg(short, long, default_value = "100")]
        lines: u32,
    },

    /// Get metrics snapshot
    Metrics,

    /// Register node on-chain
    Register {
        /// Amount to stake
        #[arg(long)]
        stake: u64,
    },

    /// Unregister from network (begins unbonding)
    Unregister,

    /// Join network (start accepting jobs)
    Join,

    /// Enter maintenance mode
    Drain,

    /// Exit maintenance mode
    Undrain,

    /// Download and stage OS upgrade
    Upgrade {
        /// OS image URL
        #[arg(long)]
        image: Option<String>,

        /// Apply staged upgrade
        #[arg(long)]
        apply: bool,
    },

    /// Reboot the node
    Reboot {
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Capability management
    #[command(name = "cap")]
    Capability {
        #[command(subcommand)]
        action: CapAction,
    },

    /// Add a node to config
    #[command(name = "config")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    // Expand ~ in config path
    let config_path = shellexpand::tilde(&cli.config).into_owned();

    match cli.command {
        Commands::Bootstrap { nodes, output } => commands::bootstrap::run(nodes, output).await,
        Commands::Apply { file } => {
            let node = require_node(&cli.node)?;
            commands::apply::run(&config_path, &node, &file).await
        }
        Commands::Get { resource, output } => {
            let node = require_node(&cli.node)?;
            let format = match output.as_deref() {
                Some("json") => OutputFormat::Json,
                Some("yaml") => OutputFormat::Yaml,
                _ => OutputFormat::Text,
            };
            commands::get::run(&config_path, &node, &resource, format).await
        }
        Commands::Edit { resource } => {
            let node = require_node(&cli.node)?;
            commands::edit::run(&config_path, &node, &resource).await
        }
        Commands::Status { watch } => {
            let node = require_node(&cli.node)?;
            let format = parse_output_format(&cli.output);
            commands::status::run(&config_path, &node, watch, format).await
        }
        Commands::Logs { follow, lines } => {
            let node = require_node(&cli.node)?;
            commands::logs::run(&config_path, &node, follow, lines).await
        }
        Commands::Metrics => {
            let node = require_node(&cli.node)?;
            let format = parse_output_format(&cli.output);
            commands::metrics::run(&config_path, &node, format).await
        }
        Commands::Register { stake } => {
            let node = require_node(&cli.node)?;
            commands::lifecycle::register(&config_path, &node, stake).await
        }
        Commands::Unregister => {
            let node = require_node(&cli.node)?;
            commands::lifecycle::unregister(&config_path, &node).await
        }
        Commands::Join => {
            let node = require_node(&cli.node)?;
            commands::lifecycle::join(&config_path, &node).await
        }
        Commands::Drain => {
            let node = require_node(&cli.node)?;
            commands::lifecycle::drain(&config_path, &node).await
        }
        Commands::Undrain => {
            let node = require_node(&cli.node)?;
            commands::lifecycle::undrain(&config_path, &node).await
        }
        Commands::Upgrade { image, apply } => {
            let node = require_node(&cli.node)?;
            commands::upgrade::run(&config_path, &node, image, apply).await
        }
        Commands::Reboot { force } => {
            let node = require_node(&cli.node)?;
            commands::reboot::run(&config_path, &node, force).await
        }
        Commands::Capability { action } => {
            let node = require_node(&cli.node)?;
            commands::capability::run(&config_path, &node, action).await
        }
        Commands::Config { action } => commands::config::run(&config_path, action).await,
    }
}
