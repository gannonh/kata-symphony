use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::triage::domain::{FactoryError, StageUsage};

pub const IMPLEMENTATION_STAGE_NAME: &str = "implementation";
pub const IMPLEMENTATION_SCHEMA_VERSION: u32 = 1;
pub const IMPLEMENTATION_MANIFEST_MAX_BYTES: usize = 64 * 1024;
pub const IMPLEMENTATION_SUMMARY_MAX_BYTES: usize = 4_000;
pub const IMPLEMENTATION_LIMITATION_MAX_BYTES: usize = 1_000;
pub const IMPLEMENTATION_LIMITATIONS_MAX_ITEMS: usize = 20;
pub const IMPLEMENTATION_EVIDENCE_MAX_ITEMS: usize = 10;
pub const IMPLEMENTATION_EVIDENCE_REFERENCE_MAX_BYTES: usize = 500;
pub const IMPLEMENTATION_EVIDENCE_SUMMARY_MAX_BYTES: usize = 1_000;
pub const IMPLEMENTATION_BLOCKER_SUMMARY_MAX_BYTES: usize = 2_000;
pub const IMPLEMENTATION_BLOCKER_EVIDENCE_MAX_BYTES: usize = 4_000;
pub const IMPLEMENTATION_COMMENT_MARKER_PREFIX: &str = "<!-- symphony:implementation:";
pub const IMPLEMENTATION_COMMENT_MARKER_SUFFIX: &str = " -->";
pub const IMPLEMENTATION_DEFAULT_SPEC_FILE: &str =
    "specs/{issue_identifier}/APPROVED-v{version}.md";
pub const IMPLEMENTATION_MAX_BUNDLE_BYTES_DEFAULT: u64 = 100 * 1024 * 1024;
pub const IMPLEMENTATION_MAX_BUNDLE_BYTES_HARD_CAP: u64 = 1024 * 1024 * 1024;
pub const IMPLEMENTATION_SPEC_PATH_MAX_BYTES: usize = 240;
pub const IMPLEMENTATION_VALIDATION_MAX_COMMANDS: usize = 20;
pub const IMPLEMENTATION_VALIDATION_NAME_MAX_BYTES: usize = 100;
pub const IMPLEMENTATION_VALIDATION_COMMAND_MAX_BYTES: usize = 4_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationMode {
    Preview,
    Automatic,
}

impl ImplementationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Automatic => "automatic",
        }
    }
}

