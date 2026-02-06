//! Worker module for job processing and lifecycle management.
//!
//! This module provides the core worker functionality for Graphene nodes:
//!
//! - **State machine** ([`WorkerStateMachine`]): Manages worker lifecycle from
//!   registration through graceful shutdown with atomic slot management.
//! - **Job context** ([`WorkerJobContext`]): Implements the [`JobContext`] trait
//!   to connect the P2P protocol handler with the job executor.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
//! │  P2P Protocol   │────▶│ WorkerJobContext│────▶│   JobExecutor   │
//! │     Handler     │     │                 │     │                 │
//! └─────────────────┘     │ - Reserve slot  │     │ - Run unikernel │
//!         │               │ - Create Job    │     │ - Encrypt output│
//!         ▼               │ - Spawn task    │     └─────────────────┘
//! ┌─────────────────┐     │ - Deliver result│              │
//! │   JobRequest    │     └─────────────────┘              │
//! │                 │              │                       ▼
//! │ - manifest      │              │              ┌─────────────────┐
//! │ - ticket        │              │              │ ExecutionResult │
//! │ - assets        │              ▼              └─────────────────┘
//! └─────────────────┘     ┌─────────────────┐              │
//!                         │   Job Store     │              │
//!                         │                 │◀─────────────┘
//!                         │ state tracking  │
//!                         └─────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use graphene_node::worker::{WorkerJobContext, WorkerStateMachine};
//! use graphene_node::executor::MockJobExecutor;
//! use graphene_node::result::MockResultDelivery;
//!
//! // Create worker components
//! let state_machine = WorkerStateMachine::new_shared(4);
//! let executor = Arc::new(MockJobExecutor::happy_path());
//! let delivery = Arc::new(MockResultDelivery::new());
//!
//! // Create job context
//! let context = WorkerJobContext::new(
//!     state_machine,
//!     executor,
//!     delivery,
//!     capabilities,
//! );
//!
//! // Use with protocol handler
//! let handler = JobProtocolHandler::new(validator, Arc::new(context));
//! ```

pub mod context;
pub mod state;

pub use context::{JobStore, WorkerJobContext};
pub use state::{SlotGuard, StateError, WorkerEvent, WorkerState, WorkerStateMachine};
