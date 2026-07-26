use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::ops::Deref;

// ── Issue (spec §4.1.1) ────────────────────────────────────────────────

/// Normalized issue record from the tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    pub state: String,
    #[serde(default)]
    pub branch_name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub assignee_id: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<BlockerRef>,
    /// Whether this issue is routable to this worker (assignee filter).
    #[serde(default = "default_true")]
    pub assigned_to_worker: bool,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// Number of child sub-issues (0 for flat tickets, >0 for slices).
    #[serde(default)]
    pub children_count: u32,
    /// Parent issue identifier (e.g. "KAT-928") if this is a sub-issue.
    #[serde(default)]
    pub parent_identifier: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Canonical Kata workflow phase vocabulary shared across CLI/Desktop/Symphony.
pub const KATA_PHASE_NAMES: [&str; 8] = [
    "Backlog",
    "Todo",
    "In Progress",
    "Agent Review",
    "Human Review",
    "Merging",
    "Rework",
    "Done",
];

/// Return the canonical Kata phase name when the provided value matches one.
///
/// Matching is case-insensitive and whitespace/underscore/hyphen agnostic.
pub fn canonical_kata_phase_name(state_name: &str) -> Option<&'static str> {
    let normalized = normalize_kata_phase_key(state_name);
    KATA_PHASE_NAMES
        .iter()
        .copied()
        .find(|phase| normalize_kata_phase_key(phase) == normalized)
}

fn normalize_kata_phase_key(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse a Kata identifier prefix from a GitHub title, e.g. `[S01] Build`.
///
/// Accepted prefixes are `M`, `S`, and `T` with one or more trailing digits.
pub fn parse_kata_identifier(title: &str) -> Option<String> {
    let trimmed = title.trim_start();
    let bracketed = trimmed.strip_prefix('[')?;
    let end = bracketed.find(']')?;
    let token = &bracketed[..end];

    let mut chars = token.chars();
    let prefix = chars.next()?.to_ascii_uppercase();
    if !matches!(prefix, 'M' | 'S' | 'T') {
        return None;
    }

    let digits = chars.as_str();
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    Some(format!("[{prefix}{digits}]"))
}

/// Parse parent issue metadata from body lines such as:
/// - `Parent: #10`
/// - `Part of: #10`
/// - `**Parent:** #10`
pub fn parse_parent_issue_reference(body: &str) -> Option<String> {
    for line in body.lines() {
        let normalized = line.trim().replace('*', "");
        let lower = normalized.to_ascii_lowercase();

        if !(lower.starts_with("parent:") || lower.starts_with("part of:")) {
            continue;
        }

        if let Some(identifier) = extract_first_issue_reference(&normalized) {
            return Some(identifier);
        }
    }

    None
}

fn extract_first_issue_reference(line: &str) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] != '#' {
            index += 1;
            continue;
        }

        let mut end = index + 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }

        if end > index + 1 {
            let digits: String = chars[index + 1..end].iter().collect();
            return Some(format!("#{digits}"));
        }

        index += 1;
    }

    None
}

/// A blocker reference from an inverse relation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}

/// A blocked issue entry for the orchestrator snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedIssueEntry {
    pub issue_id: String,
    pub identifier: String,
    pub title: String,
    pub state: String,
    pub blocker_identifiers: Vec<String>,
}

/// A worker escalation request emitted when human input is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRequest {
    pub id: String,
    pub issue_id: String,
    pub issue_identifier: String,
    pub method: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub timeout_ms: u64,
}

/// Operator response payload routed back to a waiting worker escalation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationResponse {
    pub request_id: String,
    pub response: serde_json::Value,
    #[serde(default)]
    pub responder_id: Option<String>,
    pub responded_at: DateTime<Utc>,
}

/// Snapshot-safe view of currently pending escalations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingEscalation {
    pub request_id: String,
    pub issue_id: String,
    pub issue_identifier: String,
    pub method: String,
    pub preview: String,
    pub created_at: DateTime<Utc>,
    pub timeout_ms: u64,
}

// ── Shared context contract (M002/S06) ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ContextScope {
    Project,
    Milestone(String),
    Label(String),
}

impl ContextScope {
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("project") {
            return Some(Self::Project);
        }

        let (raw_kind, raw_value) = normalized.split_once(':')?;
        let kind = raw_kind.trim().to_ascii_lowercase();
        let raw_value = raw_value.trim();
        if raw_value.is_empty() {
            return None;
        }

