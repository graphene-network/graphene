#![cfg(target_os = "linux")]

use monad_node::executor::drive::linux::LinuxDriveBuilder;
use monad_node::executor::drive::DriveConfig;
use monad_node::executor::drive::ExecutionDriveBuilder;
use monad_node::p2p::messages::JobManifest;
use std::collections::HashMap;
use std::process::Command;
use tempfile::tempdir;
use tokio::runtime::Builder;

fn ext4_cat(path: &std::path::Path, file: &str) -> String {
    let out = Command::new("debugfs")
        .args(["-R", &format!("cat {}", file), path.to_str().unwrap()])
        .output()
        .expect("debugfs cat");
    assert!(
        out.status.success(),
        "debugfs cat {} failed: {}",
        file,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn ext4_stat(path: &std::path::Path, file: &str) -> String {
    let out = Command::new("debugfs")
        .args(["-R", &format!("stat {}", file), path.to_str().unwrap()])
        .output()
        .expect("debugfs stat");
    assert!(
        out.status.success(),
        "debugfs stat {} failed: {}",
        file,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn ext4_contains_main_py() {
    let work = tempdir().expect("tmpdir");
    let builder = LinuxDriveBuilder::new(DriveConfig {
        work_dir: work.path().to_path_buf(),
        image_size_mb: 16,
    });

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 30_000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let code = b"print('hello')\n";

    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    let image_path = rt
        .block_on(builder.prepare("test-job", code, None, &manifest.env, &manifest))
        .expect("prepare drive");

    // Use debugfs to verify /app/main.py exists and is non-empty
    let stat = Command::new("debugfs")
        .args(["-R", "stat /app/main.py", image_path.to_str().unwrap()])
        .output()
        .expect("debugfs");
    assert!(
        stat.status.success(),
        "debugfs stat failed: {}",
        String::from_utf8_lossy(&stat.stderr)
    );

    let cat = Command::new("debugfs")
        .args(["-R", "cat /app/main.py", image_path.to_str().unwrap()])
        .output()
        .expect("debugfs cat");
    assert!(
        cat.status.success(),
        "debugfs cat failed: {}",
        String::from_utf8_lossy(&cat.stderr)
    );
    let contents = String::from_utf8_lossy(&cat.stdout);
    assert!(
        contents.contains("print('hello')"),
        "main.py contents not found: {}",
        contents
    );
}

#[test]
fn ext4_contains_index_js() {
    let work = tempdir().expect("tmpdir");
    let builder = LinuxDriveBuilder::new(DriveConfig {
        work_dir: work.path().to_path_buf(),
        image_size_mb: 16,
    });

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 30_000,
        kernel: "node:21".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let code = br#"console.log("hello");"#;

    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    let image_path = rt
        .block_on(builder.prepare("test-job-node", code, None, &manifest.env, &manifest))
        .expect("prepare drive");

    let stat = Command::new("debugfs")
        .args(["-R", "stat /app/index.js", image_path.to_str().unwrap()])
        .output()
        .expect("debugfs");
    assert!(
        stat.status.success(),
        "debugfs stat failed: {}",
        String::from_utf8_lossy(&stat.stderr)
    );

    let cat = Command::new("debugfs")
        .args(["-R", "cat /app/index.js", image_path.to_str().unwrap()])
        .output()
        .expect("debugfs cat");
    assert!(
        cat.status.success(),
        "debugfs cat failed: {}",
        String::from_utf8_lossy(&cat.stderr)
    );
    let contents = String::from_utf8_lossy(&cat.stdout);
    assert!(
        contents.contains("hello"),
        "index.js contents not found: {}",
        contents
    );
}

#[test]
fn env_json_has_reserved_vars() {
    let work = tempdir().expect("tmpdir");
    let builder = LinuxDriveBuilder::new(DriveConfig {
        work_dir: work.path().to_path_buf(),
        image_size_mb: 16,
    });

    let mut user_env = HashMap::new();
    user_env.insert("USER_VAR".to_string(), "abc".to_string());
    user_env.insert(
        "GRAPHENE_JOB_ID".to_string(),
        "should_be_overridden".to_string(),
    );

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 42_000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: user_env,
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    let image_path = rt
        .block_on(builder.prepare("env-test", b"print('hi')", None, &manifest.env, &manifest))
        .expect("prepare drive");

    let env_json = ext4_cat(&image_path, "/etc/graphene/env.json");
    assert!(
        env_json.contains("\"GRAPHENE_JOB_ID\""),
        "env.json missing GRAPHENE_JOB_ID: {}",
        env_json
    );
    assert!(
        env_json.contains("\"GRAPHENE_TIMEOUT_MS\": \"42000\""),
        "env.json missing timeout: {}",
        env_json
    );
    assert!(
        env_json.contains("\"GRAPHENE_INPUT_PATH\": \"/input\""),
        "env.json missing input path: {}",
        env_json
    );
    assert!(
        env_json.contains("\"USER_VAR\": \"abc\""),
        "env.json missing user var: {}",
        env_json
    );
    assert!(
        !env_json.contains("should_be_overridden"),
        "reserved var override leak: {}",
        env_json
    );
}

#[test]
fn input_inline_written_to_input_dir() {
    let work = tempdir().expect("tmpdir");
    let builder = LinuxDriveBuilder::new(DriveConfig {
        work_dir: work.path().to_path_buf(),
        image_size_mb: 16,
    });

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 10_000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: HashMap::new(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    };

    let code = b"print('hi')";
    let input = b"INPUT_DATA";

    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    let image_path = rt
        .block_on(builder.prepare("input-test", code, Some(input), &manifest.env, &manifest))
        .expect("prepare drive");

    let stat = ext4_stat(&image_path, "/input/input");
    assert!(
        stat.contains("Size: 10"),
        "input file size unexpected: {}",
        stat
    );
    let contents = ext4_cat(&image_path, "/input/input");
    assert_eq!(contents, "INPUT_DATA", "input contents mismatch");
}
