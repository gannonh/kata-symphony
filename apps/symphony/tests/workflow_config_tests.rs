//! Integration tests for workflow parsing, config extraction, and Liquid rendering.
//!
//! These tests define and verify the behavioral contract for slice S02:
//! WORKFLOW.md parsing, typed config extraction, env-var resolution,
//! tilde expansion, and WorkflowStore hot-reload.

use serial_test::serial;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

use symphony::config::{from_workflow, validate};
use symphony::domain::{
    AgentBackend, CodexConfig, DockerCodexAuth, GithubProjectOwnerType, ServiceConfig,
    TrackerConfig, WorkspaceConfig, WorkspaceIsolation, WorkspaceRepoStrategy,
};
use symphony::error::SymphonyError;
use symphony::notifications::should_notify;
use symphony::workflow::parse_workflow;
use symphony::workflow_store::WorkflowStore;

// ─────────────────────────────────────────────────────────────────────────────
// Parse group
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_parse_workflow_happy_path() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "---\ntracker:\n  kind: linear\n---\nHello {{{{ issue.id }}}}"
    )
    .unwrap();

    let result = parse_workflow(file.path());
    let def = result.expect("happy-path parse should succeed");

    assert!(
        def.prompt_template.contains("Hello"),
        "prompt_template should contain 'Hello', got: {:?}",
        def.prompt_template
    );
    let kind = def.config["tracker"]["kind"].as_str().unwrap_or("");
    assert_eq!(kind, "linear");
}

#[test]
fn test_parse_workflow_no_delimiter() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "Hello {{{{ issue.id }}}}").unwrap();

    let result = parse_workflow(file.path());
    let def = result.expect("no-delimiter parse should succeed");

    assert!(
        def.prompt_template.contains("Hello"),
        "whole file should become prompt, got: {:?}",
        def.prompt_template
    );
    // config yields an empty/null/default mapping (no front matter)
    assert!(
        def.config.is_mapping() || def.config.is_null(),
        "config should be empty mapping or null, got: {:?}",
        def.config
    );
}

#[test]
fn test_parse_workflow_delimiter_not_on_first_line_is_not_front_matter() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "Intro paragraph\n---\nthis-is-not-front-matter\n---\nPrompt body"
    )
    .unwrap();

    let result = parse_workflow(file.path());
    let def = result.expect("non-leading delimiters should be treated as prompt text");

    assert!(
        def.config.as_mapping().is_some_and(|m| m.is_empty()),
        "config should be empty when front matter does not start at line 1, got: {:?}",
        def.config
    );
    assert!(
        def.prompt_template.contains("Intro paragraph"),
        "prompt should preserve text before delimiter, got: {:?}",
        def.prompt_template
    );
    assert!(
        def.prompt_template.contains("---"),
        "prompt should preserve markdown horizontal rule delimiters, got: {:?}",
        def.prompt_template
    );
}

#[test]
fn test_parse_workflow_empty_yaml() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "---\n\n---\nSome prompt text").unwrap();

    let result = parse_workflow(file.path());
    let def = result.expect("empty YAML front matter should succeed with defaults");

    assert!(
        def.prompt_template.contains("Some prompt text"),
        "prompt should contain body text, got: {:?}",
        def.prompt_template
    );
}

#[test]
fn test_parse_workflow_non_map_yaml() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "---\n- list item\n---\nSome prompt").unwrap();

    let result = parse_workflow(file.path());
    assert!(
        matches!(result, Err(SymphonyError::WorkflowFrontMatterNotAMap)),
        "non-map YAML front matter should return WorkflowFrontMatterNotAMap, got: {:?}",
        result
    );
}

#[test]
fn test_repo_workflow_requires_publish_gate_before_agent_review() {
    // The publish-gate contract now lives in the per-state prompt file
    let prompt_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("prompts/in-progress.md");
    let content = std::fs::read_to_string(&prompt_path)
        .expect("prompts/in-progress.md should exist for publish-gate contract assertions");

    assert!(
        content.contains("git ls-remote --exit-code --heads origin"),
        "in-progress.md must require explicit remote-branch proof before Agent Review"
    );
    assert!(
        content.contains("gh pr view --json url,state,headRefName,baseRefName"),
        "in-progress.md must require explicit PR proof before Agent Review"
    );
    assert!(
        content.contains("Agent Review"),
        "in-progress.md must transition to Agent Review (not Human Review)"
    );
    assert!(
        content.contains("Move the issue to `Agent Review`"),
        "in-progress.md must instruct workers to move issue to Agent Review"
    );
    assert!(
        !content.contains("phase: \"verifying\""),
        "in-progress.md must not use verifying as a PR-review handoff phase"
    );
}

#[test]
fn test_agent_review_prompt_transitions_to_human_review() {
    let prompt_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("prompts/agent-review.md");
    let content = std::fs::read_to_string(&prompt_path)
        .expect("prompts/agent-review.md should exist for review-transition assertions");

    assert!(
        content.contains("move the issue to `Human Review`"),
        "agent-review.md must advance to human-review after feedback is resolved"
    );
    assert!(
        !content.contains("phase: \"verifying\""),
        "agent-review.md must not use verifying as a human-review handoff phase"
    );
}

#[test]
fn test_repo_workflow_example_uses_per_state_prompts() {
    let workflow_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("WORKFLOW.md");
    let def = parse_workflow(&workflow_path).expect("active WORKFLOW.md should parse");
    let mut test_config = def.config.clone();
    if let Some(map) = test_config.as_mapping_mut() {
        map.remove(serde_yaml::Value::String("notifications".to_string()));
    }

    // When using per-state prompts, the body after --- should be empty or minimal
    // The config should have a prompts section
    let config = from_workflow(&test_config).expect("config should parse");
    assert_eq!(config.agent_backend, AgentBackend::KataCli);
    assert_eq!(
        config.pi_agent.command,
        vec!["pi".to_string(), "--mode".to_string(), "rpc".to_string()]
    );
    assert!(
        config.prompts.is_some(),
        "active WORKFLOW.md should use per-state prompts config"
    );
    let prompts = config.prompts.unwrap();
    assert!(prompts.system.is_some(), "should have system prompt");
    assert!(prompts.repo.is_some(), "should have repo prompt");
    assert!(
        !prompts.by_state.is_empty(),
        "should have by_state mappings"
    );
    assert!(
        prompts.by_state.contains_key("in progress"),
        "should map 'in progress' state"
    );
    assert!(
        prompts.by_state.contains_key("agent review"),
        "should map 'agent review' state"
    );
}