        match kind.as_str() {
            "milestone" => Some(Self::Milestone(raw_value.to_string())),
            "label" => Some(Self::Label(raw_value.to_ascii_lowercase())),
            _ => None,
        }
    }

    pub fn as_scope_key(&self) -> String {
        match self {
            Self::Project => "project".to_string(),
            Self::Milestone(id) => format!("milestone:{id}"),
            Self::Label(label) => format!("label:{label}"),
        }
    }
}

impl fmt::Display for ContextScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_scope_key())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextEntry {
    pub id: String,
    pub author_issue: String,
    pub scope: ContextScope,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub ttl_ms: u64,
}

// ── Event stream contract (M002/S01) ───────────────────────────────────

pub const SYMPHONY_EVENT_STREAM_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Snapshot,
    Runtime,
    Worker,
    Tool,
    Heartbeat,
    EscalationCreated,
    EscalationResponded,
    EscalationTimedOut,
    EscalationCancelled,
    SharedContextWritten,
    SharedContextExpired,
    SupervisorSteer,
    SupervisorConflictDetected,
    SupervisorEscalated,
    SupervisorPatternDetected,
    Triage,
}

impl EventKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "snapshot" => Some(Self::Snapshot),
            "runtime" => Some(Self::Runtime),
            "worker" => Some(Self::Worker),
            "tool" => Some(Self::Tool),
            "heartbeat" => Some(Self::Heartbeat),
            "escalation_created" => Some(Self::EscalationCreated),
            "escalation_responded" => Some(Self::EscalationResponded),
            "escalation_timed_out" => Some(Self::EscalationTimedOut),
            "escalation_cancelled" => Some(Self::EscalationCancelled),
            "shared_context_written" => Some(Self::SharedContextWritten),
            "shared_context_expired" => Some(Self::SharedContextExpired),
            "supervisor_steer" => Some(Self::SupervisorSteer),
            "supervisor_conflict_detected" => Some(Self::SupervisorConflictDetected),
            "supervisor_escalated" => Some(Self::SupervisorEscalated),
            "supervisor_pattern_detected" => Some(Self::SupervisorPatternDetected),
            "triage" => Some(Self::Triage),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Runtime => "runtime",
            Self::Worker => "worker",
            Self::Tool => "tool",
            Self::Heartbeat => "heartbeat",
            Self::EscalationCreated => "escalation_created",
            Self::EscalationResponded => "escalation_responded",
            Self::EscalationTimedOut => "escalation_timed_out",
            Self::EscalationCancelled => "escalation_cancelled",
            Self::SharedContextWritten => "shared_context_written",
            Self::SharedContextExpired => "shared_context_expired",
            Self::SupervisorSteer => "supervisor_steer",
            Self::SupervisorConflictDetected => "supervisor_conflict_detected",
            Self::SupervisorEscalated => "supervisor_escalated",
            Self::SupervisorPatternDetected => "supervisor_pattern_detected",
            Self::Triage => "triage",
        }
    }

    pub fn variants() -> &'static [&'static str] {
        &[
            "snapshot",
            "runtime",
            "worker",
            "tool",
            "heartbeat",
            "escalation_created",
            "escalation_responded",
            "escalation_timed_out",
            "escalation_cancelled",
            "shared_context_written",
            "shared_context_expired",
            "supervisor_steer",
            "supervisor_conflict_detected",
            "supervisor_escalated",
            "supervisor_pattern_detected",
            "triage",
        ]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity {
    Debug,
    Info,
    Warn,
    Error,
}

impl EventSeverity {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn variants() -> &'static [&'static str] {
        &["debug", "info", "warn", "error"]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymphonyEventEnvelope {
    pub version: String,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub kind: EventKind,
    pub severity: EventSeverity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    pub event: String,
    pub payload: serde_json::Value,
}

impl SymphonyEventEnvelope {
    pub fn new(
        sequence: u64,
        timestamp: DateTime<Utc>,
        kind: EventKind,
        severity: EventSeverity,
        issue: Option<String>,
        event: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            version: SYMPHONY_EVENT_STREAM_VERSION.to_string(),
            sequence,
            timestamp,
            kind,
            severity,
            issue,
            event: event.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventFilter {
    #[serde(default)]
    pub issues: BTreeSet<String>,
    #[serde(default)]
    pub kinds: BTreeSet<EventKind>,
    #[serde(default)]
    pub severities: BTreeSet<EventSeverity>,
}

impl EventFilter {
    pub fn matches(&self, event: &SymphonyEventEnvelope) -> bool {
        let issue_matches = if self.issues.is_empty() {
            true
        } else {
            event
                .issue
                .as_ref()
                .map(|issue| self.issues.contains(issue))
                .unwrap_or(false)
        };

        let kind_matches = self.kinds.is_empty() || self.kinds.contains(&event.kind);
        let severity_matches =
            self.severities.is_empty() || self.severities.contains(&event.severity);

        issue_matches && kind_matches && severity_matches
    }

    pub fn has_any_constraints(&self) -> bool {
        !(self.issues.is_empty() && self.kinds.is_empty() && self.severities.is_empty())
    }
}

// ── ApiKey ─────────────────────────────────────────────────────────────

/// A redacted API-key value.
///
/// `Debug` always prints `[REDACTED]` so the raw key can never leak into
/// tracing/log output, even via `{:?}` on a type that transitively contains
/// this field.  Use [`as_str`](ApiKey::as_str) or the `Deref<Target = str>`
/// impl to access the underlying value.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(s: impl Into<String>) -> Self {
        ApiKey(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl PartialEq for ApiKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Deref for ApiKey {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<String> for ApiKey {
    fn from(s: String) -> Self {
        ApiKey(s)
    }
}

impl From<&str> for ApiKey {
    fn from(s: &str) -> Self {
        ApiKey(s.to_string())
    }
}

// ── Workflow Definition (spec §4.1.2) ──────────────────────────────────

/// Parsed WORKFLOW.md payload.
#[derive(Debug, Clone)]
pub struct WorkflowDefinition {
    /// YAML front matter as a raw map.
    pub config: serde_yaml::Value,
    /// Markdown body after front matter, trimmed.
    pub prompt_template: String,
}

// ── Service Config (spec §4.1.3 + §5.3) ───────────────────────────────

/// Per-state prompt configuration.
///
/// When present, the orchestrator selects a prompt template based on the
/// issue's tracker state at dispatch time instead of using the monolithic
/// `prompt_template` body.
#[derive(Debug, Clone, Default)]
pub struct PromptsConfig {
    /// System-level preamble injected every turn (agent identity, tool
    /// guidance, repo-agnostic instructions).
    pub system: Option<String>,
    /// Repository-specific context injected every turn (build commands,
    /// conventions, directory layout).
    pub repo: Option<String>,
    /// Legacy single-file preamble. Superseded by `system` + `repo` but
    /// still honoured for backward compatibility.
    pub shared: Option<String>,
    /// Map of normalized state name → prompt template content.
    pub by_state: HashMap<String, String>,
    /// Fallback template for states not in `by_state`.
    pub default: Option<String>,
}

/// Notification configuration.
#[derive(Debug, Clone, Default)]
pub struct NotificationsConfig {
    pub slack: Option<SlackConfig>,
}

/// Slack webhook configuration.
#[derive(Clone)]
pub struct SlackConfig {
    pub webhook_url: String,
    pub events: Vec<String>,
}

impl fmt::Debug for SlackConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlackConfig")
            .field("webhook_url", &"[REDACTED]")
            .field("events", &self.events)
            .finish()
    }
}

/// Top-level typed runtime config derived from WorkflowDefinition.config.
#[derive(Debug, Clone, Default)]
pub struct ServiceConfig {
    pub tracker: TrackerConfig,
    pub polling: PollingConfig,
    pub workspace: WorkspaceConfig,
    pub worker: WorkerConfig,
    pub agent: AgentConfig,
    pub codex: CodexConfig,
    pub pi_agent: PiAgentConfig,
    pub agent_backend: AgentBackend,
    pub hooks: HooksConfig,
    pub server: ServerConfig,
    pub prompts: Option<PromptsConfig>,
    pub notifications: Option<NotificationsConfig>,
    pub shared_context: SharedContextConfig,
    pub supervisor: SupervisorConfig,
    pub storage: crate::triage::domain::StorageConfig,
    pub triage: crate::triage::domain::TriageConfig,
    pub spec: crate::spec::domain::SpecConfig,
}

/// Tracker configuration (spec §5.3.1).
#[derive(Debug, Clone)]
pub struct TrackerConfig {
    pub kind: Option<String>,
    pub endpoint: String,
    /// Tracker API key/token. Stored as [`ApiKey`] so it cannot accidentally
    /// appear in debug/tracing output — `{:?}` on this struct prints `[REDACTED]`.
    pub api_key: Option<ApiKey>,
    // Linear-specific fields
    pub project_slug: Option<String>,
    pub workspace_slug: Option<String>,
    // GitHub-specific fields
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub github_project_owner_type: Option<GithubProjectOwnerType>,
    pub github_project_number: Option<u64>,
    pub label_prefix: Option<String>,
    // Shared tracker fields
    pub assignee: Option<String>,
    pub active_states: Vec<String>,
    pub terminal_states: Vec<String>,
    /// Labels that disqualify an issue from dispatch.  Any issue carrying at
    /// least one of these labels (case-insensitive) is silently skipped.
    /// Use `["kata:task"]` to prevent Symphony from dispatching Kata sub-tasks.
    pub exclude_labels: Vec<String>,
}

const DEFAULT_LINEAR_WORKSPACE_SLUG: &str = "kata-sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubProjectOwnerType {
    User,
    Org,
}

impl GithubProjectOwnerType {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "user" => Some(Self::User),
            "org" | "organization" => Some(Self::Org),
            _ => None,
        }
    }

    pub fn url_segment(self) -> &'static str {
        match self {
            Self::User => "users",
            Self::Org => "orgs",
        }
    }
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            kind: None,
            endpoint: "https://api.linear.app/graphql".to_string(),
            api_key: None,
            project_slug: None,
            workspace_slug: None,
            repo_owner: None,
            repo_name: None,
            github_project_owner_type: None,
            github_project_number: None,
            label_prefix: None,
            assignee: None,
            active_states: vec!["Todo".to_string(), "In Progress".to_string()],
            terminal_states: vec![
                "Closed".to_string(),
                "Cancelled".to_string(),
                "Canceled".to_string(),
                "Duplicate".to_string(),
                "Done".to_string(),
            ],
            exclude_labels: vec![],
        }
    }
}

