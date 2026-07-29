// Config layer — typed extraction, env resolution, tilde expansion, and validation.
//
// Implements spec §5.3 defaults, $VAR env indirection, ~ home expansion,
// nil-dropping normalization, and field validation.
//
// SECURITY: api_key values must never be emitted via tracing.

use std::collections::HashMap;

use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use crate::domain::{
    canonical_kata_phase_name, AgentBackend, AgentConfig, ApiKey, CodexConfig, DockerCodexAuth,
    DockerConfig, GithubProjectOwnerType, HooksConfig, NotificationsConfig, PiAgentConfig,
    PollingConfig, PromptsConfig, ServerConfig, ServiceConfig, SharedContextConfig, SlackConfig,
    SupervisorConfig, TrackerConfig, WorkerConfig, WorkspaceConfig, WorkspaceIsolation,
    WorkspaceRepoStrategy, KATA_PHASE_NAMES,
};
use crate::error::{Result, SymphonyError};
use crate::github::auth::{github_token_missing_message, resolve_github_token};
use crate::notifications;
use crate::repo_url::repo_is_remote;
use crate::spec::domain::{SpecApprovalRoute, SpecConfig, SpecDecisionLabels, SpecPromptsConfig};
use crate::implementation::domain::{
    ImplementationCompletionRoute, ImplementationConfig, ImplementationMode,
    ImplementationValidationCommand, IMPLEMENTATION_MAX_BUNDLE_BYTES_HARD_CAP,
    IMPLEMENTATION_VALIDATION_COMMAND_MAX_BYTES, IMPLEMENTATION_VALIDATION_MAX_COMMANDS,
    IMPLEMENTATION_VALIDATION_NAME_MAX_BYTES,
};
use crate::triage::domain::{
    RouteMapping, StorageConfig, TriageConfig, TriageMode, TriageRoutesConfig,
};

// ── Key normalization and null-dropping ───────────────────────────────────────

/// Recursively walk a YAML `Value`:
/// - coerce all mapping keys to `Value::String`
/// - drop mapping entries whose value is `Value::Null`
///
/// Note: only `agent.model_by_state` / legacy `pi_agent.model_by_state` and
/// `agent.model_by_label` / legacy `pi_agent.model_by_label`
/// map keys are lowercased (done separately in `from_workflow`).
/// General key casing is NOT changed.
fn normalize_keys(val: Value) -> Value {
    match val {
        Value::Mapping(map) => {
            let mut new_map = Mapping::new();
            for (k, v) in map {
                if v.is_null() {
                    continue; // drop null entries
                }
                let key_str = key_to_string(k);
                new_map.insert(Value::String(key_str), normalize_keys(v));
            }
            Value::Mapping(new_map)
        }
        Value::Sequence(seq) => Value::Sequence(seq.into_iter().map(normalize_keys).collect()),
        other => other,
    }
}

fn key_to_string(k: Value) -> String {
    match k {
        Value::String(s) => s,
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => "null".to_string(),
        // Sequence/Mapping keys are unusual; log a warning and fall back to
        // debug representation so the entry is preserved rather than silently
        // dropped, but the operator is notified.
        other => {
            tracing::warn!(
                key = ?other,
                "mapping key is not a scalar type — using debug representation; \
                 config sections with complex keys may not be extracted correctly"
            );
            format!("{other:?}")
        }
    }
}

// ── Env-var resolution ────────────────────────────────────────────────────────

/// Returns the bare variable name if `val` is a `$IDENTIFIER` reference
/// (starts with `$`, no `/`, spaces, or `:` — guards against partial paths
/// like `$HOME/x`).  Returns `None` for all other inputs.
fn env_reference_name(val: &str) -> Option<&str> {
    let var_name = val.strip_prefix('$')?;
    let is_bare_identifier = !var_name.is_empty()
        && !var_name.contains('/')
        && !var_name.contains(' ')
        && !var_name.contains(':');
    if is_bare_identifier {
        Some(var_name)
    } else {
        None
    }
}

/// Resolve a `$VAR` reference.
///
/// If `val` starts with `$` and the remainder is a non-empty identifier
/// (no `/`, spaces, or `:` — guards against partial paths like `$HOME/x`),
/// look up the env var.  Return the value if set and non-empty; otherwise
/// return an empty string.  For all other inputs return `val` unchanged.
///
/// Logs a warning when a `$VAR` reference is provided but the env var is not
/// set or is empty, giving operators a clear signal for the root cause of
/// subsequent validation failures.
fn resolve_env(val: &str) -> String {
    if let Some(var_name) = env_reference_name(val) {
        return match std::env::var(var_name) {
            Ok(v) if !v.is_empty() => v,
            Ok(_) => {
                tracing::warn!(
                    var = var_name,
                    "env var referenced in config is set but empty"
                );
                String::new()
            }
            Err(_) => {
                tracing::warn!(var = var_name, "env var referenced in config is not set");
                String::new()
            }
        };
    }
    val.to_string()
}

fn validate_public_url(value: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(value).map_err(|err| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "server.public_url must be a valid absolute URL: {err}"
        ))
    })?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "server.public_url must use http or https".to_string(),
        ));
    }

    if parsed.host_str().is_none() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "server.public_url must include a host".to_string(),
        ));
    }

    Ok(value.to_string())
}

// ── Tilde expansion ───────────────────────────────────────────────────────────

/// Expand a leading `~` to `$HOME`.
///
/// `"~"` → `$HOME`; `"~/foo"` → `"$HOME/foo"`; anything else is returned
/// unchanged.
fn expand_tilde_with_home(val: &str, home: Option<&str>) -> String {
    if val == "~" || val.starts_with("~/") {
        match home {
            Some(home) if !home.is_empty() => {
                if val == "~" {
                    home.to_string()
                } else {
                    format!("{}{}", home, &val[1..])
                }
            }
            _ => val.to_string(),
        }
    } else {
        val.to_string()
    }
}

fn expand_tilde(val: &str) -> String {
    if val == "~" || val.starts_with("~/") {
        let home = std::env::var("HOME").ok();
        let home_ref = home.as_deref().filter(|h| !h.is_empty());
        if home_ref.is_none() {
            tracing::warn!(
                raw_path = val,
                "HOME is not set or empty; tilde in workspace.root will not be expanded \
                 — workspace path may be relative or invalid"
            );
        }
        expand_tilde_with_home(val, home_ref)
    } else {
        val.to_string()
    }
}