#[test]
fn test_worker_prompts_do_not_reference_legacy_kata_tools() {
    let prompts_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("prompts");
    let files = [
        "system.md",
        "in-progress.md",
        "agent-review.md",
        "rework.md",
        "merging.md",
    ];

    let forbidden = [
        "kata_get_issue",
        "kata_list_tasks",
        "kata_read_document",
        "kata_write_document",
        "kata_upsert_comment",
        "kata_update_issue_state",
        "kata_create_followup_issue",
        "linear_get_issue",
        "linear_update_issue",
        "linear_add_comment",
        "linear_graphql",
        "You are working on a Linear ticket",
        ".agents/skills/sym-",
    ];

    for file in files {
        let content =
            std::fs::read_to_string(prompts_dir.join(file)).expect("prompt file should exist");

        for needle in forbidden {
            assert!(
                !content.contains(needle),
                "{file} must not include unavailable or backend-specific token {needle}"
            );
        }

        assert!(
            content.contains("$SYMPHONY_BIN")
                || content.contains("direct Symphony helper contract"),
            "{file} must route tracker operations through the Symphony helper"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config group
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_config_defaults() {
    let empty = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    let config = from_workflow(&empty).expect("empty config should produce spec §5.3 defaults");

    assert_eq!(config.tracker.kind, None);
    assert_eq!(config.tracker.api_key, None);
    assert_eq!(config.tracker.project_slug, None);
    assert_eq!(config.tracker.workspace_slug, None);
    assert_eq!(config.tracker.repo_owner, None);
    assert_eq!(config.tracker.repo_name, None);
    assert_eq!(config.tracker.github_project_owner_type, None);
    assert_eq!(config.tracker.github_project_number, None);
    assert_eq!(config.tracker.label_prefix, None);
    assert_eq!(config.polling.interval_ms, 30_000);
    assert_eq!(config.agent.max_concurrent_agents, 10);
    assert_eq!(config.agent.max_turns, 20);
    assert_eq!(config.agent.escalation_timeout_ms, 300_000);
    assert_eq!(config.workspace.repo, None);
    assert_eq!(config.workspace.strategy, WorkspaceRepoStrategy::Auto);
    assert_eq!(config.workspace.isolation, WorkspaceIsolation::Local);
    assert_eq!(config.workspace.branch_prefix, "symphony");
    assert_eq!(config.workspace.clone_branch, None);
    assert_eq!(config.workspace.base_branch.as_deref(), Some("main"));
    assert!(!config.workspace.cleanup_on_done);
    assert_eq!(config.agent_backend, AgentBackend::KataCli);
    assert_eq!(
        config.pi_agent.command,
        vec!["pi".to_string(), "--mode".to_string(), "rpc".to_string()]
    );
    assert_eq!(config.pi_agent.model, None);
    assert!(config.pi_agent.model_by_label.is_empty());
    assert!(config.pi_agent.model_by_state.is_empty());
    assert!(config.pi_agent.no_session);
    assert_eq!(config.pi_agent.append_system_prompt, None);
    assert_eq!(config.pi_agent.read_timeout_ms, 5_000);
    assert_eq!(config.pi_agent.stall_timeout_ms, 300_000);
    assert_eq!(config.server.public_url, None);
    assert!(config.notifications.is_none());
    assert!(!config.triage.enabled);
    assert_eq!(
        config.triage.mode,
        symphony::triage::domain::TriageMode::Preview
    );
    assert_eq!(config.triage.intake_label, "needs-triage");
    assert_eq!(config.triage.prompt, "prompts/triage.md");
    assert_eq!(config.triage.turn_timeout_ms, 900_000);
    assert_eq!(config.triage.max_attempts, 3);
    assert_eq!(config.triage.max_intake_pages, 100);
    assert_eq!(config.triage.routes.implement.label, "ready-for-agent");
    assert_eq!(
        config.triage.routes.implement.state.as_deref(),
        Some("Todo")
    );
    assert_eq!(config.storage.path, None);
    assert_eq!(config.storage.busy_timeout_ms, 5_000);
    assert!(!config.spec.enabled);
    assert_eq!(config.spec.intake_label, "ready-to-spec");
    assert_eq!(config.spec.max_review_cycles, 3);
    assert_eq!(config.spec.labels.approved, "spec-approved");
    assert_eq!(config.spec.approval_route.label, "ready-for-agent");
}

#[test]
fn test_triage_preview_mode_parses_with_routes() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: org
  github_project_number: 42
agent:
  name: pi
storage:
  path: ~/symphony-state.sqlite3
  busy_timeout_ms: 2500
triage:
  enabled: true
  mode: preview
  intake_label: needs-triage
  prompt: prompts/custom-triage.md
  model: anthropic/claude-sonnet-4-6
  turn_timeout_ms: 600000
  max_attempts: 2
  max_intake_pages: 50
  routes:
    implement:
      label: ready-for-agent
      state: Todo
    spec:
      label: ready-to-spec
    needs_information:
      label: needs-info
    park:
      label: wait-to-implement
    human_owned:
      label: ready-for-human
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("triage preview config should parse");

    assert!(config.triage.enabled);
    assert_eq!(
        config.triage.mode,
        symphony::triage::domain::TriageMode::Preview
    );
    assert_eq!(config.triage.intake_label, "needs-triage");
    assert_eq!(config.triage.prompt, "prompts/custom-triage.md");
    assert_eq!(
        config.triage.model.as_deref(),
        Some("anthropic/claude-sonnet-4-6")
    );
    assert_eq!(config.triage.turn_timeout_ms, 600_000);
    assert_eq!(config.triage.max_attempts, 2);
    assert_eq!(config.triage.max_intake_pages, 50);
    assert_eq!(config.triage.routes.implement.label, "ready-for-agent");
    assert_eq!(
        config.triage.routes.implement.state.as_deref(),
        Some("Todo")
    );
    assert_eq!(config.triage.routes.spec.label, "ready-to-spec");
    assert_eq!(config.triage.routes.needs_information.label, "needs-info");
    assert_eq!(config.triage.routes.park.label, "wait-to-implement");
    assert_eq!(config.triage.routes.human_owned.label, "ready-for-human");
    assert_eq!(config.storage.busy_timeout_ms, 2_500);
    assert!(
        config
            .storage
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("/symphony-state.sqlite3")
                && !path.starts_with('~')),
        "storage.path should expand tilde, got: {:?}",
        config.storage.path
    );

    validate(&config).expect("valid triage preview config should pass validation");
}

#[test]
fn test_spec_stage_parses_and_validates_full_configuration() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: org
  github_project_number: 42
agent:
  name: pi
spec:
  enabled: true
  intake_label: ready-to-spec
  prompts:
    draft: prompts/my-draft.md
    review: prompts/my-review.md
    revise: prompts/my-revise.md
  model: draft-model
  review_model: review-model
  turn_timeout_ms: 120000
  max_intake_pages: 20
  max_review_cycles: 2
  max_attempts: 4
  max_revision_requests: 5
  labels:
    approved: approve-it
    revise: revise-it
  approval_route:
    label: implement-it
    state: Todo
"#;
    let config = parse_yaml_config(yaml_str);
    assert!(config.spec.enabled);
    assert_eq!(config.spec.prompts.draft, "prompts/my-draft.md");
    assert_eq!(config.spec.model.as_deref(), Some("draft-model"));
    assert_eq!(config.spec.review_model.as_deref(), Some("review-model"));
    assert_eq!(config.spec.turn_timeout_ms, 120_000);
    assert_eq!(config.spec.max_intake_pages, 20);
    assert_eq!(config.spec.max_review_cycles, 2);
    assert_eq!(config.spec.max_attempts, 4);
    assert_eq!(config.spec.max_revision_requests, 5);
    assert_eq!(config.spec.labels.approved, "approve-it");
    assert_eq!(config.spec.approval_route.label, "implement-it");
    validate(&config).expect("valid spec configuration");
}

#[test]
fn test_spec_rejects_codex_models_and_label_collisions() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: org
  github_project_number: 42
agent:
  name: codex
spec:
  enabled: true
  model: forbidden
"#;
    let config = parse_yaml_config(yaml_str);
    let error = validate(&config).expect_err("Codex model override must fail");
    assert!(error.to_string().contains("spec.model"));

    let collision = parse_yaml_config(
        r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: org
  github_project_number: 42
spec:
  enabled: true
  labels:
    approved: ready-to-spec
"#,
    );
    assert!(validate(&collision)
        .expect_err("managed labels must be distinct")
        .to_string()
        .contains("distinct labels"));
}

#[test]
fn test_triage_rejects_duplicate_route_labels() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: org
  github_project_number: 42
triage:
  enabled: true
  routes:
    implement:
      label: ready-for-agent
    spec:
      label: ready-for-agent
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("duplicate labels should parse");
    let err = validate(&config).expect_err("duplicate route labels must fail validation");
    assert!(
        matches!(
            err,
            SymphonyError::InvalidWorkflowConfig(ref msg)
                if msg.contains("triage route labels must be distinct")
                    && msg.contains("ready-for-agent")
        ),
        "expected duplicate route label validation failure, got: {err}"
    );
}

#[test]
fn test_triage_rejects_model_when_agent_is_codex() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: org
  github_project_number: 42
agent:
  name: codex
