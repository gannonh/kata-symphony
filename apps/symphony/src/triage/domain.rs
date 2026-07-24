use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const TRIAGE_SCHEMA_VERSION: u32 = 1;
pub const TRIAGE_COMMENT_MARKER_PREFIX: &str = "<!-- symphony:triage:";
pub const TRIAGE_COMMENT_MARKER_SUFFIX: &str = " -->";
pub const ARTIFACT_MAX_BYTES: usize = 64 * 1024;
pub const FACTORY_EVENT_PAYLOAD_MAX_BYTES: usize = 64 * 1024;
pub const FACTORY_ERROR_STRING_MAX_BYTES: usize = 2_000;
pub const ARTIFACT_RATIONALE_MAX_BYTES: usize = 2_000;
pub const ARTIFACT_EVIDENCE_REFERENCE_MAX_BYTES: usize = 500;
pub const ARTIFACT_EVIDENCE_SUMMARY_MAX_BYTES: usize = 1_000;
pub const ARTIFACT_NEXT_ACTION_MAX_BYTES: usize = 1_000;
pub const ARTIFACT_CLARIFICATION_MAX_BYTES: usize = 1_000;
pub const ARTIFACT_REPRODUCTION_OUTCOME_MAX_BYTES: usize = 2_000;
pub const ARTIFACT_EVIDENCE_MIN_ITEMS: usize = 1;
pub const ARTIFACT_EVIDENCE_MAX_ITEMS: usize = 20;
pub const TRIAGE_LEASE_RENEW_INTERVAL_MS: i64 = 10_000;
pub const TRIAGE_LEASE_STALE_AFTER_MS: i64 = 60_000;
pub const TRIAGE_STAGE_NAME: &str = "triage";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum TriageMode {
    #[default]
    Preview,
    Automatic,
}

impl TriageMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "preview" => Some(Self::Preview),
            "automatic" => Some(Self::Automatic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Automatic => "automatic",
        }
    }
}

impl fmt::Display for TriageMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TriageRoute {
    Implement,
    Spec,
    NeedsInformation,
    Park,
    HumanOwned,
}

impl TriageRoute {
    pub fn parse(value: &str) -> Option<Self> {
        parse_route(value)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implement => "implement",
            Self::Spec => "spec",
            Self::NeedsInformation => "needs_information",
            Self::Park => "park",
            Self::HumanOwned => "human_owned",
        }
    }
}

impl fmt::Display for TriageRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    Low,
    Medium,
    High,
}

impl RiskClass {
    pub fn parse(value: &str) -> Option<Self> {
        parse_risk_class(value)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl fmt::Display for RiskClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Issue,
    Repository,
    Reproduction,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::Repository => "repository",
            Self::Reproduction => "reproduction",
        }
    }
}