// ── Intermediate serde structs ────────────────────────────────────────────────
//
// All fields are Option<T> so that missing YAML keys produce None (not an
// error), letting callers substitute domain defaults.

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawTrackerConfig {
    kind: Option<String>,
    endpoint: Option<String>,
    api_key: Option<String>,
    project_slug: Option<String>,
    workspace_slug: Option<String>,
    repo_owner: Option<String>,
    repo_name: Option<String>,
    github_project_owner_type: Option<String>,
    github_project_number: Option<u64>,
    label_prefix: Option<String>,
    assignee: Option<String>,
    active_states: Option<Vec<String>>,
    terminal_states: Option<Vec<String>>,
    exclude_labels: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawPollingConfig {
    interval_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawWorkspaceConfig {
    root: Option<String>,
    repo: Option<String>,
    strategy: Option<String>,
    git_strategy: Option<String>,
    isolation: Option<String>,
    docker: Option<RawDockerConfig>,
    branch_prefix: Option<String>,
    clone_branch: Option<String>,
    base_branch: Option<String>,
    cleanup_on_done: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawDockerConfig {
    image: Option<String>,
    setup: Option<String>,
    codex_auth: Option<String>,
    env: Option<Vec<String>>,
    volumes: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawWorkerConfig {
    ssh_hosts: Option<Vec<String>>,
    max_concurrent_agents_per_host: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAgentConfig {
    name: Option<String>,
    command: Option<Value>,
    backend: Option<String>,
    max_concurrent_agents: Option<u32>,
    max_turns: Option<u32>,
    max_retry_backoff_ms: Option<u64>,
    escalation_timeout_ms: Option<u64>,
    model: Option<String>,
    model_by_label: Option<HashMap<String, String>>,
    model_by_state: Option<HashMap<String, String>>,
    no_session: Option<bool>,
    append_system_prompt: Option<String>,
    read_timeout_ms: Option<u64>,
    stall_timeout_ms: Option<u64>,
    approval_policy: Option<Value>,
    thread_sandbox: Option<String>,
    turn_sandbox_policy: Option<Value>,
    turn_timeout_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawEscalationConfig {
    timeout_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawCodexConfig {
    /// Accepts a YAML string (`"codex app-server"`) or a list
    /// (`["codex", "app-server"]`).  Deserialized as a raw Value so we can
    /// support both forms in `parse_codex_command`.
    command: Option<Value>,
    approval_policy: Option<Value>,
    thread_sandbox: Option<String>,
    turn_sandbox_policy: Option<Value>,
    turn_timeout_ms: Option<u64>,
    read_timeout_ms: Option<u64>,
    stall_timeout_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawPiAgentConfig {
    command: Option<Value>,
    model: Option<String>,
    model_by_label: Option<HashMap<String, String>>,
    model_by_state: Option<HashMap<String, String>>,
    no_session: Option<bool>,
    append_system_prompt: Option<String>,
    read_timeout_ms: Option<u64>,
    stall_timeout_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawHooksConfig {
    after_create: Option<String>,
    before_run: Option<String>,
    after_run: Option<String>,
    before_remove: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawServerConfig {
    port: Option<u16>,
    host: Option<String>,
    public_url: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawPromptsConfig {
    system: Option<String>,
    repo: Option<String>,
    /// Legacy single-file preamble. Superseded by `system` + `repo`.
    shared: Option<String>,
    by_state: Option<HashMap<String, String>>,
    default: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawNotificationsConfig {
    slack: Option<RawSlackConfig>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSharedContextConfig {
    ttl_ms: Option<u64>,
    max_entries: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSupervisorConfig {
    enabled: Option<bool>,
    model: Option<String>,
    steer_cooldown_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSlackConfig {
    webhook_url: Option<String>,
    events: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawStorageConfig {
    path: Option<String>,
    busy_timeout_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawRouteMapping {
    label: Option<String>,
    state: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawTriageRoutes {
    implement: Option<RawRouteMapping>,
    spec: Option<RawRouteMapping>,
    needs_information: Option<RawRouteMapping>,
    park: Option<RawRouteMapping>,
    human_owned: Option<RawRouteMapping>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawTriageConfig {
    enabled: Option<bool>,
    mode: Option<String>,
    intake_label: Option<String>,
    prompt: Option<String>,
    model: Option<String>,
    turn_timeout_ms: Option<u64>,
    max_attempts: Option<u32>,
    max_intake_pages: Option<u32>,
    routes: Option<RawTriageRoutes>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSpecPrompts {
    draft: Option<String>,
    review: Option<String>,
    revise: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSpecLabels {
    approved: Option<String>,
    revise: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSpecConfig {
    enabled: Option<bool>,
    intake_label: Option<String>,
    prompts: Option<RawSpecPrompts>,
    model: Option<String>,
    review_model: Option<String>,
    turn_timeout_ms: Option<u64>,
    max_intake_pages: Option<u32>,
    max_review_cycles: Option<u32>,
    max_attempts: Option<u32>,
    max_revision_requests: Option<u32>,
    labels: Option<RawSpecLabels>,
    approval_route: Option<RawRouteMapping>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawImplementationValidationCommand {
    name: Option<String>,
    command: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawImplementationCompletionRoute {
    state: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawImplementationConfig {
    enabled: Option<bool>,
    mode: Option<String>,
    prompt: Option<String>,
    repair_prompt: Option<String>,
    model: Option<String>,
    max_turns: Option<u32>,
    invocation_timeout_ms: Option<u64>,
    max_attempts: Option<u32>,
    max_validation_cycles: Option<u32>,
    max_bundle_bytes: Option<u64>,
    spec_file: Option<String>,
    validation: Option<Vec<RawImplementationValidationCommand>>,
    completion_route: Option<RawImplementationCompletionRoute>,
}

// ── Section extraction helper ─────────────────────────────────────────────────

fn extract_section<T>(normalized: &Value, section: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de> + Default,
{
    let section_val = normalized
        .get(section)
        .cloned()
        .unwrap_or(Value::Mapping(Mapping::new()));

    serde_yaml::from_value(section_val).map_err(|e| {
        SymphonyError::InvalidWorkflowConfig(format!("config section '{section}': {e}"))
    })
}

// ── YAML → JSON conversion ────────────────────────────────────────────────────

/// Convert a `serde_yaml::Value` to `serde_json::Value`.
///
/// Both serde representations share the same serde data model for the subset
/// of types used in config (maps, sequences, strings, numbers, booleans).
/// Returns an error for YAML-only types (tagged values, special floats, etc.)
/// that cannot be represented in JSON.
fn yaml_to_json(val: Value) -> Result<serde_json::Value> {
    serde_yaml::from_value(val).map_err(|e| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "codex policy field could not be converted to JSON: {e}"
        ))
    })
}

// ── command parsing ───────────────────────────────────────────────────────────

/// Parse `codex.command` from YAML.
///
/// Accepts either a whitespace-split string (`"codex app-server"`) or an
/// explicit list (`["codex", "app-server"]`).  Returns `InvalidWorkflowConfig`
/// for any other shape.
fn parse_command_value(val: Value, field_name: &str) -> Result<Vec<String>> {
    match val {
        Value::String(s) if s.is_empty() => Ok(vec![]),
        Value::String(s) => Ok(s.split_whitespace().map(|p| p.to_string()).collect()),
        Value::Sequence(seq) => seq
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                other => Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "{field_name} list elements must be strings, got: {other:?}"
                ))),
            })
            .collect(),
        other => Err(SymphonyError::InvalidWorkflowConfig(format!(
            "{field_name} must be a string or list of strings, got: {other:?}"
        ))),
    }
}

fn parse_codex_command(val: Value) -> Result<Vec<String>> {
    parse_command_value(val, "codex.command")
}

fn parse_agent_command(val: Value) -> Result<Vec<String>> {
    match val {
        Value::String(s) if s.is_empty() => Ok(vec![]),
        Value::String(s) => shell_words::split(&s).map_err(|err| {
            SymphonyError::InvalidWorkflowConfig(format!(
                "agent.command string could not be parsed: {err}"
            ))
        }),
        Value::Sequence(seq) => seq
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                other => Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "agent.command list elements must be strings, got: {other:?}"
                ))),
            })
            .collect(),
        other => Err(SymphonyError::InvalidWorkflowConfig(format!(
            "agent.command must be a string or list of strings, got: {other:?}"
        ))),
    }
}

fn parse_pi_agent_command(val: Value) -> Result<Vec<String>> {
    match val {
        Value::String(s) if s.is_empty() => Ok(vec![]),
        Value::String(s) => shell_words::split(&s).map_err(|err| {
            SymphonyError::InvalidWorkflowConfig(format!(
                "pi_agent.command string could not be parsed: {err}"
            ))
        }),
        Value::Sequence(seq) => seq
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                other => Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "pi_agent.command list elements must be strings, got: {other:?}"
                ))),
            })
            .collect(),
        other => Err(SymphonyError::InvalidWorkflowConfig(format!(
            "pi_agent.command must be a string or list of strings, got: {other:?}"
        ))),
    }
}

fn parse_agent_name(value: &str) -> Result<AgentBackend> {
    match value {
        "pi" => Ok(AgentBackend::KataCli),
        "codex" => Ok(AgentBackend::Codex),
        other => Err(SymphonyError::InvalidWorkflowConfig(format!(
            "agent.name must be 'pi' or 'codex' (got '{other}')"
        ))),
    }
}

fn parse_workspace_git_strategy(value: &str) -> Result<WorkspaceRepoStrategy> {
    match value {
        "clone-local" => Ok(WorkspaceRepoStrategy::CloneLocal),
        "clone-remote" => Ok(WorkspaceRepoStrategy::CloneRemote),
        "worktree" => Ok(WorkspaceRepoStrategy::Worktree),
        "auto" => Ok(WorkspaceRepoStrategy::Auto),
        other => Err(SymphonyError::InvalidWorkflowConfig(format!(
            "workspace.git_strategy must be 'clone-local', 'clone-remote', 'worktree', or 'auto' (got '{other}')"
        ))),
    }
}

fn parse_legacy_workspace_strategy(value: &str) -> Result<WorkspaceRepoStrategy> {
    match value {
        "clone" => Ok(WorkspaceRepoStrategy::Auto),
        "worktree" => Ok(WorkspaceRepoStrategy::Worktree),
        other => Err(SymphonyError::InvalidWorkflowConfig(format!(
            "workspace.strategy must be 'clone' or 'worktree' (got '{other}')"
        ))),
    }
}

fn parse_workspace_isolation(value: &str) -> Result<WorkspaceIsolation> {
    match value {
        "local" => Ok(WorkspaceIsolation::Local),
        "docker" => Ok(WorkspaceIsolation::Docker),
        other => Err(SymphonyError::InvalidWorkflowConfig(format!(
            "workspace.isolation must be 'local' or 'docker' (got '{other}')"
        ))),
    }
}

fn parse_docker_codex_auth(value: &str) -> Result<DockerCodexAuth> {
    match value {
        "auto" => Ok(DockerCodexAuth::Auto),
        "mount" => Ok(DockerCodexAuth::Mount),
        "env" => Ok(DockerCodexAuth::Env),
        other => Err(SymphonyError::InvalidWorkflowConfig(format!(
            "workspace.docker.codex_auth must be 'auto', 'mount', or 'env' (got '{other}')"
        ))),
    }
}

fn build_route_mapping(raw: Option<RawRouteMapping>, default: RouteMapping) -> RouteMapping {
    match raw {
        None => default,
        Some(raw) => {
            let label = raw
                .label
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or(default.label);
            let state = match raw.state {
                Some(value) => {
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                }
                None => default.state,
            };
            RouteMapping { label, state }
        }
    }
}

fn build_triage_routes(
    raw: Option<RawTriageRoutes>,
    defaults: TriageRoutesConfig,
) -> TriageRoutesConfig {
    let raw = raw.unwrap_or_default();
    TriageRoutesConfig {
        implement: build_route_mapping(raw.implement, defaults.implement),
        spec: build_route_mapping(raw.spec, defaults.spec),
        needs_information: build_route_mapping(raw.needs_information, defaults.needs_information),
        park: build_route_mapping(raw.park, defaults.park),
        human_owned: build_route_mapping(raw.human_owned, defaults.human_owned),
    }
}

// ── Validated config wrapper ──────────────────────────────────────────────────

/// A [`ServiceConfig`] that has passed [`validate`].
///
/// This newtype enforces at the type level that validation must occur before
/// dispatch.  The orchestrator's dispatch function should accept
/// `ValidatedServiceConfig`, making it impossible to accidentally dispatch
/// with an unvalidated config.
#[derive(Debug)]
pub struct ValidatedServiceConfig(ServiceConfig);

impl ValidatedServiceConfig {
    /// Borrow the underlying config.
    pub fn inner(&self) -> &ServiceConfig {
        &self.0
    }
    /// Consume and return the underlying config.
    pub fn into_inner(self) -> ServiceConfig {
        self.0
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Derive a typed [`ServiceConfig`] from a raw YAML front-matter map.
///
/// Applies spec §5.3 defaults, resolves `$ENV_VAR` references in string
/// fields, expands leading `~` in path fields, drops null entries, and
/// normalises map keys to strings.
///
/// # Errors
/// Returns [`SymphonyError::InvalidWorkflowConfig`] if a YAML section cannot
/// be deserialized into its target struct, or if a codex policy field cannot
/// be converted to JSON.
pub fn from_workflow(config: &Value) -> Result<ServiceConfig> {
    let normalized = normalize_keys(config.clone());

    // ── Deserialize each config section ──────────────────────────────────
    let raw_tracker: RawTrackerConfig = extract_section(&normalized, "tracker")?;
    let raw_polling: RawPollingConfig = extract_section(&normalized, "polling")?;
    let raw_workspace: RawWorkspaceConfig = extract_section(&normalized, "workspace")?;
    let raw_worker: RawWorkerConfig = extract_section(&normalized, "worker")?;
    let raw_agent: RawAgentConfig = extract_section(&normalized, "agent")?;
    let raw_escalation: RawEscalationConfig = extract_section(&normalized, "escalation")?;
    let raw_codex: RawCodexConfig = extract_section(&normalized, "codex")?;
    let raw_kata_agent: RawPiAgentConfig = extract_section(&normalized, "kata_agent")?;
    let raw_pi_agent: RawPiAgentConfig = extract_section(&normalized, "pi_agent")?;
    let raw_hooks: RawHooksConfig = extract_section(&normalized, "hooks")?;
    let raw_server: RawServerConfig = extract_section(&normalized, "server")?;
    let raw_prompts: RawPromptsConfig = extract_section(&normalized, "prompts")?;
    let raw_notifications: RawNotificationsConfig = extract_section(&normalized, "notifications")?;
    let raw_shared_context: RawSharedContextConfig =
        extract_section(&normalized, "shared_context")?;
    let raw_supervisor: RawSupervisorConfig = extract_section(&normalized, "supervisor")?;
    let raw_storage: RawStorageConfig = extract_section(&normalized, "storage")?;
    let raw_triage: RawTriageConfig = extract_section(&normalized, "triage")?;
    let raw_spec: RawSpecConfig = extract_section(&normalized, "spec")?;
    let raw_implementation: RawImplementationConfig = extract_section(&normalized, "implementation")?;

    let defaults = ServiceConfig::default();
    let has_kata_agent_section = normalized.get("kata_agent").is_some();
    let has_pi_agent_section = normalized.get("pi_agent").is_some();

    if has_kata_agent_section && has_pi_agent_section {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "config must set only one of 'kata_agent' or 'pi_agent'".to_string(),
        ));
    }

    // ── TrackerConfig ─────────────────────────────────────────────────────
    let tracker_kind = raw_tracker
        .kind
        .map(|v| resolve_env(&v))
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty());

    // Resolve $VAR references in api_key; on empty result try LINEAR_API_KEY
    // as canonical fallback (spec §5.3.1 note), but only for the Linear tracker.
    // NOTE: api_key is intentionally never passed to any tracing call.
    let api_key: Option<ApiKey> = raw_tracker
        .api_key
        .map(|v| {
            let explicit_env_ref = env_reference_name(&v).is_some();
            let resolved = resolve_env(&v);
            if explicit_env_ref
                && resolved.is_empty()
                && matches!(tracker_kind.as_deref(), Some("linear"))
            {
                // Try canonical fallback only for Linear when an explicit $VAR
                // reference was provided but resolved to nothing.
                std::env::var("LINEAR_API_KEY").unwrap_or_default()
            } else {
                resolved
            }
        })
        .filter(|v| !v.is_empty()) // treat empty string as absent
        .map(ApiKey::new);

    let assignee = raw_tracker
        .assignee
        .map(|v| resolve_env(&v))
        .filter(|v| !v.is_empty());

    let project_slug = raw_tracker
        .project_slug
        .map(|v| resolve_env(&v))
        .filter(|v| !v.is_empty());
    let workspace_slug = raw_tracker
        .workspace_slug
        .map(|v| resolve_env(&v))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let repo_owner = raw_tracker
        .repo_owner
        .map(|v| resolve_env(&v))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let repo_name = raw_tracker
        .repo_name
        .map(|v| resolve_env(&v))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let github_project_owner_type = raw_tracker
        .github_project_owner_type
        .map(|v| resolve_env(&v))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(|v| {
            GithubProjectOwnerType::parse(&v).ok_or_else(|| {
                SymphonyError::InvalidWorkflowConfig(
                    "tracker.github_project_owner_type must be either 'user' or 'org'".to_string(),
                )
            })
        })
        .transpose()?;
    let github_project_number = raw_tracker.github_project_number;

    let label_prefix = None;

    if matches!(tracker_kind.as_deref(), Some("github")) {
        if repo_owner.is_none() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "tracker.repo_owner is required when tracker.kind is github".to_string(),
            ));
        }
        if repo_name.is_none() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "tracker.repo_name is required when tracker.kind is github".to_string(),
            ));
        }
        if github_project_owner_type.is_none() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "tracker.github_project_owner_type is required when tracker.kind is github; use 'user' or 'org'".to_string(),
            ));
        }
        if github_project_number.is_none() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "tracker.github_project_number is required when tracker.kind is github; GitHub Projects v2 is the only supported GitHub state backend".to_string(),
            ));
        }
    }

    let endpoint_default = if matches!(tracker_kind.as_deref(), Some("github")) {
        "https://api.github.com".to_string()
    } else {
        defaults.tracker.endpoint.clone()
    };

    let tracker = TrackerConfig {
        kind: tracker_kind,
        endpoint: raw_tracker.endpoint.unwrap_or(endpoint_default),
        api_key,
        project_slug,
        workspace_slug,
        repo_owner,
        repo_name,
        github_project_owner_type,
        github_project_number,
        label_prefix,
        assignee,
        active_states: raw_tracker
            .active_states
            .unwrap_or(defaults.tracker.active_states.clone()),
        terminal_states: raw_tracker
            .terminal_states
            .unwrap_or(defaults.tracker.terminal_states.clone()),
        exclude_labels: raw_tracker
            .exclude_labels
            .unwrap_or(defaults.tracker.exclude_labels.clone()),
    };

    // ── PollingConfig ─────────────────────────────────────────────────────
    let polling = PollingConfig {
        interval_ms: raw_polling
            .interval_ms
            .unwrap_or(defaults.polling.interval_ms),
    };

    // ── WorkspaceConfig ───────────────────────────────────────────────────
    let raw_root = raw_workspace
        .root
        .unwrap_or_else(|| defaults.workspace.root.clone());
    let git_strategy = raw_workspace
        .git_strategy
        .map(|value| resolve_env(&value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let legacy_strategy = raw_workspace
        .strategy
        .map(|value| resolve_env(&value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let strategy = if let Some(value) = git_strategy.as_deref() {
        if legacy_strategy.is_some() {
            tracing::warn!(
                "workspace.strategy is deprecated and ignored because workspace.git_strategy is set"
            );
        }
        parse_workspace_git_strategy(value)?
    } else if let Some(value) = legacy_strategy.as_deref() {
        tracing::warn!("workspace.strategy is deprecated; use workspace.git_strategy");
        parse_legacy_workspace_strategy(value)?
    } else {
        WorkspaceRepoStrategy::Auto
    };
    let isolation = raw_workspace
        .isolation
        .map(|value| resolve_env(&value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .as_deref()
        .map(parse_workspace_isolation)
        .transpose()?
        .unwrap_or(WorkspaceIsolation::Local);
    let docker = if isolation == WorkspaceIsolation::Docker {
        let docker_defaults = DockerConfig::default();
        let raw_docker = raw_workspace.docker.as_ref();

        let image = raw_docker
            .and_then(|docker| docker.image.as_deref())
            .map(resolve_env)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(docker_defaults.image);
        let setup = raw_docker
            .and_then(|docker| docker.setup.as_deref())
            .map(resolve_env)
            .and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(expand_tilde(trimmed))
                }
            });
        let codex_auth = raw_docker
            .and_then(|docker| docker.codex_auth.as_deref())
            .map(resolve_env)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .as_deref()
            .map(parse_docker_codex_auth)
            .transpose()?
            .unwrap_or(docker_defaults.codex_auth);
        let env = raw_docker
            .and_then(|docker| docker.env.as_ref())
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| resolve_env(entry))
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let volumes = raw_docker
            .and_then(|docker| docker.volumes.as_ref())
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| resolve_env(entry))
                    .map(|entry| expand_tilde(entry.trim()))
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Some(DockerConfig {
            image,
            setup,
            codex_auth,
            env,
            volumes,
        })
    } else {
        None
    };
    let repo = raw_workspace.repo.and_then(|value| {
        let resolved = resolve_env(&value);
        let trimmed = resolved.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(expand_tilde(trimmed))
        }
    });
    let branch_prefix = raw_workspace
        .branch_prefix
        .map(|value| resolve_env(&value))
        .unwrap_or_else(|| defaults.workspace.branch_prefix.clone());
    let clone_branch = raw_workspace
        .clone_branch
        .map(|value| resolve_env(&value))
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
    let base_branch = raw_workspace
        .base_branch
        .map(|value| resolve_env(&value))
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .or_else(|| defaults.workspace.base_branch.clone());
    let workspace = WorkspaceConfig {
        root: expand_tilde(&raw_root),
        repo,
        strategy,
        isolation,
        docker,
        branch_prefix: branch_prefix.trim().to_string(),
        clone_branch,
        base_branch,
        cleanup_on_done: raw_workspace
            .cleanup_on_done
            .unwrap_or(defaults.workspace.cleanup_on_done),
    };

    // ── WorkerConfig ──────────────────────────────────────────────────────
    let worker = WorkerConfig {
        ssh_hosts: raw_worker.ssh_hosts.unwrap_or_default(),
        max_concurrent_agents_per_host: raw_worker.max_concurrent_agents_per_host,
    };

    if workspace.isolation == WorkspaceIsolation::Docker {
        if !worker.ssh_hosts.is_empty() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "worker.ssh_hosts is not supported with workspace.isolation 'docker'".to_string(),
            ));
        }

        let effective_strategy = match workspace.strategy {
            WorkspaceRepoStrategy::Auto => {
                workspace
                    .repo
                    .as_deref()
                    .map_or(WorkspaceRepoStrategy::Auto, |repo| {
                        if repo_is_remote(repo) {
                            WorkspaceRepoStrategy::CloneRemote
                        } else {
                            WorkspaceRepoStrategy::CloneLocal
                        }
                    })
            }
            other => other,
        };

        match effective_strategy {
            WorkspaceRepoStrategy::CloneRemote | WorkspaceRepoStrategy::Auto => {}
            WorkspaceRepoStrategy::CloneLocal => {
                return Err(SymphonyError::InvalidWorkflowConfig(
                    "workspace.git_strategy 'clone-local' is not supported with workspace.isolation 'docker'"
                        .to_string(),
                ));
            }
            WorkspaceRepoStrategy::Worktree => {
                return Err(SymphonyError::InvalidWorkflowConfig(
                    "workspace.git_strategy 'worktree' is not supported with workspace.isolation 'docker'"
                        .to_string(),
                ));
            }
        }
    }

    // ── AgentConfig ───────────────────────────────────────────────────────
    let agent = AgentConfig {
        max_concurrent_agents: raw_agent
            .max_concurrent_agents
            .unwrap_or(defaults.agent.max_concurrent_agents),
        max_turns: raw_agent.max_turns.unwrap_or(defaults.agent.max_turns),
        max_retry_backoff_ms: raw_agent
            .max_retry_backoff_ms
            .unwrap_or(defaults.agent.max_retry_backoff_ms),
        escalation_timeout_ms: raw_agent
            .escalation_timeout_ms
            .or(raw_escalation.timeout_ms)
            .unwrap_or(defaults.agent.escalation_timeout_ms),
    };

    // ── Agent runtime selection ─────────────────────────────────────────
    //
    // Public config uses `agent.name` plus `agent.command`.  The older
    // `agent.backend` key is accepted as a compatibility alias for the key
    // name only; values still must be real worker runtime names.
    let agent_backend = raw_agent
        .name
        .as_ref()
        .or(raw_agent.backend.as_ref())
        .map(|value| resolve_env(value))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .map(|value| parse_agent_name(&value))
        .transpose()?
        .unwrap_or(defaults.agent_backend);

    // ── CodexConfig ───────────────────────────────────────────────────────
    // approval_policy: propagate conversion errors rather than silently
    // substituting null, which could bypass configured safety constraints.
    let approval_policy = match raw_agent.approval_policy.or(raw_codex.approval_policy) {
        Some(v) => yaml_to_json(v)?,
        None => defaults.codex.approval_policy.clone(),
    };

    // turn_sandbox_policy: also propagate errors (Option<Result> → Result<Option>).
    let turn_sandbox_policy: Option<serde_json::Value> = raw_codex
        .turn_sandbox_policy
        .or(raw_agent.turn_sandbox_policy)
        .map(yaml_to_json)
        .transpose()?;

    let codex_command = if agent_backend == AgentBackend::Codex {
        match raw_agent.command.clone().or(raw_codex.command) {
            Some(val) if raw_agent.command.is_some() => parse_agent_command(val)?,
            Some(val) => parse_codex_command(val)?,
            None => defaults.codex.command.clone(),
        }
    } else {
        match raw_codex.command {
            Some(val) => parse_codex_command(val)?,
            None => defaults.codex.command.clone(),
        }
    };

    let codex = CodexConfig {
        command: codex_command,
        approval_policy,
        thread_sandbox: raw_agent
            .thread_sandbox
            .or(raw_codex.thread_sandbox)
            .unwrap_or(defaults.codex.thread_sandbox.clone()),
        turn_sandbox_policy,
        turn_timeout_ms: raw_agent
            .turn_timeout_ms
            .or(raw_codex.turn_timeout_ms)
            .unwrap_or(defaults.codex.turn_timeout_ms),
        read_timeout_ms: raw_agent
            .read_timeout_ms
            .or(raw_codex.read_timeout_ms)
            .unwrap_or(defaults.codex.read_timeout_ms),
        stall_timeout_ms: raw_agent
            .stall_timeout_ms
            .or(raw_codex.stall_timeout_ms)
            .unwrap_or(defaults.codex.stall_timeout_ms),
    };

    // ── Kata/Pi agent config ───────────────────────────────────────────
    let selected_agent_config = if has_kata_agent_section {
        raw_kata_agent
    } else {
        raw_pi_agent
    };

    let pi_agent_command = if agent_backend == AgentBackend::KataCli {
        match raw_agent.command.clone().or(selected_agent_config.command) {
            Some(val) if raw_agent.command.is_some() => parse_agent_command(val)?,
            Some(val) => parse_pi_agent_command(val)?,
            None => defaults.pi_agent.command.clone(),
        }
    } else {
        match selected_agent_config.command {
            Some(val) => parse_pi_agent_command(val)?,
            None => defaults.pi_agent.command.clone(),
        }
    };
    let pi_agent_model = raw_agent
        .model
        .or(selected_agent_config.model)
        .map(|value| resolve_env(&value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let pi_agent_model_by_label: HashMap<String, String> = raw_agent
        .model_by_label
        .or(selected_agent_config.model_by_label)
        .unwrap_or_default()
        .into_iter()
        .map(|(label, model)| {
            (
                label.trim().to_lowercase(),
                resolve_env(&model).trim().to_string(),
            )
        })
        .filter(|(label, model)| !label.is_empty() && !model.is_empty())
        .collect();
    let pi_agent_model_by_state: HashMap<String, String> = raw_agent
        .model_by_state
        .or(selected_agent_config.model_by_state)
        .unwrap_or_default()
        .into_iter()
        .map(|(state, model)| {
            (
                state.trim().to_lowercase(),
                resolve_env(&model).trim().to_string(),
            )
        })
        .filter(|(state, model)| !state.is_empty() && !model.is_empty())
        .collect();
    let pi_agent_append_system_prompt = raw_agent
        .append_system_prompt
        .or(selected_agent_config.append_system_prompt)
        .map(|value| resolve_env(&value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let pi_agent = PiAgentConfig {
        command: pi_agent_command,
        model: pi_agent_model,
        model_by_label: pi_agent_model_by_label,
        model_by_state: pi_agent_model_by_state,
        no_session: raw_agent
            .no_session
            .or(selected_agent_config.no_session)
            .unwrap_or(defaults.pi_agent.no_session),
        append_system_prompt: pi_agent_append_system_prompt,
        read_timeout_ms: raw_agent
            .read_timeout_ms
            .or(selected_agent_config.read_timeout_ms)
            .unwrap_or(defaults.pi_agent.read_timeout_ms),
        stall_timeout_ms: raw_agent
            .stall_timeout_ms
            .or(selected_agent_config.stall_timeout_ms)
            .unwrap_or(defaults.pi_agent.stall_timeout_ms),
    };

    // ── HooksConfig ───────────────────────────────────────────────────────
    let hooks = HooksConfig {
        after_create: raw_hooks.after_create,
        before_run: raw_hooks.before_run,
        after_run: raw_hooks.after_run,
        before_remove: raw_hooks.before_remove,
        timeout_ms: raw_hooks.timeout_ms.unwrap_or(defaults.hooks.timeout_ms),
    };

    // ── ServerConfig ──────────────────────────────────────────────────────
    let public_url = raw_server
        .public_url
        .map(|value| resolve_env(&value))
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .map(|value| validate_public_url(&value))
        .transpose()?;

    let server = ServerConfig {
        port: raw_server.port,
        host: raw_server.host.unwrap_or(defaults.server.host.clone()),
        public_url,
    };

    // ── PromptsConfig ─────────────────────────────────────────────────────
    let trim_path = |v: Option<String>| -> Option<String> {
        v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    let system = trim_path(raw_prompts.system);
    let repo = trim_path(raw_prompts.repo);
    let shared = trim_path(raw_prompts.shared);
    let default = trim_path(raw_prompts.default);
    let by_state: HashMap<String, String> = raw_prompts
        .by_state
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| {
            let key = k.trim().to_ascii_lowercase();
            let path = v.trim().to_string();
            (!key.is_empty() && !path.is_empty()).then_some((key, path))
        })
        .collect();
    let prompts = if system.is_some()
        || repo.is_some()
        || shared.is_some()
        || !by_state.is_empty()
        || default.is_some()
    {
        Some(PromptsConfig {
            system,
            repo,
            shared,
            by_state,
            default,
        })
    } else {
        None
    };

    // ── NotificationsConfig ───────────────────────────────────────────────
    let slack = match raw_notifications.slack {
        None => None,
        Some(raw) => {
            let webhook_url = raw
                .webhook_url
                .map(|value| resolve_env(&value))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    SymphonyError::InvalidWorkflowConfig(
                        "notifications.slack.webhook_url must be non-empty when notifications.slack is configured"
                            .to_string(),
                    )
                })?;

            let events = raw
                .events
                .unwrap_or_default()
                .into_iter()
                .map(|event| event.trim().to_ascii_lowercase())
                .filter(|event| !event.is_empty())
                .collect::<Vec<_>>();

            if events.is_empty() {
                tracing::warn!(
                    "notifications.slack.events is empty; no Slack notifications will be sent"
                );
            }

            for event in &events {
                if !notifications::is_supported_slack_event(event) {
                    return Err(SymphonyError::InvalidWorkflowConfig(format!(
                        "notifications.slack.events contains unsupported value '{event}'. Supported values: {}",
                        notifications::SUPPORTED_SLACK_EVENTS.join(", ")
                    )));
                }
            }

            Some(SlackConfig {
                webhook_url,
                events,
            })
        }
    };

    let notifications = slack.map(|slack| NotificationsConfig { slack: Some(slack) });

    let shared_context = SharedContextConfig {
        ttl_ms: raw_shared_context
            .ttl_ms
            .unwrap_or(defaults.shared_context.ttl_ms),
        max_entries: raw_shared_context
            .max_entries
            .unwrap_or(defaults.shared_context.max_entries),
    };

    let supervisor = SupervisorConfig {
        enabled: raw_supervisor
            .enabled
            .unwrap_or(defaults.supervisor.enabled),
        model: raw_supervisor
            .model
            .map(|value| resolve_env(&value))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        steer_cooldown_ms: raw_supervisor
            .steer_cooldown_ms
            .unwrap_or(defaults.supervisor.steer_cooldown_ms),
    };

    // ── StorageConfig ─────────────────────────────────────────────────────
    let storage_path = raw_storage
        .path
        .map(|value| resolve_env(&value))
        .map(|value| expand_tilde(value.trim()))
        .filter(|value| !value.is_empty());
    let storage = StorageConfig {
        path: storage_path,
        busy_timeout_ms: raw_storage
            .busy_timeout_ms
            .unwrap_or(defaults.storage.busy_timeout_ms),
    };

    // ── TriageConfig ──────────────────────────────────────────────────────
    let triage_mode = match raw_triage.mode {
        Some(value) => {
            let resolved = resolve_env(&value);
            let trimmed = resolved.trim();
            TriageMode::parse(trimmed).ok_or_else(|| {
                SymphonyError::InvalidWorkflowConfig(format!(
                    "triage.mode must be 'preview' or 'automatic' (got '{trimmed}')"
                ))
            })?
        }
        None => defaults.triage.mode,
    };
    let triage_intake_label = raw_triage
        .intake_label
        .map(|value| resolve_env(&value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| defaults.triage.intake_label.clone());
    let triage_prompt = raw_triage
        .prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| defaults.triage.prompt.clone());
    let triage_model = raw_triage
        .model
        .map(|value| resolve_env(&value))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let triage = TriageConfig {
        enabled: raw_triage.enabled.unwrap_or(defaults.triage.enabled),
        mode: triage_mode,
        intake_label: triage_intake_label,
        prompt: triage_prompt,
        model: triage_model,
        turn_timeout_ms: raw_triage
            .turn_timeout_ms
            .unwrap_or(defaults.triage.turn_timeout_ms),
        max_attempts: raw_triage
            .max_attempts
            .unwrap_or(defaults.triage.max_attempts),
        max_intake_pages: raw_triage
            .max_intake_pages
            .unwrap_or(defaults.triage.max_intake_pages),
        routes: build_triage_routes(raw_triage.routes, defaults.triage.routes.clone()),
    };

    // ── SpecConfig ────────────────────────────────────────────────────────
    let spec_prompts_raw = raw_spec.prompts.unwrap_or_default();
    let spec_labels_raw = raw_spec.labels.unwrap_or_default();
    let spec_approval_route = build_route_mapping(
        raw_spec.approval_route,
        RouteMapping {
            label: defaults.spec.approval_route.label.clone(),
            state: defaults.spec.approval_route.state.clone(),
        },
    );
    let config_string = |value: Option<String>, default: &str| {
        value
            .map(|value| resolve_env(&value))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| default.to_string())
    };
    let config_model = |value: Option<String>| {
        value
            .map(|value| resolve_env(&value))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let spec = SpecConfig {
        enabled: raw_spec.enabled.unwrap_or(defaults.spec.enabled),
        intake_label: config_string(raw_spec.intake_label, &defaults.spec.intake_label),
        prompts: SpecPromptsConfig {
            draft: config_string(spec_prompts_raw.draft, &defaults.spec.prompts.draft),
            review: config_string(spec_prompts_raw.review, &defaults.spec.prompts.review),
            revise: config_string(spec_prompts_raw.revise, &defaults.spec.prompts.revise),
        },
        model: config_model(raw_spec.model),
        review_model: config_model(raw_spec.review_model),
        turn_timeout_ms: raw_spec
            .turn_timeout_ms
            .unwrap_or(defaults.spec.turn_timeout_ms),
        max_intake_pages: raw_spec
            .max_intake_pages
            .unwrap_or(defaults.spec.max_intake_pages),
        max_review_cycles: raw_spec
            .max_review_cycles
            .unwrap_or(defaults.spec.max_review_cycles),
        max_attempts: raw_spec.max_attempts.unwrap_or(defaults.spec.max_attempts),
        max_revision_requests: raw_spec
            .max_revision_requests
            .unwrap_or(defaults.spec.max_revision_requests),
        labels: SpecDecisionLabels {
            approved: config_string(spec_labels_raw.approved, &defaults.spec.labels.approved),
            revise: config_string(spec_labels_raw.revise, &defaults.spec.labels.revise),
        },
        approval_route: SpecApprovalRoute {
            label: spec_approval_route.label,
            state: spec_approval_route.state,
        },
    };

    // ── ImplementationConfig ──────────────────────────────────────────────
    let impl_defaults = ImplementationConfig::default();
    let implementation_mode = match raw_implementation
        .mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => impl_defaults.mode,
        Some("preview") => ImplementationMode::Preview,
        Some("automatic") => ImplementationMode::Automatic,
        Some(other) => {
            return Err(SymphonyError::InvalidWorkflowConfig(format!(
                "implementation.mode must be 'preview' or 'automatic', got '{other}'"
            )));
        }
    };
    let validation = raw_implementation
        .validation
        .unwrap_or_default()
        .into_iter()
        .map(|command| {
            Ok(ImplementationValidationCommand {
                name: command
                    .name
                    .map(|value| resolve_env(&value))
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_default(),
                command: command
                    .command
                    .map(|value| resolve_env(&value))
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_default(),
                timeout_ms: command.timeout_ms.unwrap_or(1_800_000),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let completion_route = raw_implementation.completion_route.map(|route| {
        ImplementationCompletionRoute {
            state: route
                .state
                .map(|value| resolve_env(&value))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_default(),
        }
    });
    let implementation = ImplementationConfig {
        enabled: raw_implementation
            .enabled
            .unwrap_or(defaults.implementation.enabled),
        mode: implementation_mode,
        prompt: config_string(raw_implementation.prompt, &impl_defaults.prompt),
        repair_prompt: config_string(
            raw_implementation.repair_prompt,
            &impl_defaults.repair_prompt,
        ),
        model: config_model(raw_implementation.model),
        max_turns: raw_implementation
            .max_turns
            .unwrap_or(defaults.agent.max_turns.max(1)),
        invocation_timeout_ms: raw_implementation
            .invocation_timeout_ms
            .unwrap_or(impl_defaults.invocation_timeout_ms),
        max_attempts: raw_implementation
            .max_attempts
            .unwrap_or(impl_defaults.max_attempts),
        max_validation_cycles: raw_implementation
            .max_validation_cycles
            .unwrap_or(impl_defaults.max_validation_cycles),
        max_bundle_bytes: raw_implementation
            .max_bundle_bytes
            .unwrap_or(impl_defaults.max_bundle_bytes),
        spec_file: config_string(raw_implementation.spec_file, &impl_defaults.spec_file),
        validation,
        completion_route,
    };

    Ok(ServiceConfig {
        tracker,
        polling,
        workspace,
        worker,
        agent,
        codex,
        pi_agent,
        agent_backend,
        hooks,
        server,
        prompts,
        notifications,
        shared_context,
        supervisor,
        storage,
        triage,
        spec,
        implementation,
    })
}

/// Validate a [`ServiceConfig`] for required fields and acceptable values.
///
/// Checks spec §6.3 requirements:
/// - `tracker.kind` must be `"linear"`
/// - `tracker.api_key` must be present and non-empty
/// - `tracker.project_slug` must be present and non-empty
/// - `codex.command` must be non-empty
///
/// On success returns a [`ValidatedServiceConfig`] — a newtype wrapper that
/// signals at the type level that validation has occurred.  The orchestrator's
/// dispatch function should accept `ValidatedServiceConfig` to prevent
/// dispatching with an unvalidated config (see D017).
///
/// The api_key value is never included in failure messages.
pub fn validate(config: &ServiceConfig) -> Result<ValidatedServiceConfig> {
    let tracker_kind = match config.tracker.kind.as_deref() {
        Some("linear") => "linear",
        Some("github") => "github",
        Some(other) => {
            return Err(SymphonyError::UnsupportedTrackerKind(other.to_string()));
        }
        None => {
            return Err(SymphonyError::MissingTrackerKind);
        }
    };

    for (field, states) in [
        ("tracker.active_states", &config.tracker.active_states),
        ("tracker.terminal_states", &config.tracker.terminal_states),
    ] {
        for configured_state in states {
            if canonical_kata_phase_name(configured_state).is_none() {
                tracing::info!(
                    event = "symphony_state_vocabulary_check",
                    field,
                    configured_state = %configured_state,
                    canonical_phases = ?KATA_PHASE_NAMES,
                    "state name is non-canonical but allowed by config validation"
                );
            }
        }
    }

    if tracker_kind == "linear" {
        // tracker.api_key must be present and non-empty
        match config.tracker.api_key.as_deref() {
            Some(k) if !k.trim().is_empty() => {}
            _ => {
                return Err(SymphonyError::MissingLinearApiToken);
            }
        }

        // tracker.project_slug must be present and non-empty
        match config.tracker.project_slug.as_deref() {
            Some(slug) if !slug.is_empty() => {}
            _ => {
                return Err(SymphonyError::MissingLinearProjectSlug);
            }
        }
    }

    if tracker_kind == "github" {
        if resolve_github_token(&config.tracker).is_none() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                github_token_missing_message().to_string(),
            ));
        }

        if config
            .tracker
            .repo_owner
            .as_deref()
            .map(str::trim)
            .map(|owner| owner.is_empty())
            .unwrap_or(true)
        {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "tracker.repo_owner is required when tracker.kind is github".to_string(),
            ));
        }

        if config
            .tracker
            .repo_name
            .as_deref()
            .map(str::trim)
            .map(|repo| repo.is_empty())
            .unwrap_or(true)
        {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "tracker.repo_name is required when tracker.kind is github".to_string(),
            ));
        }

        if config.tracker.github_project_owner_type.is_none() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "tracker.github_project_owner_type is required when tracker.kind is github; use 'user' or 'org'".to_string(),
            ));
        }

        if config.tracker.github_project_number.is_none() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "tracker.github_project_number is required when tracker.kind is github; GitHub Projects v2 is the only supported GitHub state backend".to_string(),
            ));
        }

        if let Some(project_number) = config.tracker.github_project_number {
            tracing::debug!(project_number, "Projects v2 mode active");
        }
    }

    // backend-specific command requirements
    if config.agent_backend == AgentBackend::Codex && config.codex.command.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "agent.command is required when agent.name is 'codex'".to_string(),
        ));
    }
    if config.agent_backend == AgentBackend::KataCli && config.pi_agent.command.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "agent.command is required when agent.name is 'pi'".to_string(),
        ));
    }

    // workspace.branch_prefix must be present and non-empty
    if config.workspace.branch_prefix.trim().is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "workspace.branch_prefix must be non-empty".to_string(),
        ));
    }

    // workspace.strategy=worktree requires a local workspace.repo path
    if config.workspace.strategy == WorkspaceRepoStrategy::Worktree {
        let repo = config.workspace.repo.as_deref().ok_or_else(|| {
            SymphonyError::InvalidWorkflowConfig(
                "workspace.repo is required when workspace.strategy is 'worktree'".to_string(),
            )
        })?;
        if repo_is_remote(repo) {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "workspace.strategy 'worktree' requires workspace.repo to be a local path"
                    .to_string(),
            ));
        }
    }

    if config.workspace.strategy == WorkspaceRepoStrategy::CloneLocal {
        let repo = config.workspace.repo.as_deref().ok_or_else(|| {
            SymphonyError::InvalidWorkflowConfig(
                "workspace.repo is required when workspace.git_strategy is 'clone-local'"
                    .to_string(),
            )
        })?;
        if repo_is_remote(repo) {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "workspace.git_strategy 'clone-local' requires workspace.repo to be a local path"
                    .to_string(),
            ));
        }
    }

    if config.shared_context.ttl_ms == 0 {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "shared_context.ttl_ms must be greater than 0".to_string(),
        ));
    }

    if config.shared_context.max_entries == 0 {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "shared_context.max_entries must be greater than 0".to_string(),
        ));
    }

    if config.storage.busy_timeout_ms == 0 {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "storage.busy_timeout_ms must be greater than 0".to_string(),
        ));
    }

    if config.triage.enabled {
        if tracker_kind != "github" {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "triage.enabled requires tracker.kind to be 'github' (GitHub Projects v2 only for A1)"
                    .to_string(),
            ));
        }

        if config.triage.intake_label.trim().is_empty() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "triage.intake_label must be non-empty when triage is enabled".to_string(),
            ));
        }

        if config.triage.prompt.trim().is_empty() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "triage.prompt must be non-empty when triage is enabled".to_string(),
            ));
        }

        if config.triage.turn_timeout_ms == 0 {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "triage.turn_timeout_ms must be greater than 0".to_string(),
            ));
        }

        if config.triage.max_attempts == 0 {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "triage.max_attempts must be greater than 0".to_string(),
            ));
        }

        if config.triage.max_intake_pages == 0 {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "triage.max_intake_pages must be greater than 0".to_string(),
            ));
        }

        if config.agent_backend == AgentBackend::Codex && config.triage.model.is_some() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "triage.model is not supported when agent.name is 'codex'".to_string(),
            ));
        }

        let route_labels = [
            ("implement", config.triage.routes.implement.label.as_str()),
            ("spec", config.triage.routes.spec.label.as_str()),
            (
                "needs_information",
                config.triage.routes.needs_information.label.as_str(),
            ),
            ("park", config.triage.routes.park.label.as_str()),
            (
                "human_owned",
                config.triage.routes.human_owned.label.as_str(),
            ),
        ];

        for (route, label) in route_labels {
            if label.trim().is_empty() {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "triage.routes.{route}.label must be non-empty when triage is enabled"
                )));
            }
        }

        let intake = config.triage.intake_label.trim();
        for (route, label) in route_labels {
            if label.trim() == intake {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "triage.routes.{route}.label must not equal triage.intake_label ('{intake}')"
                )));
            }
        }

        let mut seen: Vec<&str> = Vec::with_capacity(route_labels.len());
        for (route, label) in route_labels {
            let normalized = label.trim();
            if seen.contains(&normalized) {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "triage route labels must be distinct; duplicate label '{normalized}' on route '{route}'"
                )));
            }
            seen.push(normalized);
        }
    }

    if config.spec.enabled {
        if tracker_kind != "github" {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "spec.enabled requires tracker.kind to be 'github' (GitHub Projects v2 only for A2)"
                    .to_string(),
            ));
        }

        for (field, value) in [
            ("spec.intake_label", config.spec.intake_label.as_str()),
            ("spec.prompts.draft", config.spec.prompts.draft.as_str()),
            ("spec.prompts.review", config.spec.prompts.review.as_str()),
            ("spec.prompts.revise", config.spec.prompts.revise.as_str()),
            ("spec.labels.approved", config.spec.labels.approved.as_str()),
            ("spec.labels.revise", config.spec.labels.revise.as_str()),
            (
                "spec.approval_route.label",
                config.spec.approval_route.label.as_str(),
            ),
        ] {
            if value.trim().is_empty() {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "{field} must be non-empty when spec is enabled"
                )));
            }
        }

        for (field, value) in [
            ("spec.turn_timeout_ms", config.spec.turn_timeout_ms),
            ("spec.max_intake_pages", config.spec.max_intake_pages as u64),
            (
                "spec.max_review_cycles",
                config.spec.max_review_cycles as u64,
            ),
            ("spec.max_attempts", config.spec.max_attempts as u64),
            (
                "spec.max_revision_requests",
                config.spec.max_revision_requests as u64,
            ),
        ] {
            if value == 0 {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "{field} must be greater than 0"
                )));
            }
        }

        if config.agent_backend == AgentBackend::Codex
            && (config.spec.model.is_some() || config.spec.review_model.is_some())
        {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "spec.model and spec.review_model are not supported when agent.name is 'codex'"
                    .to_string(),
            ));
        }

        let spec_managed: [(&str, &str); 4] = [
            ("spec.intake_label", config.spec.intake_label.as_str()),
            ("spec.labels.approved", config.spec.labels.approved.as_str()),
            ("spec.labels.revise", config.spec.labels.revise.as_str()),
            (
                "spec.approval_route.label",
                config.spec.approval_route.label.as_str(),
            ),
        ];
        for left in 0..spec_managed.len() {
            for right in left + 1..spec_managed.len() {
                if spec_managed[left]
                    .1
                    .trim()
                    .eq_ignore_ascii_case(spec_managed[right].1.trim())
                {
                    return Err(SymphonyError::InvalidWorkflowConfig(format!(
                        "{} and {} must use distinct labels (duplicate '{}')",
                        spec_managed[left].0,
                        spec_managed[right].0,
                        spec_managed[left].1.trim()
                    )));
                }
            }
        }

        if config.triage.enabled {
            if !config
                .triage
                .routes
                .spec
                .label
                .eq_ignore_ascii_case(&config.spec.intake_label)
            {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "triage.routes.spec.label ('{}') must equal spec.intake_label ('{}') when both stages are enabled",
                    config.triage.routes.spec.label, config.spec.intake_label
                )));
            }
            let triage_managed = [
                ("triage.intake_label", config.triage.intake_label.as_str()),
                (
                    "triage.routes.implement.label",
                    config.triage.routes.implement.label.as_str(),
                ),
                (
                    "triage.routes.needs_information.label",
                    config.triage.routes.needs_information.label.as_str(),
                ),
                (
                    "triage.routes.park.label",
                    config.triage.routes.park.label.as_str(),
                ),
                (
                    "triage.routes.human_owned.label",
                    config.triage.routes.human_owned.label.as_str(),
                ),
            ];
            for (decision_field, decision_label) in [
                ("spec.labels.approved", config.spec.labels.approved.as_str()),
                ("spec.labels.revise", config.spec.labels.revise.as_str()),
            ] {
                if let Some((triage_field, _)) = triage_managed
                    .iter()
                    .find(|(_, label)| decision_label.trim().eq_ignore_ascii_case(label.trim()))
                {
                    return Err(SymphonyError::InvalidWorkflowConfig(format!(
                        "{decision_field} and {triage_field} must use distinct labels (duplicate '{}')",
                        decision_label.trim()
                    )));
                }
            }
        }
    }

    if config.implementation.enabled {
        if tracker_kind != "github" {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "implementation.enabled requires tracker.kind to be 'github' (GitHub Projects v2 only for A3)"
                    .to_string(),
            ));
        }

        if !config.spec.enabled {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "implementation.enabled requires spec.enabled so an A2 approval path exists"
                    .to_string(),
            ));
        }

        for (field, value) in [
            ("implementation.prompt", config.implementation.prompt.as_str()),
            (
                "implementation.repair_prompt",
                config.implementation.repair_prompt.as_str(),
            ),
            ("implementation.spec_file", config.implementation.spec_file.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "{field} must be non-empty when implementation is enabled"
                )));
            }
        }

        for (field, value) in [
            ("implementation.max_turns", config.implementation.max_turns as u64),
            (
                "implementation.invocation_timeout_ms",
                config.implementation.invocation_timeout_ms,
            ),
            (
                "implementation.max_attempts",
                config.implementation.max_attempts as u64,
            ),
            (
                "implementation.max_validation_cycles",
                config.implementation.max_validation_cycles as u64,
            ),
            (
                "implementation.max_bundle_bytes",
                config.implementation.max_bundle_bytes,
            ),
        ] {
            if value == 0 {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "{field} must be greater than 0"
                )));
            }
        }

        if config.implementation.max_bundle_bytes > IMPLEMENTATION_MAX_BUNDLE_BYTES_HARD_CAP {
            return Err(SymphonyError::InvalidWorkflowConfig(format!(
                "implementation.max_bundle_bytes must be at most {IMPLEMENTATION_MAX_BUNDLE_BYTES_HARD_CAP} (1 GiB)"
            )));
        }

        if config.agent_backend == AgentBackend::Codex && config.implementation.model.is_some() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "implementation.model is not supported when agent.name is 'codex'".to_string(),
            ));
        }

        crate::implementation::artifact::validate_spec_file_path(&config.implementation.spec_file)
            .map_err(|error| {
                SymphonyError::InvalidWorkflowConfig(format!(
                    "implementation.spec_file is invalid: {error}"
                ))
            })?;

        let validation = &config.implementation.validation;
        if validation.is_empty() || validation.len() > IMPLEMENTATION_VALIDATION_MAX_COMMANDS {
            return Err(SymphonyError::InvalidWorkflowConfig(format!(
                "implementation.validation must contain 1 to {IMPLEMENTATION_VALIDATION_MAX_COMMANDS} commands when implementation is enabled"
            )));
        }
        let mut seen_names = Vec::new();
        for (index, command) in validation.iter().enumerate() {
            if command.name.trim().is_empty()
                || command.name.len() > IMPLEMENTATION_VALIDATION_NAME_MAX_BYTES
            {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "implementation.validation[{index}].name must be 1 to {IMPLEMENTATION_VALIDATION_NAME_MAX_BYTES} UTF-8 bytes"
                )));
            }
            if command.command.trim().is_empty()
                || command.command.len() > IMPLEMENTATION_VALIDATION_COMMAND_MAX_BYTES
            {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "implementation.validation[{index}].command must be 1 to {IMPLEMENTATION_VALIDATION_COMMAND_MAX_BYTES} UTF-8 bytes"
                )));
            }
            if command.timeout_ms == 0 {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "implementation.validation[{index}].timeout_ms must be greater than 0"
                )));
            }
            let normalized = command.name.trim().to_ascii_lowercase();
            if seen_names.iter().any(|name: &String| name == &normalized) {
                return Err(SymphonyError::InvalidWorkflowConfig(format!(
                    "implementation.validation names must be unique (duplicate '{}')",
                    command.name.trim()
                )));
            }
            seen_names.push(normalized);
        }

        if config.implementation.mode == ImplementationMode::Automatic {
            let state = config
                .implementation
                .completion_route
                .as_ref()
                .map(|route| route.state.trim())
                .unwrap_or("");
            if state.is_empty() {
                return Err(SymphonyError::InvalidWorkflowConfig(
                    "implementation.completion_route.state is required when implementation.mode is 'automatic'"
                        .to_string(),
                ));
            }
        }
    }

    Ok(ValidatedServiceConfig(config.clone()))
}
// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_drops_nulls() {
        let yaml_str = "key: ~\nother: value";
        let val: Value = serde_yaml::from_str(yaml_str).unwrap();
        let normalized = normalize_keys(val);
        assert!(
            normalized.get("key").is_none(),
            "null entry should be dropped"
        );
        assert!(normalized.get("other").is_some());
    }

    #[test]
    fn normalize_coerces_keys() {
        // Numeric map keys in YAML
        let yaml_str = "1: one\n2: two";
        let val: Value = serde_yaml::from_str(yaml_str).unwrap();
        let normalized = normalize_keys(val);
        assert!(
            normalized.get("1").is_some(),
            "numeric key should be coerced to string"
        );
    }

    #[test]
    fn resolve_env_returns_raw_for_non_dollar() {
        assert_eq!(resolve_env("literal"), "literal");
    }

    #[test]
    fn resolve_env_returns_empty_for_unset_var() {
        // Use an env var name that is extremely unlikely to be set.
        let result = resolve_env("$SYMPHONY_TEST_UNSET_XYZZY_12345");
        assert_eq!(result, "");
    }

    #[test]
    fn expand_tilde_home() {
        assert_eq!(
            expand_tilde_with_home("~/foo", Some("/Users/tester")),
            "/Users/tester/foo"
        );
        assert_eq!(
            expand_tilde_with_home("~", Some("/Users/tester")),
            "/Users/tester"
        );
    }

    #[test]
    fn expand_tilde_no_op() {
        assert_eq!(expand_tilde("/abs/path"), "/abs/path");
        assert_eq!(expand_tilde("relative"), "relative");
    }

    #[test]
    fn expand_tilde_home_unset_keeps_input() {
        assert_eq!(expand_tilde_with_home("~", None), "~");
        assert_eq!(expand_tilde_with_home("~/foo", None), "~/foo");
        assert_eq!(expand_tilde_with_home("~/foo", Some("")), "~/foo");
    }

    #[test]
    fn parse_codex_command_string() {
        let val = Value::String("codex app-server".to_string());
        let cmd = parse_codex_command(val).unwrap();
        assert_eq!(cmd, vec!["codex", "app-server"]);
    }

    #[test]
    fn parse_codex_command_list() {
        let val: Value = serde_yaml::from_str("- codex\n- app-server").unwrap();
        let cmd = parse_codex_command(val).unwrap();
        assert_eq!(cmd, vec!["codex", "app-server"]);
    }

    #[test]
    fn parse_codex_command_empty_string() {
        let val = Value::String(String::new());
        let cmd = parse_codex_command(val).unwrap();
        assert!(cmd.is_empty());
    }

    #[test]
    fn parse_pi_agent_command_string_shell_words() {
        let val = Value::String(
            "\"/tmp/pi runtime/pi\" --mode rpc --model \"anthropic/claude sonnet\"".to_string(),
        );
        let cmd = parse_pi_agent_command(val).unwrap();
        assert_eq!(
            cmd,
            vec![
                "/tmp/pi runtime/pi",
                "--mode",
                "rpc",
                "--model",
                "anthropic/claude sonnet",
            ]
        );
    }

    #[test]
    fn parse_pi_agent_command_list() {
        let val: Value = serde_yaml::from_str("- pi\n- --mode\n- rpc").unwrap();
        let cmd = parse_pi_agent_command(val).unwrap();
        assert_eq!(cmd, vec!["pi", "--mode", "rpc"]);
    }

    #[test]
    fn raw_supervisor_config_defaults_when_section_omitted() {
        let yaml: Value =
            serde_yaml::from_str("tracker: { kind: linear }").expect("yaml fixture should parse");
        let normalized = normalize_keys(yaml);
        let raw: RawSupervisorConfig =
            extract_section(&normalized, "supervisor").expect("section should parse");

        assert_eq!(raw.enabled, None);
        assert_eq!(raw.model, None);
        assert_eq!(raw.steer_cooldown_ms, None);
    }

    #[test]
    fn raw_supervisor_config_honors_explicit_cooldown() {
        let yaml: Value = serde_yaml::from_str(
            "tracker: { kind: linear }\nsupervisor:\n  steer_cooldown_ms: 45000",
        )
        .expect("yaml fixture should parse");
        let normalized = normalize_keys(yaml);
        let raw: RawSupervisorConfig =
            extract_section(&normalized, "supervisor").expect("section should parse");

        assert_eq!(raw.steer_cooldown_ms, Some(45_000));
    }

    #[test]
    fn supervisor_model_env_and_empty_values_resolve_to_none() {
        let env_yaml: Value = serde_yaml::from_str(
            "tracker: { kind: linear }\nsupervisor:\n  model: $SYMPHONY_TEST_UNSET_SUPERVISOR_MODEL",
        )
        .expect("yaml fixture should parse");
        let env_config = from_workflow(&env_yaml).expect("workflow should parse");
        assert_eq!(env_config.supervisor.model, None);

        let empty_yaml: Value =
            serde_yaml::from_str("tracker: { kind: linear }\nsupervisor:\n  model: \"\"")
                .expect("yaml fixture should parse");
        let empty_config = from_workflow(&empty_yaml).expect("workflow should parse");
        assert_eq!(empty_config.supervisor.model, None);
    }

    #[test]
    fn api_key_debug_is_redacted() {
        let key = ApiKey::new("super-secret-key");
        assert_eq!(format!("{:?}", key), "[REDACTED]");
    }

    #[test]
    fn api_key_deref_gives_str() {
        let key = ApiKey::new("my-key");
        assert_eq!(&*key, "my-key");
        assert_eq!(key.as_str(), "my-key");
    }

    fn github_base_yaml() -> String {
        r#"
tracker:
  kind: github
  api_key: test-token
  repo_owner: owner
  repo_name: repo
  github_project_owner_type: user
  github_project_number: 1
agent:
  name: pi
  command: pi
"#
        .to_string()
    }

    #[test]
    fn implementation_defaults_when_section_absent() {
        let yaml: Value = serde_yaml::from_str(&github_base_yaml()).unwrap();
        let config = from_workflow(&yaml).unwrap();
        assert!(!config.implementation.enabled);
        assert_eq!(config.implementation.mode, ImplementationMode::Preview);
        assert_eq!(
            config.implementation.prompt,
            "prompts/implementation.md"
        );
        assert!(config.implementation.validation.is_empty());
    }

    #[test]
    fn implementation_parses_preview_validation_commands() {
        let mut yaml = github_base_yaml();
        yaml.push_str(
            r#"
spec:
  enabled: true
implementation:
  enabled: true
  mode: preview
  validation:
    - name: unit
      command: cargo test
      timeout_ms: 60000
"#,
        );
        let value: Value = serde_yaml::from_str(&yaml).unwrap();
        let config = from_workflow(&value).unwrap();
        assert!(config.implementation.enabled);
        assert_eq!(config.implementation.validation.len(), 1);
        assert_eq!(config.implementation.validation[0].name, "unit");
        validate(&config).unwrap();
    }

    #[test]
    fn implementation_rejects_automatic_without_completion_route() {
        let mut yaml = github_base_yaml();
        yaml.push_str(
            r#"
spec:
  enabled: true
implementation:
  enabled: true
  mode: automatic
  validation:
    - name: unit
      command: cargo test
      timeout_ms: 60000
"#,
        );
        let value: Value = serde_yaml::from_str(&yaml).unwrap();
        let config = from_workflow(&value).unwrap();
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("completion_route.state"));
    }

    #[test]
    fn implementation_requires_spec_enabled() {
        let mut yaml = github_base_yaml();
        yaml.push_str(
            r#"
implementation:
  enabled: true
  validation:
    - name: unit
      command: cargo test
      timeout_ms: 60000
"#,
        );
        let value: Value = serde_yaml::from_str(&yaml).unwrap();
        let config = from_workflow(&value).unwrap();
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("spec.enabled"));
    }

    #[test]
    fn implementation_rejects_duplicate_validation_names() {
        let mut yaml = github_base_yaml();
        yaml.push_str(
            r#"
spec:
  enabled: true
implementation:
  enabled: true
  validation:
    - name: unit
      command: cargo test
      timeout_ms: 60000
    - name: UNIT
      command: cargo clippy
      timeout_ms: 60000
"#,
        );
        let value: Value = serde_yaml::from_str(&yaml).unwrap();
        let config = from_workflow(&value).unwrap();
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("unique"));
    }
}
