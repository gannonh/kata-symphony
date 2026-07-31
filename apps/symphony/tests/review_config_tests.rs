//! Focused validation coverage for the A4 review configuration contract.

use symphony::config::{from_workflow, validate};
use symphony::error::SymphonyError;

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
review:
  enabled: true
"#;

fn raw_review_workflow() -> serde_yaml::Value {
    serde_yaml::from_str(REVIEW_WORKFLOW).expect("review workflow fixture should parse")
}

fn review_error(raw: serde_yaml::Value) -> String {
    let config = from_workflow(&raw).expect("review fixture should extract into typed config");
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
fn review_requires_github_tracker() {
    let mut raw = raw_review_workflow();
    raw["tracker"]["kind"] = serde_yaml::Value::String("linear".to_string());
    raw["tracker"]["project_slug"] = serde_yaml::Value::String("kata".to_string());
    let message = review_error(raw);
    assert!(message.contains("review.enabled requires tracker.kind to be 'github'"));
}

#[test]
fn review_requires_spec_and_implementation_stages() {
    let mut raw = raw_review_workflow();
    raw["spec"]["enabled"] = serde_yaml::Value::Bool(false);
    let message = review_error(raw);
    assert!(message.contains("review.enabled requires spec.enabled and implementation.enabled"));
}

#[test]
fn review_prompt_and_trigger_state_must_be_non_empty() {
    let mut prompt = raw_review_workflow();
    prompt["review"]["prompt"] = serde_yaml::Value::String("   ".to_string());
    assert!(review_error(prompt).contains("review.prompt must be non-empty"));

    let mut trigger = raw_review_workflow();
    trigger["review"]["trigger_state"] = serde_yaml::Value::String("  ".to_string());
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
        let mut raw = raw_review_workflow();
        raw["review"][field] = serde_yaml::Value::Number(0.into());
        let message = review_error(raw);
        assert!(message.contains(&format!("review.{field} must be greater than 0")));
    }
}

#[test]
fn review_codex_model_override_is_rejected() {
    let mut raw = raw_review_workflow();
    raw["agent"]["name"] = serde_yaml::Value::String("codex".to_string());
    raw["review"]["model"] = serde_yaml::Value::String("forbidden".to_string());
    let message = review_error(raw);
    assert!(message.contains("review.model is not supported when agent.name is 'codex'"));
}

#[test]
fn automatic_review_requires_completion_route() {
    let mut raw = raw_review_workflow();
    raw["review"]["mode"] = serde_yaml::Value::String("automatic".to_string());
    let message = review_error(raw);
    assert!(message.contains("review.completion_route.state is required"));
}