impl TrackerConfig {
    /// Build a browser URL for the configured tracker project.
    ///
    /// - GitHub kind: `https://github.com/{users|orgs}/{owner}/projects/{project_number}`
    /// - Linear kind (or default): `https://linear.app/{workspace}/project/{slug}`
    pub fn tracker_project_url(&self) -> Option<String> {
        match self
            .kind
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
        {
            Some(kind) if kind.eq_ignore_ascii_case("github") => {
                let owner = self.repo_owner.as_deref()?.trim();
                let owner_type = self.github_project_owner_type?;
                let project_number = self.github_project_number?;
                if owner.is_empty() {
                    return None;
                }

                Some(format!(
                    "https://github.com/{}/{owner}/projects/{project_number}",
                    owner_type.url_segment()
                ))
            }
            _ => self.build_linear_url(),
        }
    }

    fn build_linear_url(&self) -> Option<String> {
        let project_slug = self.project_slug.as_deref()?.trim();
        if project_slug.is_empty() {
            return None;
        }
        let workspace_slug = self
            .workspace_slug
            .as_deref()
            .unwrap_or(DEFAULT_LINEAR_WORKSPACE_SLUG)
            .trim();
        if workspace_slug.is_empty() {
            return None;
        }

        Some(format!(
            "https://linear.app/{workspace_slug}/project/{project_slug}"
        ))
    }
}

