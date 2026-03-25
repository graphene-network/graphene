//! HTTP REST API for the Graphene worker node.
//!
//! Provides endpoints for job submission, status polling, result retrieval,
//! health checks, and management operations.
//!
//! # Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | POST | `/v1/jobs` | Submit a new job |
//! | GET | `/v1/jobs/:id` | Get job status |
//! | GET | `/v1/jobs/:id/result` | Get job result |
//! | GET | `/v1/health` | Health check |
//! | GET | `/v1/capabilities` | List capabilities |
//! | GET | `/v1/management/status` | Node status |
//! | GET | `/v1/management/metrics` | Node metrics |
//! | POST | `/v1/management/lifecycle/:action` | Lifecycle control |

pub mod handlers;
pub mod management;
pub mod router;
pub mod state;

pub use management::{ManagementRequest, ManagementResponse, NodeConfig, NodeStatus, Role};
pub use router::build_router;
pub use state::AppState;
