//! Node management API over Iroh QUIC
//!
//! Provides remote management capabilities for shell-less Graphene nodes.
//! Inspired by Talos Linux's API-only management model.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────┐
//! │  Operator Workstation                                     │
//! │  ┌─────────────────────────────────────────────────────┐ │
//! │  │  graphenectl + capability token                     │ │
//! │  └──────────────────────┬──────────────────────────────┘ │
//! └─────────────────────────┼─────────────────────────────────┘
//!                           │ Iroh QUIC (same port as P2P)
//!                           ▼
//! ┌───────────────────────────────────────────────────────────┐
//! │  Graphene Node                                            │
//! │  ┌─────────────────────────────────────────────────────┐ │
//! │  │  ManagementHandler                                  │ │
//! │  │  - Validates capability tokens                      │ │
//! │  │  - Processes ManagementRequest                      │ │
//! │  │  - Returns ManagementResponse                       │ │
//! │  └─────────────────────────────────────────────────────┘ │
//! └───────────────────────────────────────────────────────────┘
//! ```

pub mod capability;
pub mod config;
pub mod handler;
pub mod protocol;

pub use capability::{Capability, CapabilityError, CapabilityManager, Role};
pub use config::NodeConfig;
pub use handler::ManagementHandler;
pub use protocol::{ManagementRequest, ManagementResponse, NodeStatus};

/// ALPN protocol identifier for management API
pub const MANAGEMENT_ALPN: &[u8] = b"graphene-mgmt/1";