triage:
  enabled: true
  model: anthropic/claude-sonnet-4-6
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("codex triage model config should parse");
    let err = validate(&config).expect_err("triage.model with codex must fail validation");
    assert!(
        matches!(
            err,
            SymphonyError::InvalidWorkflowConfig(ref msg)
                if msg.contains("triage.model is not supported when agent.name is 'codex'")
        ),
        "expected codex triage.model rejection, got: {err}"
    );
}

#[test]
fn test_triage_rejects_route_label_equal_to_intake_label() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: org
  github_project_number: 42
triage:
  enabled: true
  intake_label: needs-triage
  routes:
    park:
      label: needs-triage
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("intake-equal route label should parse");
    let err = validate(&config).expect_err("route label equal to intake must fail validation");
    assert!(
        matches!(
            err,
            SymphonyError::InvalidWorkflowConfig(ref msg)
                if msg.contains("must not equal triage.intake_label")
                    && msg.contains("park")
        ),
        "expected intake-equal route label rejection, got: {err}"
    );
}

#[test]
fn test_storage_busy_timeout_ms_zero_fails_validation() {
    let yaml_str = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-project
storage:
  busy_timeout_ms: 0
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("busy_timeout_ms=0 should parse");
    assert_eq!(config.storage.busy_timeout_ms, 0);

    let err = validate(&config).expect_err("busy_timeout_ms=0 must fail validation");
    assert!(
        matches!(
            err,
            SymphonyError::InvalidWorkflowConfig(ref msg)
                if msg.contains("storage.busy_timeout_ms must be greater than 0")
        ),
        "expected busy_timeout_ms validation failure, got: {err}"
    );
}

#[test]
fn test_github_tracker_config_parses() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: org
  github_project_number: 42
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("github tracker config should parse");

    assert_eq!(config.tracker.kind.as_deref(), Some("github"));
    assert_eq!(config.tracker.repo_owner.as_deref(), Some("kata-sh"));
    assert_eq!(config.tracker.repo_name.as_deref(), Some("kata-mono"));
    assert_eq!(
        config.tracker.github_project_owner_type,
        Some(GithubProjectOwnerType::Org)
    );
    assert_eq!(config.tracker.github_project_number, Some(42));
    assert_eq!(config.tracker.label_prefix, None);
    assert_eq!(config.tracker.project_slug, None);
}

#[test]
fn test_github_tracker_config_missing_repo_owner_errors() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_name: kata-mono
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("missing repo_owner should fail for github tracker");

    assert!(
        matches!(err, SymphonyError::InvalidWorkflowConfig(ref msg) if msg == "tracker.repo_owner is required when tracker.kind is github"),
        "expected github repo_owner validation error, got: {err}"
    );
}