impl Default for ImplementationMode {
    fn default() -> Self {
        Self::Preview
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationValidationCommand {
    pub name: String,
    pub command: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationCompletionRoute {
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationConfig {
    pub enabled: bool,
    pub mode: ImplementationMode,
    pub prompt: String,
    pub repair_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub max_turns: u32,
    pub invocation_timeout_ms: u64,
    pub max_attempts: u32,
    pub max_validation_cycles: u32,
    pub max_bundle_bytes: u64,
    pub spec_file: String,
    pub validation: Vec<ImplementationValidationCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_route: Option<ImplementationCompletionRoute>,
}

impl Default for ImplementationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ImplementationMode::Preview,
            prompt: "prompts/implementation.md".to_string(),
            repair_prompt: "prompts/implementation-repair.md".to_string(),
            model: None,
            max_turns: 20,
            invocation_timeout_ms: 3_600_000,
            max_attempts: 3,
            max_validation_cycles: 3,
            max_bundle_bytes: IMPLEMENTATION_MAX_BUNDLE_BYTES_DEFAULT,
            spec_file: IMPLEMENTATION_DEFAULT_SPEC_FILE.to_string(),
            validation: Vec::new(),
            completion_route: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationTurnKind {
    Implement,
    Repair,
}

impl ImplementationTurnKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implement => "implement",
            Self::Repair => "repair",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationTurnStatus {
    Running,
    Completed,
    Failed,
}

impl ImplementationTurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationTurnRecord {
    pub turn_id: String,
    pub stage_run_id: String,
    pub ordinal: u32,
    pub kind: ImplementationTurnKind,
    pub status: ImplementationTurnStatus,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub execution_profile: String,
    #[serde(default)]
    pub usage: StageUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FactoryError>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManifestStatus {
    Completed,
    Blocked,
}

impl ManifestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CriterionStatus {
    Implemented,
}

impl CriterionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Repository,
    Test,
    Documentation,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::Test => "test",
            Self::Documentation => "documentation",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ImplementationEvidence {
    pub kind: EvidenceKind,
    pub reference: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterionClaim {
    pub index: u32,
    pub status: CriterionStatus,
    pub evidence: Vec<ImplementationEvidence>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlockerKind {
    SpecGap,
    Environment,
    Repository,
}

impl BlockerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SpecGap => "spec_gap",
            Self::Environment => "environment",
            Self::Repository => "repository",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ImplementationBlocker {
    pub kind: BlockerKind,
    pub summary: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ImplementationManifest {
    pub schema_version: u32,
    pub status: ManifestStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<AcceptanceCriterionClaim>,
    #[serde(default)]
    pub known_limitations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<ImplementationBlocker>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfile {
    Local,
    Docker,
}

impl ExecutionProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Docker => "docker",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationAttemptInputs {
    pub approved_artifact_id: String,
    pub approved_version: u32,
    pub approval_issue_revision: String,
    pub base_branch: String,
    pub base_commit: String,
    pub execution_profile: ExecutionProfile,
    pub configuration_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationCommandResult {
    pub name: String,
    pub command_sha256: String,
    pub cycle: u32,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub termination_reason: Option<String>,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub output_sha256: String,
    pub execution_profile: ExecutionProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationCycleRecord {
    pub cycle_id: String,
    pub stage_run_id: String,
    pub cycle: u32,
    pub passed: bool,
    pub commands: Vec<ValidationCommandResult>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundleArtifactRecord {
    pub artifact_id: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub implementation_artifact_id: String,
    pub base_commit: String,
    pub head_commit: String,
    pub tree_sha: String,
    pub sha256: String,
    pub bytes_len: u64,
    pub changed_file_count: u32,
    pub changed_paths: Vec<String>,
    pub approved_spec_path: String,
    pub approved_spec_blob_sha: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub execution_profile: ExecutionProfile,
    pub created_at: DateTime<Utc>,
    pub verified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationArtifactRecord {
    pub artifact_id: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub approved_artifact_id: String,
    pub approved_version: u32,
    pub issue_revision: String,
    pub configuration_revision: String,
    pub manifest: ImplementationManifest,
    pub base_commit: String,
    pub head_commit: Option<String>,
    pub approved_spec_path: String,
    pub validation_cycles: u32,
    pub execution_profile: ExecutionProfile,
    pub received_at: DateTime<Utc>,
    pub bytes_len: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationPublicationKind {
    Preview,
    Automatic,
    Diagnostic,
}

impl ImplementationPublicationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Automatic => "automatic",
            Self::Diagnostic => "diagnostic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationPublicationIntent {
    pub intent_id: String,
    pub run_id: String,
    pub artifact_id: Option<String>,
    pub kind: ImplementationPublicationKind,
    pub status: crate::triage::domain::PublicationStatus,
    pub completed_steps: Vec<String>,
    pub retry_count: u32,
    pub last_error: Option<FactoryError>,
    pub comment_id: Option<String>,
    pub publisher_login: Option<String>,
    pub desired_effects: serde_json::Value,
    pub observed_baseline: serde_json::Value,
    pub expected_projection: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationDecision {
    Pending,
    Previewed,
    AwaitingHuman,
    Published,
    Blocked,
    Failed,
    Conflict,
}

impl ImplementationDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Previewed => "previewed",
            Self::AwaitingHuman => "awaiting_human",
            Self::Published => "published",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Conflict => "conflict",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationRunState {
    pub run_id: String,
    pub approved_artifact_id: String,
    pub approved_version: u32,
    pub configuration_revision: String,
    pub decision: ImplementationDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successful_artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<ImplementationBlocker>,
    pub updated_at: DateTime<Utc>,
}

/// A3 metric aggregates. Base fields reuse the triage shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImplementationMetricsAggregate {
    #[serde(flatten)]
    pub base: crate::triage::domain::TriageMetricsAggregate,
    pub eligible_approvals: u64,
    pub validation_cycles: u64,
    pub validation_first_pass: u64,
    pub validation_repairs: u64,
    pub validation_exhausted: u64,
    pub spec_gaps: u64,
    pub preview_publications: u64,
    pub automatic_publications: u64,
    pub publication_conflicts: u64,
    pub local_attempts: u64,
    pub docker_attempts: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implementation_defaults_match_a3_contract() {
        let config = ImplementationConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.mode, ImplementationMode::Preview);
        assert_eq!(config.prompt, "prompts/implementation.md");
        assert_eq!(config.repair_prompt, "prompts/implementation-repair.md");
        assert_eq!(config.invocation_timeout_ms, 3_600_000);
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.max_validation_cycles, 3);
        assert_eq!(config.max_bundle_bytes, IMPLEMENTATION_MAX_BUNDLE_BYTES_DEFAULT);
        assert_eq!(config.spec_file, IMPLEMENTATION_DEFAULT_SPEC_FILE);
        assert!(config.validation.is_empty());
        assert!(config.completion_route.is_none());
    }
}
