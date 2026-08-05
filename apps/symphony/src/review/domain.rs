//! Durable A4 review-stage domain values and configuration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::review::manifest::ReviewSeverity;
use crate::triage::domain::{FactoryError, StageUsage};

pub const REVIEW_STAGE_NAME: &str = "review";
pub const REVIEW_SCHEMA_VERSION: u32 = 1;
pub const REVIEW_COMMENT_MARKER_PREFIX: &str = "<!-- symphony:review:";
pub const REVIEW_COMMENT_MARKER_SUFFIX: &str = " -->";
pub const REVIEW_DEFAULT_PROMPT: &str = "prompts/agent-review.md";
pub const REVIEW_MAX_FINDINGS_DEFAULT: usize = 50;
pub const REVIEW_MAX_REPROMPTS_DEFAULT: u32 = 2;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMode {
    #[default]
    Preview,
    Automatic,
}

impl ReviewMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Automatic => "automatic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRoute {
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewConfig {
    pub enabled: bool,
    pub mode: ReviewMode,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub max_turns: u32,
    pub invocation_timeout_ms: u64,
    pub max_attempts: u32,
    pub max_reprompts: u32,
    pub max_findings: usize,
    #[serde(default)]
    pub blocking_severity: ReviewSeverity,
    pub trigger_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_route: Option<ReviewRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes_requested_route: Option<ReviewRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_probe_pull_request: Option<u64>,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ReviewMode::Preview,
            prompt: REVIEW_DEFAULT_PROMPT.to_string(),
            model: None,
            max_turns: 1,
            invocation_timeout_ms: 1_800_000,
            max_attempts: 3,
            max_reprompts: REVIEW_MAX_REPROMPTS_DEFAULT,
            max_findings: REVIEW_MAX_FINDINGS_DEFAULT,
            blocking_severity: ReviewSeverity::Blocking,
            trigger_state: "Agent Review".to_string(),
            completion_route: None,
            changes_requested_route: None,
            permission_probe_pull_request: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewAttemptRecord {
    pub attempt_id: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub draft_pr_artifact_id: String,
    pub implementation_artifact_id: String,
    pub spec_artifact_id: String,
    pub pr_number: u64,
    pub reviewed_head_sha: String,
    pub base_sha: String,
    pub status: String,
    pub reprompt_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_turn: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<FactoryError>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewFindingsArtifactRecord {
    pub artifact_id: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub attempt_id: String,
    pub draft_pr_artifact_id: String,
    pub implementation_artifact_id: String,
    pub spec_artifact_id: String,
    pub schema_version: u32,
    pub reviewed_head_sha: String,
    pub base_sha: String,
    pub manifest: crate::review::manifest::ReviewFindingsManifest,
    pub no_findings: bool,
    pub finding_count: u32,
    pub received_at: DateTime<Utc>,
    pub bytes_len: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewPublicationIntent {
    pub intent_id: String,
    pub run_id: String,
    pub artifact_id: String,
    pub kind: String,
    pub status: crate::triage::domain::PublicationStatus,
    pub completed_steps: Vec<String>,
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<FactoryError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_login: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_state: Option<String>,
    pub desired_effects: serde_json::Value,
    pub observed_baseline: serde_json::Value,
    pub expected_projection: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewFindingRecord {
    pub finding_record_id: String,
    pub run_id: String,
    pub artifact_id: String,
    pub finding_id: String,
    pub identity_key: String,
    pub reviewed_head_sha: String,
    pub base_sha: String,
    pub severity: crate::review::manifest::ReviewSeverity,
    pub category: crate::review::manifest::ReviewFindingCategory,
    pub path: String,
    pub line: u32,
    pub end_line: Option<u32>,
    pub claim: String,
    pub rationale: String,
    pub remediation: String,
    pub acceptance_criterion: Option<String>,
    pub confidence: f64,
    pub lifecycle_state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ReviewMetricsAggregate {
    pub total_attempts: u64,
    pub completed_attempts: u64,
    pub failed_attempts: u64,
    pub blocked_publications: u64,
    pub preview_publications: u64,
    pub automatic_publications: u64,
    pub findings: u64,
    pub no_findings: u64,
    pub base: crate::triage::domain::TriageMetricsAggregate,
}

impl ReviewMetricsAggregate {
    pub fn usage(&self) -> StageUsage {
        StageUsage::default()
    }
}