#[test]
fn test_github_tracker_config_requires_project_number() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: user
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("github tracker without project number should fail");
    assert!(
        err.to_string().contains("tracker.github_project_number"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_github_tracker_config_ignores_legacy_label_prefix_when_project_number_is_set() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: user
  github_project_number: 42
  label_prefix: orchestration
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("projects v2 github tracker should parse");

    assert_eq!(config.tracker.github_project_number, Some(42));
    assert_eq!(config.tracker.label_prefix, None);
}

#[test]
fn test_github_tracker_config_requires_project_owner_type() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_number: 42
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("missing project owner type should fail");
    assert!(
        err.to_string()
            .contains("tracker.github_project_owner_type"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_github_tracker_config_rejects_invalid_project_owner_type() {
    let yaml_str = r#"
tracker:
  kind: github
  api_key: github-test-token
  repo_owner: kata-sh
  repo_name: kata-mono
  github_project_owner_type: team
  github_project_number: 42
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("invalid project owner type should fail");
    assert!(
        err.to_string()
            .contains("tracker.github_project_owner_type must be either 'user' or 'org'"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_linear_config_unaffected() {
    let yaml_str = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: my-project
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("linear config should parse");

    assert_eq!(config.tracker.kind.as_deref(), Some("linear"));
    assert_eq!(config.tracker.project_slug.as_deref(), Some("my-project"));
    assert_eq!(config.tracker.repo_owner, None);
    assert_eq!(config.tracker.repo_name, None);
    assert_eq!(config.tracker.label_prefix, None);
}

#[test]
fn test_linear_lifecycle_config_uses_existing_tracker_fields() {
    let content = r#"---
tracker:
  kind: linear
  api_key: test-key
  endpoint: http://127.0.0.1:4010/graphql
  project_slug: kata-project
  workspace_slug: kata-sh
  active_states:
    - Todo
    - In Progress
  terminal_states:
    - Done
  exclude_labels:
    - kata:task
---
Prompt
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();

    let def = parse_workflow(file.path()).expect("workflow parses");
    let config = from_workflow(&def.config).expect("config loads");

    assert_eq!(config.tracker.kind.as_deref(), Some("linear"));
    assert_eq!(config.tracker.project_slug.as_deref(), Some("kata-project"));
    assert_eq!(config.tracker.workspace_slug.as_deref(), Some("kata-sh"));
    assert_eq!(config.tracker.active_states, vec!["Todo", "In Progress"]);
    assert_eq!(config.tracker.terminal_states, vec!["Done"]);
    assert_eq!(config.tracker.exclude_labels, vec!["kata:task"]);
}

#[test]
fn test_escalation_timeout_parses_from_agent_field() {
    let yaml_str = r#"
agent:
  escalation_timeout_ms: 120000
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("agent escalation timeout should parse");
    assert_eq!(config.agent.escalation_timeout_ms, 120_000);
}

#[test]
fn test_escalation_timeout_parses_from_escalation_section() {
    let yaml_str = r#"
escalation:
  timeout_ms: 90000
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("escalation timeout section should parse");
    assert_eq!(config.agent.escalation_timeout_ms, 90_000);
}

#[test]
fn test_agent_escalation_timeout_precedence_over_escalation_section() {
    let yaml_str = r#"
agent:
  escalation_timeout_ms: 75000
escalation:
  timeout_ms: 90000
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("agent field should win");
    assert_eq!(config.agent.escalation_timeout_ms, 75_000);
}

#[test]
fn test_shared_context_max_entries_zero_is_not_normalized() {
    let yaml_str = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-project
shared_context:
  max_entries: 0
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workflow config should parse");

    assert_eq!(
        config.shared_context.max_entries, 0,
        "from_workflow should preserve explicit zero values for validation"
    );

    let err = validate(&config).expect_err("shared_context.max_entries=0 must fail validation");
    assert!(
        matches!(err, SymphonyError::InvalidWorkflowConfig(ref msg) if msg.contains("shared_context.max_entries must be greater than 0")),
        "expected shared_context.max_entries validation failure, got: {err}"
    );
}

#[test]
fn test_server_public_url_parses_and_trims_trailing_slash() {
    let yaml_str = r#"
server:
  host: 0.0.0.0
  port: 8080
  public_url: "https://symphony.example.com/"
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("server config should parse");

    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, Some(8080));
    assert_eq!(
        config.server.public_url.as_deref(),
        Some("https://symphony.example.com")
    );
}

#[test]
fn test_server_public_url_rejects_malformed_or_relative_values() {
    for public_url in ["not-a-url", "/dashboard", "ftp://example.com"] {
        let yaml_str =
            format!("server:\n  host: 0.0.0.0\n  port: 8080\n  public_url: \"{public_url}\"\n");
        let raw: serde_yaml::Value = serde_yaml::from_str(&yaml_str).unwrap();
        let err = from_workflow(&raw).expect_err("invalid server.public_url should fail parsing");

        assert!(
            matches!(err, SymphonyError::InvalidWorkflowConfig(ref msg) if msg.contains("server.public_url")),
            "expected invalid public_url error for '{public_url}', got: {err}"
        );
    }
}

#[test]
fn test_agent_name_codex_uses_agent_command() {
    let yaml_str = r#"
agent:
  name: codex
  command:
    - codex
    - --model
    - gpt-5.3-codex
    - app-server
  stall_timeout_ms: 900000
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("codex agent config should parse");

    assert_eq!(config.agent_backend, AgentBackend::Codex);
    assert_eq!(
        config.codex.command,
        vec![
            "codex".to_string(),
            "--model".to_string(),
            "gpt-5.3-codex".to_string(),
            "app-server".to_string()
        ]
    );
    assert_eq!(config.codex.stall_timeout_ms, 900_000);
}

#[test]
fn test_agent_name_pi_uses_agent_command_and_params() {
    let yaml_str = r#"
agent:
  name: pi
  command: "pi --mode rpc"
  model: "anthropic/claude-sonnet-4-6"
  no_session: false
  append_system_prompt: "/tmp/system.md"
  read_timeout_ms: 1200
  stall_timeout_ms: 90000
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("pi agent config should parse");

    assert_eq!(config.agent_backend, AgentBackend::KataCli);
    assert_eq!(
        config.pi_agent.command,
        vec!["pi".to_string(), "--mode".to_string(), "rpc".to_string()]
    );
    assert_eq!(
        config.pi_agent.model.as_deref(),
        Some("anthropic/claude-sonnet-4-6")
    );
    assert!(config.pi_agent.model_by_label.is_empty());
    assert!(config.pi_agent.model_by_state.is_empty());
    assert!(!config.pi_agent.no_session);
    assert_eq!(
        config.pi_agent.append_system_prompt.as_deref(),
        Some("/tmp/system.md")
    );
    assert_eq!(config.pi_agent.read_timeout_ms, 1200);
    assert_eq!(config.pi_agent.stall_timeout_ms, 90_000);
}

#[test]
fn test_legacy_kata_runtime_name_is_rejected() {
    let yaml_str = r#"
agent:
  name: kata-cli
  command: "kata"
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("kata-cli is not a worker runtime name");
    assert!(
        err.to_string()
            .contains("agent.name must be 'pi' or 'codex'"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_pi_agent_model_by_label_normalizes_keys() {
    let yaml_str = r#"
agent:
  name: pi
  command: pi --mode rpc
  model_by_label:
    Model:Sonnet: anthropic/claude-sonnet-4-6
    MODEL:OPUS: anthropic/claude-opus-4-6
    "  ": ignored
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("model_by_label config should parse");

    assert_eq!(
        config
            .pi_agent
            .model_by_label
            .get("model:sonnet")
            .map(String::as_str),
        Some("anthropic/claude-sonnet-4-6")
    );
    assert_eq!(
        config
            .pi_agent
            .model_by_label
            .get("model:opus")
            .map(String::as_str),
        Some("anthropic/claude-opus-4-6")
    );
    assert!(!config.pi_agent.model_by_label.contains_key("Model:Sonnet"));
    assert!(
        !config.pi_agent.model_by_label.contains_key(""),
        "blank label keys should be ignored"
    );
}

#[test]
fn test_pi_agent_model_by_state_normalizes_keys() {
    let yaml_str = r#"
agent:
  name: pi
  command: pi --mode rpc
  model: anthropic/claude-opus-4-6
  model_by_state:
    Agent Review: anthropic/claude-sonnet-4-6
    MERGING: anthropic/claude-sonnet-4-6
    "  ": ignored
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("model_by_state config should parse");

    assert_eq!(
        config
            .pi_agent
            .model_by_state
            .get("agent review")
            .map(String::as_str),
        Some("anthropic/claude-sonnet-4-6")
    );
    assert_eq!(
        config
            .pi_agent
            .model_by_state
            .get("merging")
            .map(String::as_str),
        Some("anthropic/claude-sonnet-4-6")
    );
    assert!(!config.pi_agent.model_by_state.contains_key("Agent Review"));
    assert!(
        !config.pi_agent.model_by_state.contains_key(""),
        "blank state keys should be ignored"
    );
}

#[test]
fn test_agent_backend_aliases_map_to_kata_cli() {
    let raw: serde_yaml::Value =
        serde_yaml::from_str("agent:\n  name: pi\n  command: pi --mode rpc\n").unwrap();
    let config = from_workflow(&raw).expect("pi agent name should parse");
    assert_eq!(config.agent_backend, AgentBackend::KataCli);
}

#[test]
fn test_pi_agent_section_still_supported_as_alias() {
    let yaml_str = r#"
agent:
  name: pi
pi_agent:
  command: "pi --mode rpc"
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("pi_agent alias section should parse");
    assert_eq!(config.agent_backend, AgentBackend::KataCli);
    assert_eq!(
        config.pi_agent.command,
        vec!["pi".to_string(), "--mode".to_string(), "rpc".to_string()]
    );
}

#[test]
fn test_kata_agent_and_pi_agent_sections_conflict() {
    let yaml_str = r#"
agent:
  name: pi
kata_agent:
  command: "kata"
pi_agent:
  command: "kata"
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("dual agent sections should fail");
    assert!(
        err.to_string()
            .contains("only one of 'kata_agent' or 'pi_agent'"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_agent_backend_invalid_value_errors() {
    let yaml_str = r#"
agent:
  name: unknown
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("unknown agent name should fail");
    assert!(
        err.to_string().contains("agent.name"),
        "unexpected error: {err}"
    );
}

#[test]
#[serial]
fn test_config_env_var_resolution() {
    // Note: use a test-unique env var name to limit parallel-test interference.
    std::env::set_var("SYMPHONY_TEST_LINEAR_API_KEY", "test-key");

    let yaml_str = "tracker:\n  api_key: $SYMPHONY_TEST_LINEAR_API_KEY";
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("$ENV_VAR should be resolved to its value");

    assert_eq!(
        config.tracker.api_key.as_deref(),
        Some("test-key"),
        "api_key should be resolved from env"
    );

    std::env::remove_var("SYMPHONY_TEST_LINEAR_API_KEY");
}

#[test]
fn test_notifications_config_parses_all_fields() {
    let yaml_str = r#"
notifications:
  slack:
    webhook_url: https://hooks.slack.com/services/T000/B000/abc123
    events:
      - human_review
      - stalled
      - failed
      - rework
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("notifications config should parse");

    let notifications = config
        .notifications
        .as_ref()
        .expect("notifications should be present");
    let slack = notifications
        .slack
        .as_ref()
        .expect("slack config should be present");

    assert_eq!(
        slack.webhook_url,
        "https://hooks.slack.com/services/T000/B000/abc123"
    );
    assert_eq!(
        slack.events,
        vec!["human_review", "stalled", "failed", "rework"]
    );
}

#[test]
fn test_notifications_config_absent_returns_none() {
    let yaml_str = r#"
tracker:
  kind: linear
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("config should parse without notifications");

    assert!(
        config.notifications.is_none(),
        "notifications should be None for backward compatibility"
    );
}

#[test]
fn test_notifications_config_invalid_webhook_returns_error() {
    let yaml_str = r#"
notifications:
  slack:
    webhook_url: ""
    events:
      - stalled
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("empty webhook URL should be invalid");

    assert!(
        matches!(err, SymphonyError::InvalidWorkflowConfig(ref msg) if msg.contains("notifications.slack.webhook_url")),
        "expected invalid webhook URL error, got: {err}"
    );
}

#[test]
#[serial]
fn test_notifications_config_resolves_env_var_in_webhook_url() {
    let previous = std::env::var("SYMPHONY_TEST_SLACK_WEBHOOK_URL").ok();
    std::env::set_var(
        "SYMPHONY_TEST_SLACK_WEBHOOK_URL",
        "https://hooks.slack.com/services/T111/B111/envtoken",
    );

    let yaml_str = r#"
notifications:
  slack:
    webhook_url: $SYMPHONY_TEST_SLACK_WEBHOOK_URL
    events:
      - stalled
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("notifications env var should resolve");

    let slack = config
        .notifications
        .as_ref()
        .and_then(|notifications| notifications.slack.as_ref())
        .expect("slack config should exist");

    assert_eq!(
        slack.webhook_url,
        "https://hooks.slack.com/services/T111/B111/envtoken"
    );

    match previous {
        Some(value) => std::env::set_var("SYMPHONY_TEST_SLACK_WEBHOOK_URL", value),
        None => std::env::remove_var("SYMPHONY_TEST_SLACK_WEBHOOK_URL"),
    }
}

#[test]
fn test_notifications_config_normalizes_event_names() {
    let yaml_str = r#"
notifications:
  slack:
    webhook_url: https://hooks.slack.com/services/T000/B000/abc123
    events:
      - Human_Review
      - STALLED
      - Failed
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("notifications config should parse");

    let slack = config
        .notifications
        .as_ref()
        .and_then(|notifications| notifications.slack.as_ref())
        .expect("slack config should exist");

    assert_eq!(
        slack.events,
        vec!["human_review", "stalled", "failed"],
        "event names should be normalized to lowercase"
    );
}

#[test]
fn test_notifications_config_unknown_event_returns_error() {
    let yaml_str = r#"
notifications:
  slack:
    webhook_url: https://hooks.slack.com/services/T000/B000/abc123
    events:
      - stalled
      - typo_event
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("unknown notifications event should fail parsing");

    assert!(
        matches!(err, SymphonyError::InvalidWorkflowConfig(ref msg) if msg.contains("unsupported value 'typo_event'")),
        "expected invalid notifications event error, got: {err}"
    );
}

#[test]
fn test_should_notify_filters_by_event_list() {
    let yaml_str = r#"
notifications:
  slack:
    webhook_url: https://hooks.slack.com/services/T000/B000/abc123
    events:
      - stalled
      - rework
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("notifications config should parse");

    let slack = config
        .notifications
        .as_ref()
        .and_then(|notifications| notifications.slack.as_ref())
        .expect("slack config should exist");

    assert!(should_notify(slack, "stalled"));
    assert!(should_notify(slack, "ReWork"));
    assert!(!should_notify(slack, "failed"));
}

#[test]
fn test_config_workspace_slug_parse() {
    let yaml_str = "tracker:\n  workspace_slug: acme";
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workspace slug should parse");
    assert_eq!(config.tracker.workspace_slug.as_deref(), Some("acme"));
}

#[test]
fn test_config_workspace_slug_whitespace_is_treated_as_missing() {
    let yaml_str = "tracker:\n  workspace_slug: \"   \"";
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workspace slug should parse");
    assert_eq!(config.tracker.workspace_slug, None);
}

#[test]
#[serial]
fn test_config_empty_literal_api_key_does_not_use_linear_api_key_fallback() {
    let previous_linear_api_key = std::env::var("LINEAR_API_KEY").ok();
    std::env::set_var("LINEAR_API_KEY", "fallback-linear-token");

    let yaml_str = "tracker:\n  api_key: \"\"";
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("empty literal api_key should parse");

    assert_eq!(
        config.tracker.api_key, None,
        "literal empty api_key should remain missing instead of using LINEAR_API_KEY fallback"
    );

    match previous_linear_api_key {
        Some(value) => std::env::set_var("LINEAR_API_KEY", value),
        None => std::env::remove_var("LINEAR_API_KEY"),
    }
}

#[test]
#[serial]
fn test_github_api_key_env_ref_does_not_fallback_to_linear_api_key() {
    let previous_linear_api_key = std::env::var("LINEAR_API_KEY").ok();
    std::env::set_var("LINEAR_API_KEY", "fallback-linear-token");

    std::env::remove_var("SYMPHONY_TEST_GITHUB_TOKEN_MISSING");

    let yaml_str = r#"
tracker:
  kind: github
  api_key: $SYMPHONY_TEST_GITHUB_TOKEN_MISSING
  repo_owner: kata-sh
  repo_name: kata
  github_project_owner_type: org
  github_project_number: 42
"#;

    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("github config should parse");

    assert_eq!(
        config.tracker.api_key, None,
        "github api_key should stay missing when explicit env reference is unset"
    );

    match previous_linear_api_key {
        Some(value) => std::env::set_var("LINEAR_API_KEY", value),
        None => std::env::remove_var("LINEAR_API_KEY"),
    }
}

#[test]
#[serial]
fn test_config_tilde_expansion() {
    let previous_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", "/tmp/symphony-test-home");

    let yaml_str = "workspace:\n  root: ~/workspaces";
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("tilde expansion should succeed");

    assert_eq!(
        config.workspace.root, "/tmp/symphony-test-home/workspaces",
        "workspace root should expand against HOME"
    );

    match previous_home {
        Some(home) => std::env::set_var("HOME", home),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn test_workspace_git_strategy_and_branch_prefix_parse() {
    let yaml_str = r#"
workspace:
  root: ~/workspaces
  repo: https://github.com/gannonh/kata.git
  git_strategy: clone-remote
  branch_prefix: " symphony "
  clone_branch: elixir-feature-parity
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workspace bootstrap config should parse");

    assert_eq!(
        config.workspace.repo.as_deref(),
        Some("https://github.com/gannonh/kata.git")
    );
    assert_eq!(
        config.workspace.strategy,
        WorkspaceRepoStrategy::CloneRemote
    );
    assert_eq!(config.workspace.branch_prefix, "symphony");
    assert_eq!(
        config.workspace.clone_branch.as_deref(),
        Some("elixir-feature-parity")
    );
    assert_eq!(config.workspace.base_branch.as_deref(), Some("main"));
}

#[test]
fn test_workspace_legacy_strategy_clone_maps_to_auto() {
    let yaml_str = r#"
workspace:
  strategy: clone
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("legacy workspace strategy should parse");
    assert_eq!(config.workspace.strategy, WorkspaceRepoStrategy::Auto);
}

#[test]
fn test_workspace_legacy_strategy_worktree_maps_to_worktree() {
    let yaml_str = r#"
workspace:
  strategy: worktree
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("legacy workspace strategy should parse");
    assert_eq!(config.workspace.strategy, WorkspaceRepoStrategy::Worktree);
}

#[test]
fn test_workspace_git_strategy_takes_precedence_over_legacy_strategy() {
    let yaml_str = r#"
workspace:
  strategy: worktree
  git_strategy: clone-local
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workspace strategy precedence should parse");
    assert_eq!(config.workspace.strategy, WorkspaceRepoStrategy::CloneLocal);
}

#[test]
fn test_workspace_clone_branch_blank_is_ignored() {
    let yaml_str = r#"
workspace:
  clone_branch: "   "
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workspace clone branch should parse");
    assert_eq!(config.workspace.clone_branch, None);
}

#[test]
fn test_workspace_base_branch_parses_and_trims() {
    let yaml_str = r#"
workspace:
  base_branch: " elixir-feature-parity "
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workspace base branch should parse");
    assert_eq!(
        config.workspace.base_branch.as_deref(),
        Some("elixir-feature-parity")
    );
}

#[test]
fn test_workspace_base_branch_blank_uses_default_main() {
    let yaml_str = r#"
workspace:
  base_branch: "   "
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workspace base branch blank should parse");
    assert_eq!(config.workspace.base_branch.as_deref(), Some("main"));
}

#[test]
fn test_workspace_isolation_defaults_to_local() {
    let yaml_str = r#"
workspace:
  repo: /tmp/local-repo
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("workspace defaults should parse");
    assert_eq!(config.workspace.isolation, WorkspaceIsolation::Local);
}

#[test]
fn test_workspace_isolation_docker_parses() {
    let yaml_str = r#"
workspace:
  isolation: docker
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("docker isolation should parse");
    assert_eq!(config.workspace.isolation, WorkspaceIsolation::Docker);
    assert!(config.workspace.docker.is_some());
}

#[test]
fn test_docker_isolation_parses_with_defaults() {
    let yaml_str = r#"
workspace:
  isolation: docker
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("docker defaults should parse");

    let docker = config.workspace.docker.expect("docker config should exist");
    assert_eq!(docker.image, "symphony-worker:latest");
    assert_eq!(docker.setup, None);
    assert_eq!(docker.codex_auth, DockerCodexAuth::Auto);
    assert!(docker.env.is_empty());
    assert!(docker.volumes.is_empty());
}

#[test]
fn test_docker_isolation_parses_full_config() {
    let yaml_str = r#"
workspace:
  isolation: docker
  docker:
    image: my-worker:dev
    setup: docker/setups/rust.sh
    codex_auth: mount
    env:
      - FOO=bar
      - BAR=baz
    volumes:
      - ~/.ssh:/home/node/.ssh:ro
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("full docker config should parse");

    let docker = config.workspace.docker.expect("docker config should exist");
    assert_eq!(docker.image, "my-worker:dev");
    assert_eq!(docker.setup.as_deref(), Some("docker/setups/rust.sh"));
    assert_eq!(docker.codex_auth, DockerCodexAuth::Mount);
    assert_eq!(
        docker.env,
        vec!["FOO=bar".to_string(), "BAR=baz".to_string()]
    );
    assert_eq!(docker.volumes.len(), 1);
    assert!(docker.volumes[0].contains(".ssh:/home/node/.ssh:ro"));
}

#[test]
fn test_docker_codex_auth_values() {
    for (value, expected) in [
        ("auto", DockerCodexAuth::Auto),
        ("mount", DockerCodexAuth::Mount),
        ("env", DockerCodexAuth::Env),
    ] {
        let yaml_str = format!(
            r#"
workspace:
  isolation: docker
  docker:
    codex_auth: {value}
"#
        );
        let raw: serde_yaml::Value = serde_yaml::from_str(&yaml_str).unwrap();
        let config = from_workflow(&raw).expect("docker codex auth should parse");
        assert_eq!(
            config
                .workspace
                .docker
                .expect("docker config should exist")
                .codex_auth,
            expected
        );
    }
}

#[test]
fn test_docker_config_absent_when_local_isolation() {
    let yaml_str = r#"
workspace:
  isolation: local
  docker:
    image: ignored
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let config = from_workflow(&raw).expect("local isolation with docker section should parse");
    assert_eq!(config.workspace.isolation, WorkspaceIsolation::Local);
    assert!(config.workspace.docker.is_none());
}

#[test]
fn test_docker_isolation_rejects_worker_ssh_hosts() {
    let yaml_str = r#"
workspace:
  isolation: docker
worker:
  ssh_hosts:
    - worker-a
    - worker-b
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("docker isolation should reject worker.ssh_hosts");
    assert!(
        err.to_string()
            .contains("worker.ssh_hosts is not supported with workspace.isolation 'docker'"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_docker_isolation_rejects_clone_local_strategy() {
    let yaml_str = r#"
workspace:
  isolation: docker
  repo: /tmp/local-repo
  git_strategy: clone-local
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("docker isolation should reject clone-local");
    assert!(
        err.to_string().contains(
            "workspace.git_strategy 'clone-local' is not supported with workspace.isolation 'docker'"
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn test_docker_isolation_rejects_worktree_strategy() {
    let yaml_str = r#"
workspace:
  isolation: docker
  repo: /tmp/local-repo
  strategy: worktree
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("docker isolation should reject worktree");
    assert!(
        err.to_string().contains(
            "workspace.git_strategy 'worktree' is not supported with workspace.isolation 'docker'"
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn test_docker_isolation_rejects_auto_with_local_repo() {
    let yaml_str = r#"
workspace:
  isolation: docker
  repo: /tmp/local-repo
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let err = from_workflow(&raw).expect_err("docker isolation should reject local auto strategy");
    assert!(
        err.to_string().contains(
            "workspace.git_strategy 'clone-local' is not supported with workspace.isolation 'docker'"
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn test_workspace_cleanup_on_done_parses_true_and_false() {
    let yaml_true = r#"
workspace:
  cleanup_on_done: true
"#;
    let raw_true: serde_yaml::Value = serde_yaml::from_str(yaml_true).unwrap();
    let config_true = from_workflow(&raw_true).expect("cleanup_on_done=true should parse");
    assert!(config_true.workspace.cleanup_on_done);

    let yaml_false = r#"
workspace:
  cleanup_on_done: false
"#;
    let raw_false: serde_yaml::Value = serde_yaml::from_str(yaml_false).unwrap();
    let config_false = from_workflow(&raw_false).expect("cleanup_on_done=false should parse");
    assert!(!config_false.workspace.cleanup_on_done);
}

#[test]
fn test_workspace_strategy_invalid_value_errors() {
    let yaml_str = r#"
workspace:
  repo: /tmp/local-repo
  strategy: invalid
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let result = from_workflow(&raw);

    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("workspace.strategy")),
        "invalid workspace.strategy should return InvalidWorkflowConfig, got: {:?}",
        result
    );
}

#[test]
fn test_workspace_git_strategy_invalid_value_errors() {
    let yaml_str = r#"
workspace:
  git_strategy: invalid
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let result = from_workflow(&raw);

    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("workspace.git_strategy")),
        "invalid workspace.git_strategy should return InvalidWorkflowConfig, got: {:?}",
        result
    );
}

#[test]
fn test_workspace_isolation_invalid_value_errors() {
    let yaml_str = r#"
workspace:
  isolation: invalid
"#;
    let raw: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let result = from_workflow(&raw);

    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("workspace.isolation")),
        "invalid workspace.isolation should return InvalidWorkflowConfig, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_rejects_worktree_without_repo() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("linear".to_string()),
            api_key: Some("test-key".into()),
            project_slug: Some("my-project".to_string()),
            ..TrackerConfig::default()
        },
        workspace: WorkspaceConfig {
            strategy: WorkspaceRepoStrategy::Worktree,
            repo: None,
            ..WorkspaceConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("workspace.repo")),
        "worktree strategy without repo should fail validation, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_rejects_worktree_with_remote_repo() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("linear".to_string()),
            api_key: Some("test-key".into()),
            project_slug: Some("my-project".to_string()),
            ..TrackerConfig::default()
        },
        workspace: WorkspaceConfig {
            strategy: WorkspaceRepoStrategy::Worktree,
            repo: Some("https://github.com/gannonh/kata.git".to_string()),
            ..WorkspaceConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("workspace.strategy") && msg.contains("local")),
        "worktree strategy with remote repo should fail validation, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_rejects_clone_local_with_remote_repo() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("linear".to_string()),
            api_key: Some("test-key".into()),
            project_slug: Some("my-project".to_string()),
            ..TrackerConfig::default()
        },
        workspace: WorkspaceConfig {
            strategy: WorkspaceRepoStrategy::CloneLocal,
            repo: Some("https://github.com/gannonh/kata.git".to_string()),
            ..WorkspaceConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("workspace.git_strategy") && msg.contains("clone-local")),
        "clone-local strategy with remote repo should fail validation, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_rejects_clone_local_without_repo() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("linear".to_string()),
            api_key: Some("test-key".into()),
            project_slug: Some("my-project".to_string()),
            ..TrackerConfig::default()
        },
        workspace: WorkspaceConfig {
            strategy: WorkspaceRepoStrategy::CloneLocal,
            repo: None,
            ..WorkspaceConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("workspace.repo") && msg.contains("clone-local")),
        "clone-local strategy without repo should fail validation, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_rejects_clone_local_with_scp_style_remote_repo() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("linear".to_string()),
            api_key: Some("test-key".into()),
            project_slug: Some("my-project".to_string()),
            ..TrackerConfig::default()
        },
        workspace: WorkspaceConfig {
            strategy: WorkspaceRepoStrategy::CloneLocal,
            repo: Some("github.example.com:org/repo.git".to_string()),
            ..WorkspaceConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("workspace.git_strategy") && msg.contains("clone-local")),
        "clone-local strategy with SCP-style remote repo should fail validation, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_missing_api_key() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("linear".to_string()),
            api_key: None,
            project_slug: Some("my-project".to_string()),
            ..TrackerConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::MissingLinearApiToken)),
        "missing api_key should return MissingLinearApiToken, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_missing_project_slug() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("linear".to_string()),
            api_key: Some("test-key".into()),
            project_slug: None,
            ..TrackerConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::MissingLinearProjectSlug)),
        "missing project_slug should return MissingLinearProjectSlug, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_bad_tracker_kind() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("jira".to_string()),
            api_key: Some("test-key".into()),
            ..TrackerConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::UnsupportedTrackerKind(ref k)) if k == "jira"),
        "unsupported tracker kind 'jira' should return UnsupportedTrackerKind, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_github_valid() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("github".to_string()),
            api_key: Some("test-key".into()),
            repo_owner: Some("kata-sh".to_string()),
            repo_name: Some("kata".to_string()),
            github_project_owner_type: Some(GithubProjectOwnerType::Org),
            github_project_number: Some(42),
            ..TrackerConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        result.is_ok(),
        "github tracker config should validate successfully, got: {:?}",
        result
    );
}

#[test]
#[serial]
fn test_config_validation_github_missing_token_errors() {
    let previous_gh_token = std::env::var("GH_TOKEN").ok();
    let previous_github_token = std::env::var("GITHUB_TOKEN").ok();
    let previous_gh_cli_fallback = std::env::var("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK").ok();
    std::env::set_var("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK", "0");
    std::env::remove_var("GH_TOKEN");
    std::env::remove_var("GITHUB_TOKEN");

    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("github".to_string()),
            api_key: None,
            repo_owner: Some("kata-sh".to_string()),
            repo_name: Some("kata".to_string()),
            github_project_owner_type: Some(GithubProjectOwnerType::Org),
            github_project_number: Some(42),
            ..TrackerConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    let validation_failed_for_missing_token = matches!(
        result,
        Err(SymphonyError::InvalidWorkflowConfig(ref msg))
            if msg.contains("GitHub token required when tracker.kind is github")
    );

    match previous_gh_token {
        Some(value) => std::env::set_var("GH_TOKEN", value),
        None => std::env::remove_var("GH_TOKEN"),
    }
    match previous_github_token {
        Some(value) => std::env::set_var("GITHUB_TOKEN", value),
        None => std::env::remove_var("GITHUB_TOKEN"),
    }
    match previous_gh_cli_fallback {
        Some(value) => std::env::set_var("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK", value),
        None => std::env::remove_var("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK"),
    }

    assert!(
        validation_failed_for_missing_token,
        "missing github token should return descriptive validation error, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_github_missing_repo_owner_errors() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("github".to_string()),
            api_key: Some("test-key".into()),
            repo_owner: None,
            repo_name: Some("kata".to_string()),
            github_project_owner_type: Some(GithubProjectOwnerType::Org),
            github_project_number: Some(42),
            ..TrackerConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg == "tracker.repo_owner is required when tracker.kind is github"),
        "missing github repo_owner should fail validation, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_github_missing_repo_name_errors() {
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("github".to_string()),
            api_key: Some("test-key".into()),
            repo_owner: Some("kata-sh".to_string()),
            repo_name: None,
            github_project_owner_type: Some(GithubProjectOwnerType::Org),
            github_project_number: Some(42),
            ..TrackerConfig::default()
        },
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg == "tracker.repo_name is required when tracker.kind is github"),
        "missing github repo_name should fail validation, got: {:?}",
        result
    );
}

#[test]
fn test_config_validation_missing_codex_command() {
    // Build a fully valid base config so validation reaches the agent.command
    // check.  Previously this test used ServiceConfig::default() which has
    // tracker.kind=None, causing validation to fail on kind before ever
    // reaching the agent.command check.
    let config = ServiceConfig {
        tracker: TrackerConfig {
            kind: Some("linear".to_string()),
            api_key: Some("test-key".into()),
            project_slug: Some("my-project".to_string()),
            ..TrackerConfig::default()
        },
        codex: CodexConfig {
            command: vec![],
            ..CodexConfig::default()
        },
        agent_backend: AgentBackend::Codex,
        ..ServiceConfig::default()
    };

    let result = validate(&config);
    assert!(
        matches!(result, Err(SymphonyError::InvalidWorkflowConfig(ref msg)) if msg.contains("agent.command")),
        "empty command should return agent-specific InvalidWorkflowConfig, got: {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Liquid group
// ─────────────────────────────────────────────────────────────────────────────

/// Symphony requires strict variable behavior: unknown variables must produce
/// an error rather than render as empty string.
#[test]
fn test_liquid_unknown_variable_error() {
    let parser = liquid::ParserBuilder::with_stdlib()
        .build()
        .expect("liquid parser should build");

    let template = parser
        .parse("{{ unknown_var }}")
        .expect("template string should parse");

    let globals = liquid::object!({});
    let result = template.render(&globals);

    // liquid 0.26 uses strict unknown-variable behavior by default; this test
    // locks in the spec §5.4 (R007) strict-variable rendering contract explicitly.
    assert!(
        result.is_err(),
        "strict mode should error on unknown variables; got Ok: {:?}",
        result.ok()
    );
}

#[test]
fn test_liquid_known_variables_render() {
    let parser = liquid::ParserBuilder::with_stdlib()
        .build()
        .expect("liquid parser should build");

    let template = parser
        .parse("{{ issue_id }}")
        .expect("template string should parse");

    let globals = liquid::object!({ "issue_id": "LIN-1" });
    let output = template.render(&globals).expect("render should succeed");

    assert_eq!(output, "LIN-1");
}

// ─────────────────────────────────────────────────────────────────────────────
// WorkflowStore group
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_workflow_store_initial_load() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "---\ntracker:\n  kind: linear\n  api_key: test-key\n  project_slug: my-proj\n---\nHello"
    )
    .unwrap();

    let store = WorkflowStore::new(file.path()).expect("store should initialize from valid file");
    let (def, config) = store.effective_config();

    assert_eq!(config.tracker.kind.as_deref(), Some("linear"));
    assert_eq!(config.tracker.api_key.as_deref(), Some("test-key"));
    assert!(
        def.prompt_template.contains("Hello"),
        "prompt should be loaded, got: {:?}",
        def.prompt_template
    );
}

#[tokio::test]
async fn test_workflow_store_hot_reload() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "---\ntracker:\n  kind: linear\n  api_key: key1\n---\nOriginal"
    )
    .unwrap();

    let store = WorkflowStore::new(file.path()).expect("store should initialize");
    let (_, config_before) = store.effective_config();
    assert_eq!(
        config_before.tracker.api_key.as_deref(),
        Some("key1"),
        "initial api_key should be key1"
    );

    // Overwrite with updated content
    {
        let mut f = std::fs::File::create(file.path()).unwrap();
        writeln!(
            f,
            "---\ntracker:\n  kind: linear\n  api_key: key2\n---\nUpdated"
        )
        .unwrap();
    }

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    let mut observed = None;
    while tokio::time::Instant::now() < deadline {
        let (_, config_after) = store.effective_config();
        observed = config_after.tracker.api_key.clone();
        if observed.as_deref() == Some("key2") {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(75)).await;
    }

    assert_eq!(
        observed.as_deref(),
        Some("key2"),
        "hot-reload should update api_key to key2 within timeout"
    );
}

#[tokio::test]
async fn test_workflow_store_reload_failure_keeps_last_good() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "---\ntracker:\n  kind: linear\n  api_key: good-key\n---\nGood"
    )
    .unwrap();

    let store = WorkflowStore::new(file.path()).expect("store should initialize");
    let (_, config_before) = store.effective_config();
    assert_eq!(
        config_before.tracker.api_key.as_deref(),
        Some("good-key"),
        "initial api_key should be good-key"
    );

    // Overwrite with deliberately broken YAML
    {
        let mut f = std::fs::File::create(file.path()).unwrap();
        // This is syntactically invalid YAML (bare colon sequence)
        writeln!(f, "---\n: : : broken yaml : : :\n---\nBroken").unwrap();
    }

    // Keep checking for a bounded window to ensure filesystem notifications and
    // debounce processing have a chance to run under slower CI.
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let (_, config_after) = store.effective_config();
        assert_eq!(
            config_after.tracker.api_key.as_deref(),
            Some("good-key"),
            "failed reload should preserve last-known-good config; api_key must still be good-key"
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(75)).await;
    }
}

