//! End-to-end tests module
//!
//! These tests require external dependencies like kraft CLI.
//! Run with: `cargo test -p monad_node --features e2e`

#![cfg(feature = "e2e")]

mod unikraft_build;