/// Polling configuration (spec §5.3.2).
#[derive(Debug, Clone)]
pub struct PollingConfig {
    pub interval_ms: u64,
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            interval_ms: 30_000,
        }
    }
}

/// Shared context configuration (M002/S06).
#[derive(Debug, Clone)]
pub struct SharedContextConfig {
    pub ttl_ms: u64,
    pub max_entries: usize,
}

impl Default for SharedContextConfig {
    fn default() -> Self {
        Self {
            ttl_ms: 86_400_000,
            max_entries: 100,
        }
    }
}

/// Supervisor orchestration settings (M002/S07).
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Enable the supervisor task.
    pub enabled: bool,
    /// Optional model override for future model-backed supervisor decisions.
    pub model: Option<String>,
    /// Minimum milliseconds between steers sent to the same issue.
    pub steer_cooldown_ms: u64,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            steer_cooldown_ms: 120_000,
        }
    }
}

/// Runtime supervisor status for snapshot consumers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorStatus {
    #[default]
    Disabled,
    Starting,
    Active,
    Stopped,
    Failed,
}

/// Read-only supervisor metrics included in orchestrator snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorSnapshot {
    pub status: SupervisorStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub steers_issued: u64,
    #[serde(default)]
    pub conflicts_detected: u64,
    #[serde(default)]
    pub patterns_detected: u64,
    #[serde(default)]
    pub escalations_created: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_decision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_action_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Default for SupervisorSnapshot {
    fn default() -> Self {
        Self::disabled(None)
    }
}

impl SupervisorSnapshot {
    pub fn disabled(model: Option<String>) -> Self {
        Self {
            status: SupervisorStatus::Disabled,
            model,
            steers_issued: 0,
            conflicts_detected: 0,
            patterns_detected: 0,
            escalations_created: 0,
            last_decision: None,
            last_action_at: None,
            last_error: None,
        }
    }

