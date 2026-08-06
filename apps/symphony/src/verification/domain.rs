//! Durable A5 verification-stage domain values and configuration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const VERIFICATION_STAGE_NAME: &str = "verification";
pub const VERIFICATION_SCHEMA_VERSION: u32 = 1;
pub const VERIFICATION_DEFAULT_PROMPT: &str = "prompts/verification.md";
pub const VERIFICATION_MAX_COMMANDS: usize = 20;
pub const VERIFICATION_NAME_MAX_BYTES: usize = 100;
pub const VERIFICATION_COMMAND_MAX_BYTES: usize = 4_000;
pub const VERIFICATION_OUTPUT_TAIL_MAX_BYTES: usize = 128 * 1024;
pub const VERIFICATION_EVIDENCE_FILES_DEFAULT: usize = 100;
pub const VERIFICATION_EVIDENCE_BYTES_DEFAULT: u64 = 100 * 1024 * 1024;
pub const VERIFICATION_EVIDENCE_PATH_MAX_BYTES: usize = 500;
pub const VERIFICATION_COMMENT_MARKER_PREFIX: &str = "<!-- symphony:verification:";
pub const VERIFICATION_COMMENT_MARKER_SUFFIX: &str = " -->";
pub const VERIFICATION_CRITERIA_MAX_ITEMS: usize = 200;
pub const VERIFICATION_RATIONALE_MAX_BYTES: usize = 2_000;

/// A blocking verification command declared by trusted workflow configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerificationCommandConfig {
    pub name: String,
    pub kind: VerificationCommandKind,
    pub command: String,
    pub timeout_ms: u64,
}

/// Command kind. Exactly one command may carry `kind: acceptance`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationCommandKind {
    Test,
    Acceptance,
}

impl VerificationCommandKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Acceptance => "acceptance",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationConfig {
    pub enabled: bool,
    pub mode: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub max_turns: u32,
    pub invocation_timeout_ms: u64,
    pub max_attempts: u32,
    pub max_reprompts: u32,
    pub max_evidence_files: usize,
    pub max_evidence_bytes: u64,
    pub trigger_state: String,
    #[serde(default)]
    pub commands: Vec<VerificationCommandConfig>,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "preview".to_string(),
            prompt: VERIFICATION_DEFAULT_PROMPT.to_string(),
            model: None,
            max_turns: 1,
            invocation_timeout_ms: 1_800_000,
            max_attempts: 3,
            max_reprompts: 2,
            max_evidence_files: VERIFICATION_EVIDENCE_FILES_DEFAULT,
            max_evidence_bytes: VERIFICATION_EVIDENCE_BYTES_DEFAULT,
            trigger_state: "Verification".to_string(),
            commands: Vec::new(),
        }
    }
}

/// Durable inputs of one verification attempt, persisted before execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationAttemptRecord {
    pub attempt_id: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub pr_number: u64,
    pub reviewed_head_sha: String,
    pub base_sha: String,
    pub spec_artifact_id: String,
    pub implementation_artifact_id: String,
    pub review_artifact_id: String,
    pub configuration_revision: String,
    pub execution_profile: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::triage::domain::FactoryError>,
    /// Durable identity of the read-only verifier process (post-spawn record,
    /// same contract as the A3/A4 workers) so restart recovery can terminate
    /// it before touching the workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_pid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_process_group_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_start_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_executable: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Durable record of one configured command invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationCommandRunRecord {
    pub command_run_id: String,
    pub run_id: String,
    pub attempt_id: String,
    pub ordinal: u32,
    pub name: String,
    pub kind: VerificationCommandKind,
    pub configuration_revision: String,
    pub command_sha256: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_group_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_start_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub termination_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_sha256: Option<String>,
    pub execution_profile: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One collected evidence file below the attempt-owned evidence directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationEvidenceRecord {
    pub evidence_id: String,
    pub run_id: String,
    pub attempt_id: String,
    pub relative_path: String,
    pub sha256: String,
    pub bytes_len: u64,
    pub collected_at: DateTime<Utc>,
}

