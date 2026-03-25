//! Worker module for job processing and lifecycle management.
//!
//! This module provides the core worker functionality for Graphene nodes:
//!
//! - **State machine** ([`WorkerStateMachine`]): Manages worker lifecycle from
//!   startup through graceful shutdown with atomic slot management.

pub mod state;

pub use state::{SlotGuard, StateError, WorkerEvent, WorkerState, WorkerStateMachine};