    pub fn idle(model: Option<String>) -> Self {
        Self {
            status: SupervisorStatus::Stopped,
            model,
            steers_issued: 0,
            conflicts_detected: 0,
            patterns_detected: 0,
            escalations_created: 0,
            last_decision: None,
            last_action_at: None,
            last_error: None,
        }
    }
}

/// Workspace configuration (spec §5.3.3).
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub root: String,
    pub repo: Option<String>,
    pub strategy: WorkspaceRepoStrategy,
    pub isolation: WorkspaceIsolation,
    pub docker: Option<DockerConfig>,
    pub branch_prefix: String,
    pub clone_branch: Option<String>,
    pub base_branch: Option<String>,
    pub cleanup_on_done: bool,
}

/// Workspace repository bootstrap strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRepoStrategy {
    CloneLocal,
    CloneRemote,
    Worktree,
    Auto,
}

/// Workspace runtime isolation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceIsolation {
    Local,
    Docker,
}

/// Codex authentication mode for Docker containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockerCodexAuth {
    /// OPENAI_API_KEY env var if set, else mount auth.json, else error.
    Auto,
    /// Force bind-mount of ~/.codex/auth.json (local only).
    Mount,
    /// Force OPENAI_API_KEY env var only (cloud deployments).
    Env,
}

/// Docker-specific workspace configuration.
#[derive(Debug, Clone)]
pub struct DockerConfig {
    /// Docker image name (e.g. "symphony-worker:latest").
    pub image: String,
    /// Optional setup script path, cached as derived image layer.
    pub setup: Option<String>,
    /// How to authenticate Codex inside the container.
    pub codex_auth: DockerCodexAuth,
    /// Additional environment variables passed to the container.
    pub env: Vec<String>,
    /// Additional volume mounts (e.g. "~/.ssh:/home/node/.ssh:ro").
    pub volumes: Vec<String>,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            image: "symphony-worker:latest".to_string(),
            setup: None,
            codex_auth: DockerCodexAuth::Auto,
            env: vec![],
            volumes: vec![],
        }
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            root: default_workspace_root(),
            repo: None,
            strategy: WorkspaceRepoStrategy::Auto,
            isolation: WorkspaceIsolation::Local,
            docker: None,
            branch_prefix: "symphony".to_string(),
            clone_branch: None,
            base_branch: Some("main".to_string()),
            cleanup_on_done: false,
        }
    }
}

fn default_workspace_root() -> String {
    let tmp = std::env::temp_dir();
    tmp.join("symphony_workspaces")
        .to_string_lossy()
        .to_string()
}

/// Worker configuration (SSH extension, spec §5.3 + Appendix A).
#[derive(Debug, Clone, Default)]
pub struct WorkerConfig {
    pub ssh_hosts: Vec<String>,
    pub max_concurrent_agents_per_host: Option<u32>,
}

/// Agent configuration (spec §5.3.5).
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_concurrent_agents: u32,
    pub max_turns: u32,
    pub max_retry_backoff_ms: u64,
    pub escalation_timeout_ms: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_concurrent_agents: 10,
            max_turns: 20,
            max_retry_backoff_ms: 300_000,
            escalation_timeout_ms: 300_000,
        }
    }
}

/// Codex app-server configuration (spec §5.3.6).
#[derive(Debug, Clone)]
pub struct CodexConfig {
    /// Command and arguments used to launch the Codex app-server process.
    /// Stored as a list to enable exec-without-shell semantics (spec §5.3.6).
    /// The YAML field accepts either a string (`"codex app-server"`, split on
    /// whitespace) or an explicit list (`["codex", "app-server"]`).
    pub command: Vec<String>,
    pub approval_policy: serde_json::Value,
    pub thread_sandbox: String,
    pub turn_sandbox_policy: Option<serde_json::Value>,
    pub turn_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub stall_timeout_ms: u64,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            command: vec!["codex".to_string(), "app-server".to_string()],
            approval_policy: serde_json::json!({
                "reject": {
                    "sandbox_approval": true,
                    "rules": true,
                    "mcp_elicitations": true
                }
            }),
            thread_sandbox: "workspace-write".to_string(),
            turn_sandbox_policy: None,
            turn_timeout_ms: 3_600_000,
            read_timeout_ms: 5_000,
            stall_timeout_ms: 300_000,
        }
    }
}

/// Which worker runtime to use for agent sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentBackend {
    #[default]
    KataCli,
    Codex,
}