/// A verifier-asserted criterion. Only pass/fail/not_proven are accepted, and
/// pass/fail require at least one evidence reference inside the attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerifierCriterion {
    pub index: u32,
    pub status: VerifierCriterionStatus,
    pub rationale: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifierCriterionStatus {
    Pass,
    Fail,
    NotProven,
}

impl VerifierCriterionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::NotProven => "not_proven",
        }
    }
}

/// Strict verifier manifest. Rejected when it references evidence outside the
/// attempt, duplicates or drops criteria, or uses unsupported statuses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerifierManifest {
    pub schema_version: u32,
    pub spec_artifact_id: String,
    pub implementation_artifact_id: String,
    pub review_artifact_id: String,
    pub reviewed_head_sha: String,
    pub base_sha: String,
    pub summary: String,
    pub criteria: Vec<VerifierCriterion>,
}

/// Computed gate for one attempt. The verifier cannot override a failed
/// command, an unexecuted required command, a missing criterion, or an
/// invalid evidence reference — Symphony derives the verdict from durable
/// command outcomes plus complete criterion coverage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationGateRecord {
    pub gate_id: String,
    pub run_id: String,
    pub attempt_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_manifest: Option<VerifierManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_summary: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Durable intent for the single owned preview comment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationPublicationIntent {
    pub intent_id: String,
    pub run_id: String,
    pub attempt_id: String,
    pub kind: String,
    pub status: crate::triage::domain::PublicationStatus,
    pub completed_steps: Vec<String>,
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<crate::triage::domain::FactoryError>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A5 metric aggregates. Base fields reuse the triage shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VerificationMetricsAggregate {
    pub total_attempts: u64,
    pub completed_attempts: u64,
    pub failed_attempts: u64,
    pub interrupted_attempts: u64,
    pub superseded_attempts: u64,
    pub blocked_attempts: u64,
    pub commands_run: u64,
    pub commands_passed: u64,
    pub commands_failed: u64,
    pub gates_passed: u64,
    pub gates_failed: u64,
    pub evidence_files: u64,
    pub evidence_bytes: u64,
    pub preview_publications: u64,
    /// Average verification attempt duration in milliseconds.
    pub attempt_duration_avg_ms: u64,
    /// Highest number of attempts recorded for one reviewed head (same-head
    /// attempt count).
    pub max_same_head_attempts: u64,
    /// Token usage reported by verification stage runs.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// Harness/model usage reported by verification stage runs.
    pub model_usage: Vec<serde_json::Value>,
    pub base: crate::triage::domain::TriageMetricsAggregate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_defaults_match_a5_contract() {
        let config = VerificationConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.mode, "preview");
        assert_eq!(config.prompt, VERIFICATION_DEFAULT_PROMPT);
        assert_eq!(config.invocation_timeout_ms, 1_800_000);
        assert_eq!(config.max_attempts, 3);
        assert_eq!(
            config.max_evidence_files,
            VERIFICATION_EVIDENCE_FILES_DEFAULT
        );
        assert_eq!(
            config.max_evidence_bytes,
            VERIFICATION_EVIDENCE_BYTES_DEFAULT
        );
        assert_eq!(config.trigger_state, "Verification");
        assert!(config.commands.is_empty());
    }

    #[test]
    fn strict_manifest_rejects_unknown_fields() {
        let manifest = serde_json::json!({
            "schema_version": 1,
            "spec_artifact_id": "s",
            "implementation_artifact_id": "i",
            "review_artifact_id": "r",
            "reviewed_head_sha": "h",
            "base_sha": "b",
            "summary": "ok",
            "criteria": [],
            "surprise": true,
        });
        let parsed: Result<VerifierManifest, _> = serde_json::from_value(manifest);
        assert!(parsed.is_err());
    }
}