impl fmt::Display for EvidenceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriageEvidence {
    pub kind: EvidenceKind,
    pub reference: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReproductionSummary {
    pub attempted: bool,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriageArtifact {
    pub schema_version: u32,
    pub route: TriageRoute,
    pub risk_class: RiskClass,
    pub rationale: String,
    pub evidence: Vec<TriageEvidence>,
    pub next_action: String,
    pub clarification_question: Option<String>,
    pub reproduction: Option<ReproductionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteMapping {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriageRoutesConfig {
    pub implement: RouteMapping,
    pub spec: RouteMapping,
    pub needs_information: RouteMapping,
    pub park: RouteMapping,
    pub human_owned: RouteMapping,
}

impl Default for TriageRoutesConfig {
    fn default() -> Self {
        Self {
            implement: RouteMapping {
                label: "ready-for-agent".to_string(),
                state: Some("Todo".to_string()),
            },
            spec: RouteMapping {
                label: "ready-to-spec".to_string(),
                state: None,
            },
            needs_information: RouteMapping {
                label: "needs-info".to_string(),
                state: None,
            },
            park: RouteMapping {
                label: "wait-to-implement".to_string(),
                state: None,
            },
            human_owned: RouteMapping {
                label: "ready-for-human".to_string(),
                state: None,
            },
        }
    }
}

impl TriageRoutesConfig {
    pub fn mapping_for(&self, route: TriageRoute) -> &RouteMapping {
        match route {
            TriageRoute::Implement => &self.implement,
            TriageRoute::Spec => &self.spec,
            TriageRoute::NeedsInformation => &self.needs_information,
            TriageRoute::Park => &self.park,
            TriageRoute::HumanOwned => &self.human_owned,
        }
    }

    pub fn managed_labels(&self) -> Vec<String> {
        vec![
            self.implement.label.clone(),
            self.spec.label.clone(),
            self.needs_information.label.clone(),
            self.park.label.clone(),
            self.human_owned.label.clone(),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriageConfig {
    pub enabled: bool,
    pub mode: TriageMode,
    pub intake_label: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub turn_timeout_ms: u64,
    pub max_attempts: u32,
    pub max_intake_pages: u32,
    pub routes: TriageRoutesConfig,
}

impl Default for TriageConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: TriageMode::Preview,
            intake_label: "needs-triage".to_string(),
            prompt: "prompts/triage.md".to_string(),
            model: None,
            turn_timeout_ms: 900_000,
            max_attempts: 3,
            max_intake_pages: 100,
            routes: TriageRoutesConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub busy_timeout_ms: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: None,
            busy_timeout_ms: 5_000,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactoryRunStatus {
    Active,
    Waiting,
    Completed,
    Failed,
    Ineligible,
}

impl FactoryRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Ineligible => "ineligible",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl StageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Interrupted)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStatus {
    None,
    Pending,
    Applied,
    Blocked,
    Conflict,
}

impl PublicationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Blocked => "blocked",
            Self::Conflict => "conflict",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PublicationMode {
    Preview,
    Automatic,
}

impl PublicationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Automatic => "automatic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryError {
    pub code: String,
    pub component: String,
    pub remediation: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_step: Option<String>,
}

impl FactoryError {
    pub fn new(
        code: impl Into<String>,
        component: impl Into<String>,
        remediation: impl Into<String>,
        retryable: bool,
        publication_step: Option<String>,
    ) -> Self {
        Self {
            code: truncate_utf8_bytes(&code.into(), FACTORY_ERROR_STRING_MAX_BYTES),
            component: truncate_utf8_bytes(&component.into(), FACTORY_ERROR_STRING_MAX_BYTES),
            remediation: truncate_utf8_bytes(&remediation.into(), FACTORY_ERROR_STRING_MAX_BYTES),
            retryable,
            publication_step: publication_step
                .map(|step| truncate_utf8_bytes(&step, FACTORY_ERROR_STRING_MAX_BYTES)),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunRecord {
    pub run_id: String,
    pub forge_host: String,
    pub repository: String,
    pub issue_id: String,
    pub issue_identifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_revision: Option<String>,
    pub status: FactoryRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_stage: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageRunRecord {
    pub stage_run_id: String,
    pub run_id: String,
    pub stage: String,
    pub issue_revision: String,
    pub configuration_revision: String,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_instance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_group_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_start_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_heartbeat_at: Option<DateTime<Utc>>,
    pub status: StageStatus,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub usage: StageUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FactoryError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub issue_revision: String,
    pub configuration_revision: String,
    pub route_mapping_hash: String,
    pub artifact: TriageArtifact,
    pub received_at: DateTime<Utc>,
    pub bytes_len: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicationIntentRecord {
    pub intent_id: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    pub mode: PublicationMode,
    pub status: PublicationStatus,
    pub intake_label: String,
    pub route_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_state: Option<String>,
    pub route_mapping_hash: String,
    #[serde(default)]
    pub completed_steps: Vec<String>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<FactoryError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_login: Option<String>,
    #[serde(default)]
    pub desired_effects: serde_json::Value,
    #[serde(default)]
    pub observed_baseline: serde_json::Value,
    #[serde(default)]
    pub expected_projection: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Managed GitHub state the publisher is allowed to compare and mutate: the
/// configured route labels plus intake label, and the Projects v2 state.
///
/// Values are normalized for comparison, so a projection is only meaningful as
/// a conflict-detection artifact and never as display text.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedProjection {
    #[serde(default)]
    pub managed_labels: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub project_state: Option<String>,
}

impl ManagedProjection {
    /// Project `labels` down to the managed vocabulary, discarding every label
    /// Symphony does not own.
    pub fn observe<'a>(
        labels: impl IntoIterator<Item = &'a str>,
        managed_labels: &std::collections::BTreeSet<String>,
        project_state: Option<&str>,
    ) -> Self {
        let managed_labels = labels
            .into_iter()
            .map(normalize_managed_value)
            .filter(|label| managed_labels.contains(label))
            .collect();
        Self {
            managed_labels,
            project_state: project_state.map(normalize_managed_value).filter(|state| !state.is_empty()),
        }
    }

    /// Parse a persisted projection. Returns `None` when no projection was
    /// recorded yet, which is distinct from an empty projection.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        value.get("managed_labels")?;
        serde_json::from_value(value.clone()).ok()
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "managed_labels": self.managed_labels,
            "project_state": self.project_state,
        })
    }

    pub fn with_label(&self, label: &str) -> Self {
        let mut next = self.clone();
        next.managed_labels.insert(normalize_managed_value(label));
        next
    }

    pub fn without_label(&self, label: &str) -> Self {
        let mut next = self.clone();
        next.managed_labels.remove(&normalize_managed_value(label));
        next
    }

    pub fn with_project_state(&self, state: &str) -> Self {
        let mut next = self.clone();
        next.project_state = Some(normalize_managed_value(state));
        next
    }

    pub fn contains_label(&self, label: &str) -> bool {
        self.managed_labels.contains(&normalize_managed_value(label))
    }
}

/// Managed label vocabulary: configured route labels plus the recorded intake label.
pub fn managed_label_set<'a>(
    labels: impl IntoIterator<Item = &'a str>,
) -> std::collections::BTreeSet<String> {
    labels
        .into_iter()
        .map(normalize_managed_value)
        .filter(|label| !label.is_empty())
        .collect()
}

fn normalize_managed_value(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryEventRecord {
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_run_id: Option<String>,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriageMetricsDuration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p50_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p95_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TriageMetricsTokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriageMetricsAggregate {
    pub total_attempts: u64,
    pub completed_attempts: u64,
    pub failed_attempts: u64,
    pub ineligible_issues: u64,
    pub route_counts: std::collections::BTreeMap<String, u64>,
    pub correction_count: u64,
    pub correction_rate: f64,
    pub duration: TriageMetricsDuration,
    /// Token totals keyed by `{harness}/{model}` with null models as `unknown`.
    pub tokens_by_harness_model: std::collections::BTreeMap<String, TriageMetricsTokenTotals>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

impl Default for TriageMetricsAggregate {
    fn default() -> Self {
        Self {
            total_attempts: 0,
            completed_attempts: 0,
            failed_attempts: 0,
            ineligible_issues: 0,
            route_counts: std::collections::BTreeMap::new(),
            correction_count: 0,
            correction_rate: 0.0,
            duration: TriageMetricsDuration {
                average_ms: None,
                p50_ms: None,
                p95_ms: None,
            },
            tokens_by_harness_model: std::collections::BTreeMap::new(),
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
        }
    }
}

pub fn parse_route(value: &str) -> Option<TriageRoute> {
    match value.trim().to_ascii_lowercase().as_str() {
        "implement" => Some(TriageRoute::Implement),
        "spec" => Some(TriageRoute::Spec),
        "needs_information" | "needs-information" | "needs information" => {
            Some(TriageRoute::NeedsInformation)
        }
        "park" => Some(TriageRoute::Park),
        "human_owned" | "human-owned" | "human owned" => Some(TriageRoute::HumanOwned),
        _ => None,
    }
}

pub fn parse_risk_class(value: &str) -> Option<RiskClass> {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => Some(RiskClass::Low),
        "medium" => Some(RiskClass::Medium),
        "high" => Some(RiskClass::High),
        _ => None,
    }
}

pub fn truncate_utf8_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }

    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triage_config_defaults_match_spec() {
        let config = TriageConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.mode, TriageMode::Preview);
        assert_eq!(config.intake_label, "needs-triage");
        assert_eq!(config.turn_timeout_ms, 900_000);
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.max_intake_pages, 100);
        assert_eq!(config.routes.implement.label, "ready-for-agent");
        assert_eq!(config.routes.implement.state.as_deref(), Some("Todo"));
        assert_eq!(StorageConfig::default().busy_timeout_ms, 5_000);
    }

    #[test]
    fn parse_helpers_accept_canonical_values() {
        assert_eq!(
            parse_route("needs_information"),
            Some(TriageRoute::NeedsInformation)
        );
        assert_eq!(parse_route("human-owned"), Some(TriageRoute::HumanOwned));
        assert_eq!(parse_risk_class("HIGH"), Some(RiskClass::High));
        assert_eq!(parse_route("unknown"), None);
    }

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        assert_eq!(truncate_utf8_bytes("abcdef", 3), "abc");
        assert_eq!(truncate_utf8_bytes("ééé", 5), "éé");
    }
}