#[tokio::test]
async fn test_workflow_store_force_reload_reports_error_on_invalid_workflow() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        "---\ntracker:\n  kind: linear\n  api_key: good-key\n---\nGood"
    )
    .unwrap();

    let store = WorkflowStore::new(file.path()).expect("store should initialize");

    // Overwrite with deliberately broken YAML then force an immediate reload.
    {
        let mut f = std::fs::File::create(file.path()).unwrap();
        writeln!(f, "---\n: : : broken yaml : : :\n---\nBroken").unwrap();
    }

    let reload_result = store.force_reload().await;
    assert!(
        reload_result.is_err(),
        "force_reload should return Err for invalid workflow reloads"
    );

    // Last-known-good semantics must still hold even when force_reload reports failure.
    let (_, config_after) = store.effective_config();
    assert_eq!(
        config_after.tracker.api_key.as_deref(),
        Some("good-key"),
        "failed force_reload should preserve last-known-good config"
    );
}

// ── Per-state prompts config ──────────────────────────────────────────────────

#[test]
fn test_prompts_config_absent_returns_none() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
"#;
    let config = parse_yaml_config(yaml);
    assert!(config.prompts.is_none());
}

#[test]
fn test_prompts_config_parses_all_fields() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
prompts:
  shared: prompts/shared.md
  default: prompts/in-progress.md
  by_state:
    In Progress: prompts/in-progress.md
    Agent Review: prompts/agent-review.md
    Merging: prompts/merging.md
