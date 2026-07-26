use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::triage::domain::{FactoryError, StageUsage};

pub const SPEC_STAGE_NAME: &str = "spec";
pub const SPEC_SCHEMA_VERSION: u32 = 1;
pub const SPEC_ARTIFACT_MAX_BYTES: usize = 64 * 1024;
pub const SPEC_SECTION_MAX_BYTES: usize = 16_000;
pub const SPEC_LIST_ITEM_MAX_BYTES: usize = 1_000;
pub const SPEC_ACCEPTANCE_CRITERIA_MAX_ITEMS: usize = 50;
pub const SPEC_OPEN_DECISIONS_MAX_ITEMS: usize = 50;
pub const SPEC_FINDINGS_MAX_ITEMS: usize = 30;
pub const SPEC_COMMENT_MARKER_PREFIX: &str = "<!-- symphony:spec:";
pub const SPEC_COMMENT_MARKER_SUFFIX: &str = " -->";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecArtifact {
    pub schema_version: u32,
    pub product_behavior: String,
    pub technical_approach: String,
    pub acceptance_criteria: Vec<String>,
    pub open_decisions: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Pass,
    Revise,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Blocking,
    Advisory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingSection {
    ProductBehavior,
    TechnicalApproach,
    AcceptanceCriteria,
    OpenDecisions,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewFinding {
    pub severity: FindingSeverity,
    pub section: FindingSection,
    pub summary: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewFindings {
    pub schema_version: u32,
    pub verdict: ReviewVerdict,
    pub findings: Vec<ReviewFinding>,
}

impl ReviewFindings {
    pub fn blocking(&self) -> impl Iterator<Item = &ReviewFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.severity == FindingSeverity::Blocking)
    }

    pub fn blocking_count(&self) -> usize {
        self.blocking().count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecPromptsConfig {
    pub draft: String,
    pub review: String,
    pub revise: String,
}

impl Default for SpecPromptsConfig {
    fn default() -> Self {
        Self {
            draft: "prompts/spec-draft.md".to_string(),
            review: "prompts/spec-review.md".to_string(),
            revise: "prompts/spec-revise.md".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecDecisionLabels {
    pub approved: String,
    pub revise: String,
}

impl Default for SpecDecisionLabels {
    fn default() -> Self {
        Self {
            approved: "spec-approved".to_string(),
            revise: "spec-revise".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecApprovalRoute {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl Default for SpecApprovalRoute {
    fn default() -> Self {
        Self {
            label: "ready-for-agent".to_string(),
            state: Some("Todo".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecConfig {
    pub enabled: bool,
    pub intake_label: String,
    pub prompts: SpecPromptsConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_model: Option<String>,
    pub turn_timeout_ms: u64,
    pub max_intake_pages: u32,
    pub max_review_cycles: u32,
    pub max_attempts: u32,
    pub max_revision_requests: u32,
    pub labels: SpecDecisionLabels,
    pub approval_route: SpecApprovalRoute,
}

impl Default for SpecConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            intake_label: "ready-to-spec".to_string(),
            prompts: SpecPromptsConfig::default(),
            model: None,
            review_model: None,
            turn_timeout_ms: 1_800_000,
            max_intake_pages: 100,
            max_review_cycles: 3,
            max_attempts: 3,
            max_revision_requests: 3,
            labels: SpecDecisionLabels::default(),
            approval_route: SpecApprovalRoute::default(),
        }
    }
}

impl SpecConfig {
    pub fn managed_labels(&self) -> Vec<String> {
        vec![
            self.intake_label.clone(),
            self.labels.approved.clone(),
            self.labels.revise.clone(),
            self.approval_route.label.clone(),
        ]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpecTurnKind {
    Draft,
    Review,
    Revise,
}

impl SpecTurnKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Review => "review",
            Self::Revise => "revise",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpecTurnStatus {
    Running,
    Completed,
    Failed,
}

impl SpecTurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecTurnRecord {
    pub turn_id: String,
    pub stage_run_id: String,
    pub ordinal: u32,
    pub kind: SpecTurnKind,
    pub status: SpecTurnStatus,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecArtifactRecord {
    pub artifact_id: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub issue_revision: String,
    pub configuration_revision: String,
    pub version: u32,
    pub artifact: SpecArtifact,
    pub review_cycles: u32,
    pub unresolved_blocking_findings: Vec<ReviewFinding>,
    pub received_at: DateTime<Utc>,
    pub bytes_len: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpecPublicationKind {
    Preview,
    Approval,
    Diagnostic,
}

impl SpecPublicationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Approval => "approval",
            Self::Diagnostic => "diagnostic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecPublicationIntent {
    pub intent_id: String,
    pub run_id: String,
    pub artifact_id: Option<String>,
    pub kind: SpecPublicationKind,
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
pub enum SpecDecision {
    AwaitingDecision,
    RevisionRequested,
    PendingApproval,
    Approved,
    Blocked,
    Conflict,
}

impl SpecDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingDecision => "awaiting_decision",
            Self::RevisionRequested => "revision_requested",
            Self::PendingApproval => "pending_approval",
            Self::Approved => "approved",
            Self::Blocked => "blocked",
            Self::Conflict => "conflict",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecRunState {
    pub run_id: String,
    pub revision_requests_used: u32,
    pub pending_approval_version: Option<u32>,
    pub approved_version: Option<u32>,
    pub approved_artifact_id: Option<String>,
    pub decision: SpecDecision,
    pub updated_at: DateTime<Utc>,
}

/// Review-cycle aggregates across published spec versions.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct SpecReviewCycleMetrics {
    pub average: Option<f64>,
    pub max: Option<u64>,
}

/// A2 metric aggregates. `stage`, attempt counts, ineligible count, duration,
/// and per-harness/model tokens reuse the triage shape; the remaining fields
/// are spec-specific and absent from `?stage=triage` responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecMetricsAggregate {
    #[serde(flatten)]
    pub base: crate::triage::domain::TriageMetricsAggregate,
    pub review_cycles: SpecReviewCycleMetrics,
    /// Completed attempts that published with zero unresolved blocking findings.
    pub converged_attempts: u64,
    /// `converged_attempts / published versions`.
    pub convergence_rate: f64,
    /// Human revision requests consumed across all runs.
    pub revision_requests: u64,
    /// Publication-to-decision latency across approved runs.
    pub approval_latency: crate::triage::domain::TriageMetricsDuration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_match_a2_contract() {
        let config = SpecConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.intake_label, "ready-to-spec");
        assert_eq!(config.turn_timeout_ms, 1_800_000);
        assert_eq!(config.max_intake_pages, 100);
        assert_eq!(config.max_review_cycles, 3);
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.max_revision_requests, 3);
        assert_eq!(config.labels.approved, "spec-approved");
        assert_eq!(config.labels.revise, "spec-revise");
        assert_eq!(config.approval_route.label, "ready-for-agent");
        assert_eq!(config.approval_route.state.as_deref(), Some("Todo"));
    }
}
