//! Graphene Worker Binary
//!
//! CLI interface for running a Graphene compute worker node.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::error;
use tracing_subscriber::EnvFilter;

use monad_node::worker::{
    register_worker, run_daemon, show_status, unregister_worker, WorkerConfig, WorkerError,
};

#[derive(Parser)]
#[command(name = "graphene-worker")]
#[command(author, version, about = "Graphene compute network worker node")]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, default_value = "worker.toml", env = "GRAPHENE_CONFIG")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the worker daemon
    Run {
        /// Run in foreground (don't daemonize)
        #[arg(short, long, default_value = "true")]
        foreground: bool,
    },

    /// Register this worker on-chain
    Register {
        /// Amount of SOL to stake
        #[arg(short, long, default_value = "0.1")]
        stake: f64,

        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Unregister this worker and reclaim stake
    Unregister {
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Show worker status
    Status {
        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Show version information
    Version,
}

fn init_logging(config: &WorkerConfig) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Handle version command before loading config
    if matches!(cli.command, Commands::Version) {
        println!("graphene-worker {}", env!("CARGO_PKG_VERSION"));
        println!("Graphene compute network worker node");
        return;
    }

    // Load configuration
    let config = match WorkerConfig::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load configuration from {:?}: {}", cli.config, e);
            std::process::exit(1);
        }
    };

    // Initialize logging
    init_logging(&config);

    // Execute command
    let result: Result<(), WorkerError> = match cli.command {
        Commands::Run { foreground } => run_daemon(config, foreground).await,

        Commands::Register { stake, yes } => register_worker(&config, stake, yes).await,

        Commands::Unregister { yes } => unregister_worker(&config, yes).await,

        Commands::Status { format } => show_status(&config, &format).await,

        Commands::Version => unreachable!(),
    };

    if let Err(e) = result {
        error!("Command failed: {}", e);
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