"#;
    let config = parse_yaml_config(yaml);
    let prompts = config.prompts.expect("prompts should be Some");
    assert_eq!(prompts.shared.as_deref(), Some("prompts/shared.md"));
    assert_eq!(prompts.default.as_deref(), Some("prompts/in-progress.md"));
    assert_eq!(
        prompts.by_state.get("in progress").map(String::as_str),
        Some("prompts/in-progress.md")
    );
    assert_eq!(
        prompts.by_state.get("agent review").map(String::as_str),
        Some("prompts/agent-review.md")
    );
    assert_eq!(
        prompts.by_state.get("merging").map(String::as_str),
        Some("prompts/merging.md")
    );
}

#[test]
fn test_prompts_config_parses_system_and_repo_fields() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
prompts:
  system: prompts/system.md
  repo: prompts/repo.md
  default: prompts/in-progress.md
  by_state:
    In Progress: prompts/in-progress.md
"#;
    let config = parse_yaml_config(yaml);
    let prompts = config.prompts.expect("prompts should be Some");
    assert_eq!(prompts.system.as_deref(), Some("prompts/system.md"));
    assert_eq!(prompts.repo.as_deref(), Some("prompts/repo.md"));
    assert!(
        prompts.shared.is_none(),
        "shared should be None when not configured"
    );
    assert_eq!(prompts.default.as_deref(), Some("prompts/in-progress.md"));
}