/// Pi runtime configuration.
#[derive(Debug, Clone)]
pub struct PiAgentConfig {
    /// Command and arguments to launch the Pi runtime.
    pub command: Vec<String>,
    /// Optional default model identifier passed via `--model`.
    pub model: Option<String>,
    /// Optional per-Linear-label model overrides (lowercased label names).
    pub model_by_label: HashMap<String, String>,
    /// Optional per-Linear-state model overrides (lowercased state names).
    pub model_by_state: HashMap<String, String>,
    /// Whether to pass `--no-session`.
    pub no_session: bool,
    /// Optional file path passed via `--append-system-prompt`.
    pub append_system_prompt: Option<String>,
    /// Timeout for stdout reads.
    pub read_timeout_ms: u64,
    /// Timeout for stalled sessions (no activity).
    pub stall_timeout_ms: u64,
}

impl Default for PiAgentConfig {
    fn default() -> Self {
        Self {
            command: vec!["pi".to_string(), "--mode".to_string(), "rpc".to_string()],
            model: None,
            model_by_label: HashMap::new(),
            model_by_state: HashMap::new(),
            no_session: true,
            append_system_prompt: None,
            read_timeout_ms: 5_000,
            stall_timeout_ms: 300_000,
        }
    }
}

impl PiAgentConfig {
    /// Resolve the effective model for a tracker issue state.
    ///
    /// Looks up a lowercase/trimmed state key in `model_by_state` first,
    /// then falls back to the default `model`.
    pub fn model_for_state(&self, issue_state: &str) -> Option<String> {
        let state_key = issue_state.trim().to_lowercase();
        self.model_by_state
            .get(&state_key)
            .cloned()
            .or_else(|| self.model.clone())
    }
}

/// Hooks configuration (spec §5.3.4).
#[derive(Debug, Clone)]
pub struct HooksConfig {
    pub after_create: Option<String>,
    pub before_run: Option<String>,
    pub after_run: Option<String>,
    pub before_remove: Option<String>,
    pub timeout_ms: u64,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            after_create: None,
            before_run: None,
            after_run: None,
            before_remove: None,
            timeout_ms: 60_000,
        }
    }
}

/// Server configuration (extension, spec §13.7).
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: Option<u16>,
    pub host: String,
    pub public_url: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: None,
            host: "127.0.0.1".to_string(),
            public_url: None,
        }
    }
}

// ── Workspace (spec §4.1.4) ───────────────────────────────────────────

/// A prepared workspace directory for an issue run.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: String,
    pub workspace_key: String,
    pub created_now: bool,
}

// ── RunAttempt (spec §4.1.5) ──────────────────────────────────────────

/// A single execution attempt for an issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAttempt {
    pub issue_id: String,
    pub issue_identifier: String,
    #[serde(default)]
    pub issue_title: Option<String>,
    /// `None` for first attempt, `Some(n)` for retries.
    #[serde(default)]
    pub attempt: Option<u32>,
    pub workspace_path: String,
    pub started_at: DateTime<Utc>,
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub worker_host: Option<String>,
    /// Effective model selected for this run attempt (backend-dependent).
    #[serde(default)]
    pub model: Option<String>,
    /// Tracker issue state at dispatch time (e.g. "In Progress", "Agent Review").
    #[serde(default)]
    pub tracker_state: Option<String>,
    /// Tracker issue URL for notifications.
    #[serde(default)]
    pub issue_url: Option<String>,
}

/// Per-session token usage scoped to a single running worker session.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SessionTokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

/// Live worker-session diagnostics for dashboard rendering.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WorkerSessionInfo {
    #[serde(default)]
    pub turn_count: u32,
    #[serde(default)]
    pub max_turns: u32,
    #[serde(skip)]
    pub stall_timeout_ms: i64,
    #[serde(default)]
    pub last_activity_ms: Option<i64>,
    #[serde(default)]
    pub session_tokens: SessionTokenUsage,
    /// Name of the tool currently executing (set on tool_start, cleared on tool_end).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tool_name: Option<String>,
    /// Short preview of the arguments for the currently executing tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tool_args_preview: Option<String>,
    /// Last worker error surfaced for operator visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

// ── LiveSession (spec §4.1.6) ─────────────────────────────────────────

/// Tracks the active Codex session for a running issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveSession {
    pub session_id: String,
    pub thread_id: String,
    pub turn_id: String,
    #[serde(default)]
    pub codex_app_server_pid: Option<String>,
    #[serde(default)]
    pub last_codex_event: Option<String>,
    #[serde(default)]
    pub last_codex_timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_codex_message: Option<String>,
    #[serde(default)]
    pub codex_input_tokens: u64,
    #[serde(default)]
    pub codex_output_tokens: u64,
    #[serde(default)]
    pub codex_total_tokens: u64,
    #[serde(default)]
    pub last_reported_input_tokens: u64,
    #[serde(default)]
    pub last_reported_output_tokens: u64,
    #[serde(default)]
    pub last_reported_total_tokens: u64,
    #[serde(default)]
    pub turn_count: u32,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub worker_host: Option<String>,
}

