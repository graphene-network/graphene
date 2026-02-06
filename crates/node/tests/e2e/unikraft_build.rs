//! End-to-end tests for the Unikraft build pipeline.
//!
//! These tests require the `e2e` feature flag and the kraft CLI installed.
//! Run with: `cargo test -p graphene_node --features e2e-tests`

#![cfg(feature = "e2e-tests")]

use graphene_node::unikraft::{
    BuildJob, BuildManifest, KraftBuilder, KraftConfig, ResourceLimits, Runtime,
    UnikernelBuilder,
};
use std::path::PathBuf;

/// Create a test configuration for the KraftBuilder
fn test_config() -> KraftConfig {
    KraftConfig {
        kraft_bin: PathBuf::from("kraft"),
        cache_dir: std::env::temp_dir().join("graphene-e2e-test-cache"),
        build_timeout: std::time::Duration::from_secs(300),
    }
}

/// Test that building the Express example produces a valid unikernel
#[tokio::test]
#[ignore] // Requires kraft CLI and Linux
async fn test_build_express_app_produces_valid_unikernel() {
    let builder = KraftBuilder::new(test_config());
    let job = BuildJob::from_example("unikraft-node").expect("Failed to load example");

    let image = builder.build(&job).await.expect("Build failed");

    // Verify .unik exists
    assert!(image.path.exists(), "Unikernel file should exist");

    // Verify hash matches file contents
    let contents = std::fs::read(&image.path).expect("Failed to read unikernel");
    let computed_hash = blake3::hash(&contents);
    assert_eq!(
        image.hash,
        *computed_hash.as_bytes(),
        "Hash should match file contents"
    );

    // Verify size is non-zero
    assert!(image.size_bytes > 0, "Unikernel should have non-zero size");

    // Verify runtime
    assert_eq!(image.runtime, Runtime::Node20);

    // Clean up
    let _ = std::fs::remove_file(&image.path);
}

/// Test Dockerfile validation with the example
#[tokio::test]
async fn test_validate_example_dockerfile() {
    let builder = KraftBuilder::new(test_config());

    let dockerfile = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples/unikraft-node/Dockerfile"),
    )
    .expect("Failed to read example Dockerfile");

    let validated = builder
        .validate_dockerfile(&dockerfile)
        .expect("Validation should pass");

    assert_eq!(validated.runtime, Runtime::Node20);
    assert_eq!(validated.entrypoint, vec!["node", "index.js"]);
}

/// Test Kraftfile generation
#[test]
fn test_generate_kraftfile_for_example() {
    let builder = KraftBuilder::new(test_config());
    let manifest = BuildManifest {
        runtime: Runtime::Node20,
        entrypoint: vec!["node".to_string(), "index.js".to_string()],
        resources: ResourceLimits::default(),
    };

    let kraftfile = builder.generate_kraftfile(&manifest, "unikraft-node-example");

    assert_eq!(kraftfile.spec, "v0.6");
    assert_eq!(kraftfile.name, "unikraft-node-example");
    assert_eq!(kraftfile.runtime, "node:20");
    assert_eq!(kraftfile.rootfs, "./Dockerfile");
    assert_eq!(kraftfile.cmd, vec!["node", "index.js"]);
}

/// Test that invalid Dockerfiles are rejected
#[tokio::test]
async fn test_reject_invalid_dockerfile() {
    let builder = KraftBuilder::new(test_config());

    // Test with forbidden USER command
    let invalid_dockerfile = r#"
FROM graphene/node:20
USER node
CMD ["node", "index.js"]
"#;

    let result = builder.validate_dockerfile(invalid_dockerfile);
    assert!(result.is_err(), "Should reject USER command");

    // Test with unsupported base image
    let unsupported_base = r#"
FROM ubuntu:22.04
CMD ["bash"]
"#;

    let result = builder.validate_dockerfile(unsupported_base);
    assert!(result.is_err(), "Should reject unsupported base image");

    // Test with shell form CMD
    let shell_form = r#"
FROM graphene/node:20
CMD node index.js
"#;

    let result = builder.validate_dockerfile(shell_form);
    assert!(result.is_err(), "Should reject shell form CMD");
}