#[test]
fn test_prompts_config_system_alone_triggers_some() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
prompts:
  system: prompts/system.md
"#;
    let config = parse_yaml_config(yaml);
    assert!(
        config.prompts.is_some(),
        "system-only prompts should produce Some"
    );
    let prompts = config.prompts.unwrap();
    assert_eq!(prompts.system.as_deref(), Some("prompts/system.md"));
    assert!(prompts.repo.is_none());
    assert!(prompts.shared.is_none());
    assert!(prompts.by_state.is_empty());
    assert!(prompts.default.is_none());
}

#[test]
fn test_prompts_config_normalizes_state_keys_to_lowercase() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
prompts:
  by_state:
    "In Progress": prompts/ip.md
    "AGENT REVIEW": prompts/ar.md
"#;
    let config = parse_yaml_config(yaml);
    let prompts = config.prompts.expect("prompts should be Some");
    assert!(prompts.by_state.contains_key("in progress"));
    assert!(prompts.by_state.contains_key("agent review"));
    assert!(!prompts.by_state.contains_key("In Progress"));
    assert!(!prompts.by_state.contains_key("AGENT REVIEW"));
}

// ── Issue children_count and parent_identifier ────────────────────────────────

#[test]
fn test_issue_children_count_and_parent_parsed_from_linear() {
    // This tests the normalization in linear/client.rs
    // The fields should default to 0/None when not present in JSON
    let issue = symphony::domain::Issue {
        id: "test-id".to_string(),
        identifier: "KAT-100".to_string(),
        title: "Test".to_string(),
        description: None,
        priority: None,
        state: "In Progress".to_string(),
        branch_name: None,
        url: None,
        assignee_id: None,
        labels: vec![],
        blocked_by: vec![],
        assigned_to_worker: true,
        created_at: None,
        updated_at: None,
        children_count: 3,
        parent_identifier: Some("KAT-99".to_string()),
    };
    assert_eq!(issue.children_count, 3);
    assert_eq!(issue.parent_identifier.as_deref(), Some("KAT-99"));
}