// ── RetryEntry (spec §4.1.7) ──────────────────────────────────────────

/// An issue queued for retry after a failed attempt.
#[derive(Debug, Clone)]
pub struct RetryEntry {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    /// Monotonic millisecond timestamp when retry is due.
    pub due_at_ms: i64,
    /// Opaque handle — concrete type wired in S06.
    pub timer_handle: Option<String>,
    pub error: Option<String>,
    pub worker_host: Option<String>,
    pub workspace_path: Option<String>,
}

// ── CodexTotals ───────────────────────────────────────────────────────

/// Aggregate token and time accounting across all agent sessions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodexTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub event_count: u64,
    pub seconds_running: f64,
}

// ── RateLimitInfo ─────────────────────────────────────────────────────

/// Opaque rate-limit snapshot captured from agent events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    pub data: serde_json::Value,
}

// ── OrchestratorState (spec §4.1.8) ───────────────────────────────────

/// Mutable runtime state of the orchestrator loop.
#[derive(Debug, Clone)]
pub struct OrchestratorState {
    pub poll_interval_ms: u64,
    pub max_concurrent_agents: u32,
    pub running: HashMap<String, RunAttempt>,
    pub claimed: HashSet<String>,
    pub retry_attempts: HashMap<String, RetryEntry>,
    pub completed: HashMap<String, CompletedEntry>,
    pub codex_totals: CodexTotals,
    pub codex_rate_limits: Option<RateLimitInfo>,
}

// ── OrchestratorSnapshot (S06→S07 boundary) ───────────────────────────

/// A single retry entry in the snapshot (sorted, serializable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrySnapshotEntry {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    pub due_in_ms: i64,
    pub error: Option<String>,
    pub worker_host: Option<String>,
    pub workspace_path: Option<String>,
}

/// A completed issue entry with human-readable metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedEntry {
    pub issue_id: String,
    pub identifier: String,
    pub title: String,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Session-level metrics for a currently running issue.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunningSessionSnapshot {
    #[serde(default)]
    pub turn_count: u32,
    #[serde(default)]
    pub last_activity_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub last_event: Option<String>,
    #[serde(default)]
    pub last_event_message: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Name of the tool currently executing (set on tool_start, cleared on tool_end).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tool_name: Option<String>,
    /// Short preview of the arguments for the currently executing tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tool_args_preview: Option<String>,
    /// Last worker error mirrored from `WorkerSessionInfo` for TUI rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Polling state for the snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollingSnapshot {
    pub checking: bool,
    pub next_poll_in_ms: i64,
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub last_poll_at: Option<String>,
    #[serde(default)]
    pub poll_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SharedContextSummary {
    #[serde(default)]
    pub total_entries: usize,
    #[serde(default)]
    pub entries_by_scope: BTreeMap<String, usize>,
    #[serde(default)]
    pub oldest_entry_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub newest_entry_at: Option<DateTime<Utc>>,
}

/// Live view of one running triage attempt, surfaced to TUI/API consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageSessionInfo {
    pub issue_identifier: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub attempt: u32,
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub last_activity_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_event: Option<String>,
    #[serde(default)]
    pub last_event_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tool_args_preview: Option<String>,
    #[serde(default)]
    pub total_tokens: u64,
}

/// Shared registry of in-flight triage attempts (orchestrator ↔ coordinator).
#[derive(Default)]
pub struct TriageSessionRegistry {
    sessions: std::collections::BTreeMap<String, TriageSessionInfo>,
    /// Called after begin/update/finish so snapshot consumers see live state.
    on_change: Option<std::sync::Arc<dyn Fn(Vec<TriageSessionInfo>) + Send + Sync>>,
}

impl std::fmt::Debug for TriageSessionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriageSessionRegistry")
            .field("sessions", &self.sessions)
            .field("on_change", &self.on_change.as_ref().map(|_| "set"))
            .finish()
    }
}

impl TriageSessionRegistry {
    pub fn set_on_change(
        &mut self,
        callback: std::sync::Arc<dyn Fn(Vec<TriageSessionInfo>) + Send + Sync>,
    ) {
        self.on_change = Some(callback);
    }

