//! Integration tests for Docker container isolation.
//! Requires Docker daemon running and SYMPHONY_DOCKER_TESTS=1.

use chrono::Utc;
use serial_test::serial;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;
use tokio::process::Command;

use symphony::docker;
use symphony::domain::{DockerCodexAuth, DockerConfig, Issue};

fn docker_tests_enabled() -> bool {
    std::env::var("SYMPHONY_DOCKER_TESTS").unwrap_or_default() == "1"
}

fn unique_identifier(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn make_issue(identifier: String) -> Issue {
    Issue {
        id: identifier.clone(),
        identifier,
        title: "Docker integration test".to_string(),
        description: None,
        priority: None,
        state: "In Progress".to_string(),
        branch_name: None,
        url: None,
        assignee_id: None,
        labels: vec![],
        blocked_by: vec![],
        assigned_to_worker: true,
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
        children_count: 0,
        parent_identifier: None,
    }
}

fn non_root_worker_dockerfile() -> &'static str {
    r#"
FROM alpine:3.20
RUN adduser -D -u 10001 symphony
RUN mkdir -p /workspace && chown symphony:symphony /workspace
ENV HOME=/home/symphony
WORKDIR /workspace
USER symphony
"#
}

async fn build_image(tag: &str, dockerfile: &str) {
    let dir = tempdir().expect("tempdir should create");
    let dockerfile_path = dir.path().join("Dockerfile");
    std::fs::write(&dockerfile_path, dockerfile).expect("Dockerfile write should succeed");

    let output = Command::new("docker")
        .arg("build")
        .arg("-t")
        .arg(tag)
        .arg("-f")
        .arg(dockerfile_path.as_os_str())
        .arg(dir.path().as_os_str())
        .output()
        .await
        .expect("docker build should run");

    assert!(
        output.status.success(),
        "docker build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn remove_image_sync(tag: &str) {
    let _ = std::process::Command::new("docker")
        .args(["rmi", "-f", tag])
        .output();
}

struct ImageCleanupGuard {
    tag: String,
}

impl ImageCleanupGuard {
    fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Drop for ImageCleanupGuard {
    fn drop(&mut self) {
        remove_image_sync(&self.tag);
    }
}

struct EnvRestoreGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvRestoreGuard {
    fn capture(key: &'static str) -> Self {
        Self {
            key,
            previous: std::env::var(key).ok(),
        }
    }
}

impl Drop for EnvRestoreGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[tokio::test]
async fn test_docker_available() {
    if !docker_tests_enabled() {
        return;
    }
    assert!(docker::is_docker_available().await);
}

#[tokio::test]
#[serial]
async fn test_start_and_stop_container() {
    if !docker_tests_enabled() {
        return;
    }

    if !docker::is_docker_available().await {
        return;
    }

    let previous_api_key = std::env::var("OPENAI_API_KEY").ok();
    std::env::set_var("OPENAI_API_KEY", "sk-test");

    let issue = make_issue(unique_identifier("KAT-821-container"));
    let docker_config = DockerConfig {
        codex_auth: DockerCodexAuth::Env,
        ..DockerConfig::default()
    };

    let container_id = docker::start_container("alpine:3.20", &issue, &docker_config, &[])
        .await
        .expect("container should start");

    let inspect_output = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", &container_id])
        .output()
        .await
        .expect("docker inspect should run");
    assert!(
        String::from_utf8_lossy(&inspect_output.stdout).trim() == "true",
        "container should be running"
    );

    docker::stop_container(&container_id)
        .await
        .expect("container should stop");

    let inspect_after_stop = Command::new("docker")
        .args(["inspect", &container_id])
        .output()
        .await
        .expect("docker inspect after stop should run");
    assert!(
        !inspect_after_stop.status.success(),
        "container should be removed after stop"
    );

    if let Some(value) = previous_api_key {
        std::env::set_var("OPENAI_API_KEY", value);
    } else {
        std::env::remove_var("OPENAI_API_KEY");
    }
}

/// A5 verification command execution in Docker: create-before-start identity,
/// credential-free env, timeout stop/remove, and stopped-orphan cleanup.
#[tokio::test]
#[serial]
async fn test_verification_command_runs_in_docker_without_credentials() {
    if !docker_tests_enabled() {
        return;
    }
    if !docker::is_docker_available().await {
        return;
    }

    let previous_api_key = std::env::var("OPENAI_API_KEY").ok();
    std::env::set_var("OPENAI_API_KEY", "sk-test");
    let previous_gh_token = std::env::var("GH_TOKEN").ok();
    std::env::set_var("GH_TOKEN", "gh-secret-must-not-leak");

    let dir = tempdir().expect("tempdir should create");
    let workspace = dir.path().join("workspace");
    let evidence = dir.path().join("evidence");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    std::fs::create_dir_all(&evidence).expect("evidence dir");
    std::fs::create_dir_all(&home).expect("home dir");

    let request = symphony::verification::executor::CommandExecutionRequest {
        attempt_id: unique_identifier("docker-verification-attempt"),
        command_name: "docker-command".to_string(),
        workspace_path: workspace.clone(),
        evidence_dir: evidence.clone(),
        home_dir: home,
        command: "printf ok > /workspace/ran.txt; env".to_string(),
        timeout_ms: 120_000,
        execution_profile: symphony::implementation::domain::ExecutionProfile::Docker,
        docker: Some(DockerConfig {
            image: "alpine:3.20".to_string(),
            codex_auth: DockerCodexAuth::Env,
            ..DockerConfig::default()
        }),
    };

    let mut recorded = None;
    let result = symphony::verification::executor::execute_command(&request, |identity| {
        match identity {
            symphony::verification::executor::LaunchIdentity::Container { container_id } => {
                assert!(!container_id.is_empty());
                recorded = Some(container_id);
            }
            _ => panic!("expected container identity"),
        }
        Ok(())
    })
    .await
    .expect("docker command should run");

    assert!(result.passed, "command should pass: {:?}", result.output);
    assert!(result.exit_code == Some(0));
    assert!(
        workspace.join("ran.txt").is_file(),
        "workspace mount should work"
    );
    let env_output = format!("{}{}", result.output.stdout_tail, result.output.stderr_tail);
    assert!(
        !env_output.contains("gh-secret-must-not-leak"),
        "docker command env must not carry forge credentials"
    );
    assert!(
        !env_output.contains("GH_TOKEN"),
        "docker command env must omit GH_TOKEN"
    );
    // The container was created, recorded, started, and removed.
    assert!(recorded.is_some());
    let inspect = Command::new("docker")
        .args(["inspect", &recorded.expect("container id")])
        .output()
        .await
        .expect("docker inspect should run");
    assert!(
        !inspect.status.success(),
        "container should be removed after completion"
    );

    if let Some(value) = previous_api_key {
        std::env::set_var("OPENAI_API_KEY", value);
    } else {
        std::env::remove_var("OPENAI_API_KEY");
    }
    if let Some(value) = previous_gh_token {
        std::env::set_var("GH_TOKEN", value);
    } else {
        std::env::remove_var("GH_TOKEN");
    }
}

/// A timed-out Docker command is stopped and removed, and stopped label
/// orphans left by a crash are removed by recovery cleanup.
#[tokio::test]
#[serial]
async fn test_verification_docker_timeout_and_orphan_cleanup() {
    if !docker_tests_enabled() {
        return;
    }
    if !docker::is_docker_available().await {
        return;
    }

    let previous_api_key = std::env::var("OPENAI_API_KEY").ok();
    std::env::set_var("OPENAI_API_KEY", "sk-test");

    let dir = tempdir().expect("tempdir should create");
    let workspace = dir.path().join("workspace");
    let evidence = dir.path().join("evidence");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    std::fs::create_dir_all(&evidence).expect("evidence dir");
    std::fs::create_dir_all(&home).expect("home dir");

    let request = symphony::verification::executor::CommandExecutionRequest {
        attempt_id: unique_identifier("docker-timeout-attempt"),
        command_name: "docker-timeout".to_string(),
        workspace_path: workspace,
        evidence_dir: evidence,
        home_dir: home,
        command: "sleep 60".to_string(),
        timeout_ms: 3_000,
        execution_profile: symphony::implementation::domain::ExecutionProfile::Docker,
        docker: Some(DockerConfig {
            image: "alpine:3.20".to_string(),
            codex_auth: DockerCodexAuth::Env,
            ..DockerConfig::default()
        }),
    };

    let error = symphony::verification::executor::execute_command(&request, |_| Ok(()))
        .await
        .expect_err("timed-out command must fail");
    assert!(
        matches!(
            error,
            symphony::verification::executor::CommandRunFailure::TimedOut(_)
        ),
        "expected timeout, got {error}"
    );

    // A stopped label-matching orphan left by a crash (never started, never
    // recorded) must be removed by recovery cleanup.
    let orphan = Command::new("docker")
        .args([
            "create",
            "--label",
            "symphony.stage=verification",
            "--label",
            "symphony.attempt=crash-left-orphan",
            "alpine:3.20",
            "true",
        ])
        .output()
        .await
        .expect("docker create should run");
    assert!(orphan.status.success(), "orphan fixture should be created");
    let orphan_id = String::from_utf8_lossy(&orphan.stdout).trim().to_string();
    assert!(!orphan_id.is_empty(), "orphan fixture needs an id");

    let removed = symphony::verification::executor::cleanup_stopped_verification_containers()
        .await
        .expect("orphan cleanup should run");
    assert!(removed >= 1, "the stopped orphan must be removed");
    let inspect = Command::new("docker")
        .args(["inspect", &orphan_id])
        .output()
        .await
        .expect("docker inspect should run");
    assert!(
        !inspect.status.success(),
        "the stopped orphan must no longer exist"
    );

    if let Some(value) = previous_api_key {
        std::env::set_var("OPENAI_API_KEY", value);
    } else {
        std::env::remove_var("OPENAI_API_KEY");
    }
}

#[tokio::test]
#[serial]
async fn test_exec_in_container() {
    if !docker_tests_enabled() {
        return;
    }

    if !docker::is_docker_available().await {
        return;
    }

    let previous_api_key = std::env::var("OPENAI_API_KEY").ok();
    std::env::set_var("OPENAI_API_KEY", "sk-test");

    let issue = make_issue(unique_identifier("KAT-821-exec"));
    let docker_config = DockerConfig {
        codex_auth: DockerCodexAuth::Env,
        ..DockerConfig::default()
    };

    let container_id = docker::start_container("alpine:3.20", &issue, &docker_config, &[])
        .await
        .expect("container should start");

    let output = docker::exec_in_container(&container_id, "echo hello")
        .await
        .expect("docker exec should succeed");
    assert_eq!(output.trim(), "hello");

    docker::stop_container(&container_id)
        .await
        .expect("container should stop");

    if let Some(value) = previous_api_key {
        std::env::set_var("OPENAI_API_KEY", value);
    } else {
        std::env::remove_var("OPENAI_API_KEY");
    }
}

#[tokio::test]
#[serial]
async fn test_auth_resolution_auto() {
    if !docker_tests_enabled() {
        return;
    }

    let previous_api_key = std::env::var("OPENAI_API_KEY").ok();
    std::env::set_var("OPENAI_API_KEY", "sk-test");

    let (env_vars, volumes) =
        docker::resolve_codex_auth(DockerCodexAuth::Auto).expect("auto auth should resolve");
    assert!(!env_vars.is_empty());
    assert!(volumes.is_empty());

    if let Some(value) = previous_api_key {
        std::env::set_var("OPENAI_API_KEY", value);
    } else {
        std::env::remove_var("OPENAI_API_KEY");
    }
}

#[tokio::test]
#[serial]
async fn test_mount_auth_installs_in_non_root_home() {
    if !docker_tests_enabled() {
        return;
    }

    if !docker::is_docker_available().await {
        return;
    }

    let tag = unique_identifier("kat-903-non-root-mount");
    build_image(&tag, non_root_worker_dockerfile()).await;
    let _image_cleanup = ImageCleanupGuard::new(tag.clone());

    let _home_restore = EnvRestoreGuard::capture("HOME");
    let home = tempdir().expect("temp home should create");
    let codex_dir = home.path().join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("codex dir should create");
    let auth_path = codex_dir.join("auth.json");
    std::fs::write(&auth_path, "{}").expect("auth file should create");
    let mut permissions = std::fs::metadata(&auth_path)
        .expect("auth file metadata should exist")
        .permissions();
    permissions.set_mode(0o600);
    std::fs::set_permissions(&auth_path, permissions).expect("auth file permissions should update");
    std::env::set_var("HOME", home.path());

    let issue = make_issue(unique_identifier("KAT-903-mount"));
    let docker_config = DockerConfig {
        codex_auth: DockerCodexAuth::Mount,
        ..DockerConfig::default()
    };

    let container_id = docker::start_container(&tag, &issue, &docker_config, &[])
        .await
        .expect("container should start");

    let uid = docker::exec_in_container(&container_id, "id -u")
        .await
        .expect("id lookup should succeed");
    assert_ne!(uid.trim(), "0", "worker container should run as non-root");

    let auth_present = docker::exec_in_container(
        &container_id,
        "test -f \"$HOME/.codex/auth.json\" && echo present",
    )
    .await
    .expect("auth file probe should succeed");
    assert_eq!(auth_present.trim(), "present");

    docker::stop_container(&container_id)
        .await
        .expect("container should stop");
}

#[tokio::test]
#[serial]
async fn test_env_auth_mode_works_in_non_root_container() {
    if !docker_tests_enabled() {
        return;
    }

    if !docker::is_docker_available().await {
        return;
    }

    let tag = unique_identifier("kat-903-non-root-env");
    build_image(&tag, non_root_worker_dockerfile()).await;
    let _image_cleanup = ImageCleanupGuard::new(tag.clone());

    let _api_key_restore = EnvRestoreGuard::capture("OPENAI_API_KEY");
    std::env::set_var("OPENAI_API_KEY", "sk-test-env-mode");

    let issue = make_issue(unique_identifier("KAT-903-env"));
    let docker_config = DockerConfig {
        codex_auth: DockerCodexAuth::Env,
        ..DockerConfig::default()
    };

    let container_id = docker::start_container(&tag, &issue, &docker_config, &[])
        .await
        .expect("container should start");

    let uid = docker::exec_in_container(&container_id, "id -u")
        .await
        .expect("id lookup should succeed");
    assert_ne!(uid.trim(), "0", "worker container should run as non-root");

    let api_key = docker::exec_in_container(&container_id, "printenv OPENAI_API_KEY")
        .await
        .expect("OPENAI_API_KEY should be present");
    assert_eq!(api_key.trim(), "sk-test-env-mode");

    docker::stop_container(&container_id)
        .await
        .expect("container should stop");
}

#[tokio::test]
#[serial]
async fn test_setup_script_runs_as_root_and_restores_non_root_default_user() {
    if !docker_tests_enabled() {
        return;
    }

    if !docker::is_docker_available().await {
        return;
    }

    let base_tag = unique_identifier("kat-903-setup-base");
    build_image(&base_tag, non_root_worker_dockerfile()).await;
    let _base_image_cleanup = ImageCleanupGuard::new(base_tag.clone());

    let setup_dir = tempdir().expect("setup tempdir should create");
    let setup_script = setup_dir.path().join("setup.sh");
    std::fs::write(
        &setup_script,
        "#!/bin/sh\nset -eu\nif [ \"$(id -u)\" -ne 0 ]; then echo \"setup must run as root\" >&2; exit 1; fi\ntouch /tmp/kat-903-setup-ran\n",
    )
    .expect("setup script write should succeed");

    let derived_image = docker::resolve_image(
        &base_tag,
        Some(
            setup_script
                .to_str()
                .expect("setup path should be valid UTF-8"),
        ),
    )
    .await
    .expect("derived image should build");
    let _derived_image_cleanup = ImageCleanupGuard::new(derived_image.clone());

    let _api_key_restore = EnvRestoreGuard::capture("OPENAI_API_KEY");
    std::env::set_var("OPENAI_API_KEY", "sk-test-setup");

    let issue = make_issue(unique_identifier("KAT-903-setup"));
    let docker_config = DockerConfig {
        codex_auth: DockerCodexAuth::Env,
        ..DockerConfig::default()
    };
    let container_id = docker::start_container(&derived_image, &issue, &docker_config, &[])
        .await
        .expect("container should start");

    let uid = docker::exec_in_container(&container_id, "id -u")
        .await
        .expect("id lookup should succeed");
    assert_ne!(
        uid.trim(),
        "0",
        "derived image should restore non-root user"
    );

    let setup_marker = docker::exec_in_container(
        &container_id,
        "test -f /tmp/kat-903-setup-ran && echo present",
    )
    .await
    .expect("setup marker check should succeed");
    assert_eq!(setup_marker.trim(), "present");

    docker::stop_container(&container_id)
        .await
        .expect("container should stop");
}
