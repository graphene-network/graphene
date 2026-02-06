//! End-to-end tests module
//!
//! These tests require external dependencies like kraft CLI.
//! Run with: `cargo test -p graphene_node --features e2e-tests`

#![cfg(feature = "e2e-tests")]

mod unikraft_build;