    pub fn begin(&mut self, info: TriageSessionInfo) {
        self.sessions.insert(info.stage_run_id.clone(), info);
        self.notify();
    }

    pub fn update(&mut self, stage_run_id: &str, update: impl FnOnce(&mut TriageSessionInfo)) {
        if let Some(info) = self.sessions.get_mut(stage_run_id) {
            update(info);
            info.last_activity_at = Some(Utc::now());
            self.notify();
        }
    }

    pub fn finish(&mut self, stage_run_id: &str) {
        self.sessions.remove(stage_run_id);
        self.notify();
    }

    pub fn list(&self) -> Vec<TriageSessionInfo> {
        self.sessions.values().cloned().collect()
    }

    fn notify(&self) {
        if let Some(callback) = &self.on_change {
            callback(self.list());
        }
    }
}

/// Read-only serializable view of orchestrator state for the HTTP API.
/// Uses `BTreeMap` for deterministic JSON key ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorSnapshot {
    pub poll_interval_ms: u64,
    pub max_concurrent_agents: u32,
    #[serde(default)]
    pub tracker_project_url: Option<String>,
    pub running: BTreeMap<String, RunAttempt>,
    #[serde(default)]
    pub running_sessions: BTreeMap<String, RunningSessionSnapshot>,
    #[serde(default)]
    pub running_session_info: BTreeMap<String, WorkerSessionInfo>,
    pub claimed: BTreeSet<String>,
    pub retry_queue: Vec<RetrySnapshotEntry>,
    pub completed: Vec<CompletedEntry>,
    #[serde(default)]
    pub blocked: Vec<BlockedIssueEntry>,
    #[serde(default)]
    pub pending_escalations: Vec<PendingEscalation>,
    #[serde(default)]
    pub shared_context: SharedContextSummary,
    #[serde(default)]
    pub supervisor: SupervisorSnapshot,
    pub codex_totals: CodexTotals,
    pub codex_rate_limits: Option<RateLimitInfo>,
    pub polling: PollingSnapshot,
    #[serde(default)]
    pub triage_sessions: Vec<TriageSessionInfo>,
}

// ── RefreshRequestOutcome (S07 HTTP control seam) ─────────────────────

/// Outcome of a refresh request from the HTTP control surface.
/// Used by both the orchestrator's refresh channel and the HTTP API response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RefreshRequestOutcome {
    /// Whether this request was queued for processing (first request).
    pub queued: bool,
    /// Whether this request was coalesced with an already-pending request.
    pub coalesced: bool,
    /// Number of pending refresh requests (always 0 or 1 due to coalescing).
    pub pending_requests: u64,
}

// ── AgentEvent (spec §10.4) ───────────────────────────────────────────

/// Events emitted by a Codex agent session, parsed from the event stream.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    SessionStarted {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        session_id: String,
    },
    StartupFailed {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        error: String,
    },
    TurnCompleted {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        turn_id: String,
        message: Option<String>,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        rate_limits: Option<serde_json::Value>,
    },
    TurnFailed {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        turn_id: String,
        error: String,
    },
    TurnCancelled {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        turn_id: String,
    },
    TurnEndedWithError {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        turn_id: String,
        error: String,
    },
    TurnInputRequired {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        turn_id: String,
        prompt: Option<String>,
    },
    ApprovalAutoApproved {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        tool_call: String,
    },
    ApprovalRequired {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        method: String,
        payload: serde_json::Value,
    },
    ToolCallCompleted {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        tool_name: String,
    },
    ToolCallFailed {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        tool_name: Option<String>,
    },
    ToolInputAutoAnswered {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
    },
    UnsupportedToolCall {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        tool_name: String,
    },
    EscalationCreated {
        timestamp: DateTime<Utc>,
        issue_id: String,
        issue_identifier: String,
        request: EscalationRequest,
    },
    EscalationResponded {
        timestamp: DateTime<Utc>,
        issue_id: String,
        issue_identifier: String,
        request_id: String,
        responder_id: Option<String>,
        latency_ms: u64,
    },
    EscalationTimedOut {
        timestamp: DateTime<Utc>,
        issue_id: String,
        issue_identifier: String,
        request_id: String,
        timeout_ms: u64,
    },
    EscalationCancelled {
        timestamp: DateTime<Utc>,
        issue_id: String,
        issue_identifier: String,
        request_id: String,
        reason: String,
    },
    Notification {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        message: String,
    },
    OtherMessage {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        raw: serde_json::Value,
    },
    Malformed {
        timestamp: DateTime<Utc>,
        codex_app_server_pid: Option<String>,
        raw_text: String,
        parse_error: String,
    },
}
