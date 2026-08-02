//! Focused validation coverage for the A4 review configuration contract.

use symphony::config::{from_workflow, validate};
use symphony::domain::AgentBackend;
use symphony::error::SymphonyError;
use symphony::review::manifest::ReviewSeverity;

const REVIEW_WORKFLOW: &str = r#"
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
implementation:
  enabled: true
  validation:
    - name: tests
      command: cargo test
review:
  enabled: true
"#;

fn raw_review_workflow() -> serde_yaml::Value {
    serde_yaml::from_str(REVIEW_WORKFLOW).expect("review workflow fixture should parse")
}

fn review_config() -> symphony::domain::ServiceConfig {
    let raw = raw_review_workflow();
    from_workflow(&raw).expect("review fixture should extract into typed config")
}

fn review_error(config: symphony::domain::ServiceConfig) -> String {
    match validate(&config) {
        Err(SymphonyError::InvalidWorkflowConfig(message)) => message,
        Err(other) => panic!("expected workflow validation error, got {other}"),
        Ok(_) => panic!("expected review workflow validation to fail"),
    }
}

#[test]
fn review_defaults_validate_and_explicit_routes_parse() {
    let mut raw = raw_review_workflow();
    raw["review"]["mode"] = serde_yaml::Value::String("automatic".to_string());
    raw["review"]["prompt"] = serde_yaml::Value::String("prompts/custom-review.md".to_string());
    raw["review"]["model"] = serde_yaml::Value::String("review-model".to_string());
    raw["review"]["max_turns"] = serde_yaml::Value::Number(2.into());
    raw["review"]["invocation_timeout_ms"] = serde_yaml::Value::Number(120_000.into());
    raw["review"]["max_attempts"] = serde_yaml::Value::Number(4.into());
    raw["review"]["max_reprompts"] = serde_yaml::Value::Number(3.into());
    raw["review"]["max_findings"] = serde_yaml::Value::Number(25.into());
    raw["review"]["trigger_state"] = serde_yaml::Value::String("Agent Review".to_string());
    raw["review"]["completion_route"]["state"] =
        serde_yaml::Value::String("Human Review".to_string());
    raw["review"]["changes_requested_route"]["state"] =
        serde_yaml::Value::String("Rework".to_string());

    let config = from_workflow(&raw).expect("explicit review settings should parse");
    assert!(config.review.enabled);
    assert_eq!(config.review.max_attempts, 4);
    assert_eq!(
        config
            .review
            .completion_route
            .as_ref()
            .map(|route| route.state.as_str()),
        Some("Human Review")
    );
    validate(&config).expect("complete automatic review configuration should validate");
}

#[test]
fn review_blocking_severity_parses_all_supported_values() {
    for (value, expected) in [
        ("blocking", ReviewSeverity::Blocking),
        ("major", ReviewSeverity::Major),
        ("minor", ReviewSeverity::Minor),
        ("nit", ReviewSeverity::Nit),
    ] {
        let mut raw = raw_review_workflow();
        raw["review"]["blocking_severity"] = serde_yaml::Value::String(value.to_string());
        let config = from_workflow(&raw).expect("blocking severity should parse");
        assert_eq!(config.review.blocking_severity, expected);
    }

    let config = review_config();
    assert_eq!(config.review.blocking_severity, ReviewSeverity::Blocking);
}

#[test]
fn review_blocking_severity_rejects_invalid_values() {
    let mut raw = raw_review_workflow();
    raw["review"]["blocking_severity"] = serde_yaml::Value::String("critical".to_string());
    let error = from_workflow(&raw).expect_err("invalid blocking severity should fail");
    match error {
        SymphonyError::InvalidWorkflowConfig(message) => {
            assert!(message.contains(
                "review.blocking_severity must be 'blocking', 'major', 'minor', or 'nit'"
            ));
            assert!(message.contains("got 'critical'"));
        }
        other => panic!("expected workflow config error, got {other}"),
    }
}

#[test]
fn review_config_deserialization_defaults_blocking_severity() {
    let config = review_config();
    let mut serialized =
        serde_json::to_value(&config.review).expect("review config should serialize");
    serialized
        .as_object_mut()
        .expect("serialized config should be an object")
        .remove("blocking_severity");
    let restored: symphony::review::domain::ReviewConfig =
        serde_json::from_value(serialized).expect("old review config should deserialize");
    assert_eq!(restored.blocking_severity, ReviewSeverity::Blocking);
}

#[test]
fn review_requires_github_tracker() {
    let mut config = review_config();
    config.tracker.kind = Some("linear".to_string());
    config.tracker.project_slug = Some("kata".to_string());
    config.spec.enabled = false;
    config.implementation.enabled = false;
    let message = review_error(config);
    assert!(message.contains("review.enabled requires tracker.kind to be 'github'"));
}

#[test]
fn review_requires_spec_and_implementation_stages() {
    let mut config = review_config();
    config.spec.enabled = false;
    config.implementation.enabled = false;
    let message = review_error(config);
    assert!(message.contains("review.enabled requires spec.enabled and implementation.enabled"));
}

#[test]
fn review_prompt_and_trigger_state_must_be_non_empty() {
    let mut prompt = review_config();
    prompt.review.prompt = "   ".to_string();
    assert!(review_error(prompt).contains("review.prompt must be non-empty"));

    let mut trigger = review_config();
    trigger.review.trigger_state = "  ".to_string();
    assert!(review_error(trigger).contains("review.trigger_state must be non-empty"));
}

#[test]
fn review_numeric_limits_must_be_positive() {
    for field in [
        "max_turns",
        "invocation_timeout_ms",
        "max_attempts",
        "max_findings",
    ] {
        let mut config = review_config();
        match field {
            "max_turns" => config.review.max_turns = 0,
            "invocation_timeout_ms" => config.review.invocation_timeout_ms = 0,
            "max_attempts" => config.review.max_attempts = 0,
            "max_findings" => config.review.max_findings = 0,
            _ => unreachable!(),
        }
        let message = review_error(config);
        assert!(message.contains(&format!("review.{field} must be greater than 0")));
    }
}

#[test]
fn review_codex_model_override_is_rejected() {
    let mut config = review_config();
    config.agent_backend = AgentBackend::Codex;
    config.review.model = Some("forbidden".to_string());
    let message = review_error(config);
    assert!(message.contains("review.model is not supported when agent.name is 'codex'"));
}

#[test]
fn automatic_review_requires_completion_route() {
    let mut raw = raw_review_workflow();
    raw["review"]["mode"] = serde_yaml::Value::String("automatic".to_string());
    let config = from_workflow(&raw).expect("automatic review fixture should parse");
    let message = review_error(config);
    assert!(message.contains("review.completion_route.state is required"));
}