#[test]
fn test_issue_children_count_defaults_to_zero() {
    let issue = symphony::domain::Issue {
        id: "test-id".to_string(),
        identifier: "KAT-100".to_string(),
        title: "Test".to_string(),
        description: None,
        priority: None,
        state: "In Progress".to_string(),
        branch_name: None,
        url: None,
        assignee_id: None,
        labels: vec![],
        blocked_by: vec![],
        assigned_to_worker: true,
        created_at: None,
        updated_at: None,
        children_count: 0,
        parent_identifier: None,
    };
    assert_eq!(issue.children_count, 0);
    assert!(issue.parent_identifier.is_none());
}

fn parse_yaml_config(yaml: &str) -> symphony::domain::ServiceConfig {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).expect("valid yaml");
    from_workflow(&value).expect("config should parse")
}

#[test]
fn test_prompts_config_filters_empty_and_whitespace_paths() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
prompts:
  shared: "   "
  default: ""
  by_state:
    In Progress: "  prompts/ip.md  "
    Agent Review: ""
"#;
    let config = parse_yaml_config(yaml);
    let prompts = config
        .prompts
        .expect("prompts should be Some (ip.md is non-empty)");
    assert!(
        prompts.shared.is_none(),
        "whitespace-only shared should be None"
    );
    assert!(prompts.default.is_none(), "empty default should be None");
    assert_eq!(
        prompts.by_state.len(),
        1,
        "empty Agent Review path should be filtered"
    );
    assert_eq!(
        prompts.by_state.get("in progress").map(String::as_str),
        Some("prompts/ip.md"),
        "in progress path should be trimmed"
    );
}

#[test]
fn test_prompts_config_all_empty_returns_none() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
prompts:
  shared: ""
  default: "  "
  by_state:
    In Progress: ""
"#;
    let config = parse_yaml_config(yaml);
    assert!(config.prompts.is_none(), "all-empty prompts should be None");
}

#[test]
fn test_exclude_labels_parses_from_yaml() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
  exclude_labels:
    - kata:task
    - wontfix
"#;
    let config = parse_yaml_config(yaml);
    assert_eq!(
        config.tracker.exclude_labels,
        vec!["kata:task".to_string(), "wontfix".to_string()]
    );
}

#[test]
fn test_exclude_labels_defaults_to_empty() {
    let yaml = r#"
tracker:
  kind: linear
  api_key: test-key
  project_slug: test-slug
"#;
    let config = parse_yaml_config(yaml);
    assert!(
        config.tracker.exclude_labels.is_empty(),
        "exclude_labels should default to empty vec"
    );
}
