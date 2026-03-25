#![cfg(target_os = "linux")]

use graphene_node::executor::drive::linux::LinuxDriveBuilder;
use graphene_node::executor::drive::DriveConfig;
use graphene_node::executor::drive::ExecutionDriveBuilder;
use graphene_node::types::JobManifest;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use tokio::runtime::Builder;

fn cpio_list(path: &Path) -> Vec<String> {
    let out = Command::new("cpio")
        .args(["-t", "--file", path.to_str().unwrap()])
        .output()
        .expect("cpio list");
    assert!(
        out.status.success(),
        "cpio list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn cpio_cat(path: &Path, file: &str) -> String {
    let out = Command::new("cpio")
        .args(["-i", "--to-stdout", "--file", path.to_str().unwrap(), file])
        .output()
        .expect("cpio cat");
    assert!(
        out.status.success(),
        "cpio cat {} failed: {}",
        file,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn find_entry(entries: &[String], suffix: &str) -> String {
    entries
        .iter()
        .find(|entry| entry.ends_with(suffix))
        .cloned()
        .unwrap_or_else(|| panic!("cpio entry not found: {}", suffix))
}

#[test]
fn initrd_contains_main_py() {
    let work = tempdir().expect("tmpdir");
    let builder = LinuxDriveBuilder::new(DriveConfig {
        work_dir: work.path().to_path_buf(),
        image_size_mb: 16,
    });

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 30_000,
        runtime: "python:3.12".to_string(),
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

    let entries = cpio_list(&image_path);
    let entry = find_entry(&entries, "app/main.py");
    let contents = cpio_cat(&image_path, &entry);
    assert!(
        contents.contains("print('hello')"),
        "main.py contents not found: {}",
        contents
    );
}

#[test]
fn initrd_contains_index_js() {
    let work = tempdir().expect("tmpdir");
    let builder = LinuxDriveBuilder::new(DriveConfig {
        work_dir: work.path().to_path_buf(),
        image_size_mb: 16,
    });

    let manifest = JobManifest {
        vcpu: 1,
        memory_mb: 256,
        timeout_ms: 30_000,
        runtime: "node:21".to_string(),
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

    let entries = cpio_list(&image_path);
    let entry = find_entry(&entries, "app/index.js");
    let contents = cpio_cat(&image_path, &entry);
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
        runtime: "python:3.12".to_string(),
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

    let entries = cpio_list(&image_path);
    let entry = find_entry(&entries, "etc/graphene/env.json");
    let env_json = cpio_cat(&image_path, &entry);
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
        runtime: "python:3.12".to_string(),
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

    let entries = cpio_list(&image_path);
    let entry = find_entry(&entries, "input/input");
    let contents = cpio_cat(&image_path, &entry);
    assert_eq!(contents, "INPUT_DATA", "input contents mismatch");
}
