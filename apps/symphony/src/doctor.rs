use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use reqwest::Method;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

use crate::config;
use crate::domain::{
    AgentBackend, ServiceConfig, TrackerConfig, WorkspaceConfig, WorkspaceIsolation,
    WorkspaceRepoStrategy,
};
use crate::error::SymphonyError;
use crate::github::auth::{
    github_token_missing_message, github_token_source_name, resolve_github_token,
};
use crate::github::client::GithubClient;
use crate::github::projects_v2::ProjectsV2Client;
use crate::linear::adapter::TrackerAdapter;
use crate::linear::client::LinearClient;
use crate::notifications;
use crate::repo_url::repo_is_remote;
use crate::workflow;
use crate::workspace;

const VIEWER_QUERY: &str = r#"
query SymphonyDoctorViewer {
  viewer {
    id
  }
}
"#;

const PROJECT_QUERY: &str = r#"
query SymphonyDoctorProject($slug: String!) {
  projects(filter: {slugId: {eq: $slug}}, first: 1) {
    nodes {
      id
      name
      teams {
        nodes {
          id
          name
          states {
            nodes {
              name
            }
          }
        }
      }
    }
  }
}
"#;

const USERS_QUERY: &str = r#"
query SymphonyDoctorUsers($first: Int!, $after: String) {
  users(first: $first, after: $after) {
    nodes {
      id
      displayName
      name
      email
    }
    pageInfo {
      hasNextPage
      endCursor
    }
  }
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warning,
    Error,
    Skipped,
}

impl CheckStatus {
    fn icon(self) -> &'static str {
        match self {
            Self::Pass => "✅",
            Self::Warning => "⚠️",
            Self::Error => "🚨",
            Self::Skipped => "⏭️",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorCheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
    pub details: Option<String>,
}

impl DoctorCheckResult {
    fn pass(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Pass,
            message: message.into(),
            details: None,
        }
    }

    fn warning(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Warning,
            message: message.into(),
            details: None,
        }
    }

    fn error(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Error,
            message: message.into(),
            details: None,
        }
    }

    fn skipped(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Skipped,
            message: message.into(),
            details: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnvReferenceIssue {
    path: String,
    var_name: String,
}

pub fn check_config(workflow_path: &Path) -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();

    let workflow_definition = match workflow::parse_workflow(workflow_path) {
        Ok(definition) => {
            results.push(DoctorCheckResult::pass(
                "Config Parse",
                format!("Parsed {}", workflow_path.display()),
            ));
            definition
        }
        Err(err) => {
            results.push(DoctorCheckResult::error("Config Parse", err.to_string()));
            return results;
        }
    };

    let unresolved_env_refs = collect_unresolved_env_refs(&workflow_definition.config);
    if unresolved_env_refs.is_empty() {
        results.push(DoctorCheckResult::pass(
            "Config Env",
            "All $ENV_VAR references resolved",
        ));
    } else {
        for issue in unresolved_env_refs {
            let message = format!(
                "{} references ${} but the environment variable is unset or empty",
                issue.path, issue.var_name
            );
            if is_required_env_reference_path(&issue.path) {
                results.push(DoctorCheckResult::error("Config Env", message));
            } else {
                results.push(DoctorCheckResult::warning("Config Env", message));
            }
        }
    }

    let invalid_events = collect_invalid_slack_events(&workflow_definition.config);
    if invalid_events.is_empty() {
        results.push(DoctorCheckResult::pass(
            "Config Notifications",
            "Slack notification events are valid",
        ));
    } else {
        for event in invalid_events {
            results.push(DoctorCheckResult::warning(
                "Config Notifications",
                format!(
                    "Unsupported notifications.slack.events value '{event}' (supported: {})",
                    notifications::SUPPORTED_SLACK_EVENTS.join(", ")
                ),
            ));
        }
    }

    let service_config = match config::from_workflow(&workflow_definition.config) {
        Ok(config) => config,
        Err(err) => {
            results.push(DoctorCheckResult::error("Config Parse", err.to_string()));
            return results;
        }
    };

    match config::validate(&service_config) {
        Ok(_) => results.push(DoctorCheckResult::pass(
            "Config Validate",
            "Required workflow settings are present",
        )),
        Err(err) => {
            results.push(DoctorCheckResult::error("Config Validate", err.to_string()));
        }
    }

    let workflow_dir = workflow_path.parent().unwrap_or(Path::new("."));
    let prompt_paths = collect_prompt_paths(&service_config);
    if prompt_paths.is_empty() {
        results.push(DoctorCheckResult::skipped(
            "Config Prompts",
            "No prompt files configured",
        ));
    } else {
        let mut missing_prompt_count = 0;
        for (prompt_slot, relative_path) in prompt_paths {
            let candidate_path = resolve_prompt_path(workflow_dir, &relative_path);
            if !candidate_path.exists() {
                missing_prompt_count += 1;
                results.push(DoctorCheckResult::warning(
                    "Config Prompts",
                    format!(
                        "{prompt_slot} points to missing file {}",
                        candidate_path.display()
                    ),
                ));
            }
        }

        if missing_prompt_count == 0 {
            results.push(DoctorCheckResult::pass(
                "Config Prompts",
                "All configured prompt files exist",
            ));
        }
    }

    results
}

pub async fn check_github(config: &TrackerConfig) -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();

    let Some(resolved_token) = resolve_github_token(config) else {
        results.push(DoctorCheckResult::error(
            "GitHub PAT",
            github_token_missing_message(),
        ));
        return results;
    };
    let token_source = resolved_token.source;
    let token = resolved_token.token;

    let Some(repo_owner) = config
        .repo_owner
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        results.push(DoctorCheckResult::error(
            "GitHub Repo",
            "tracker.repo_owner is required when tracker.kind is github",
        ));
        return results;
    };

    let Some(repo_name) = config
        .repo_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        results.push(DoctorCheckResult::error(
            "GitHub Repo",
            "tracker.repo_name is required when tracker.kind is github",
        ));
        return results;
    };

    let label_prefix = config
        .label_prefix
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("symphony");

    let endpoint = config.endpoint.trim();
    let endpoint = if endpoint.is_empty() {
        "https://api.github.com"
    } else {
        endpoint
    };

    let client = GithubClient::with_base_url(
        token,
        repo_owner.to_string(),
        repo_name.to_string(),
        label_prefix.to_string(),
        endpoint,
    );

    let pat_ok = match client.request(Method::GET, "/user", None).await {
        Ok(response) => {
            match response.json::<JsonValue>().await {
                Ok(payload) => {
                    let login = payload
                        .get("login")
                        .and_then(|value| value.as_str())
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("authenticated");
                    results.push(DoctorCheckResult::pass(
                        "GitHub PAT",
                        format!(
                            "PAT authenticated as {login} (source: {})",
                            github_token_source_name(token_source)
                        ),
                    ));
                }
                Err(err) => {
                    results.push(DoctorCheckResult::warning(
                        "GitHub PAT",
                        format!(
                            "PAT authenticated but doctor could not decode /user response: {err}"
                        ),
                    ));
                }
            }
            true
        }
        Err(SymphonyError::GithubApiStatus { status: 401, .. }) => {
            results.push(DoctorCheckResult::error(
                "GitHub PAT",
                "PAT authentication failed (HTTP 401)",
            ));
            false
        }
        Err(SymphonyError::GithubApiStatus { status, .. }) => {
            results.push(DoctorCheckResult::warning(
                "GitHub PAT",
                format!("PAT authentication check returned HTTP {status}"),
            ));
            false
        }
        Err(err) => {
            results.push(DoctorCheckResult::warning(
                "GitHub PAT",
                format!("PAT authentication check failed: {err}"),
            ));
            false
        }
    };

    let repo_ok = if pat_ok {
        let repo_path = format!("/repos/{repo_owner}/{repo_name}");
        match client.request(Method::GET, &repo_path, None).await {
            Ok(_) => {
                results.push(DoctorCheckResult::pass(
                    "GitHub Repo",
                    format!("Repository {repo_owner}/{repo_name} accessible"),
                ));
                true
            }
            Err(SymphonyError::GithubApiStatus { status: 404, .. }) => {
                results.push(DoctorCheckResult::error(
                    "GitHub Repo",
                    format!("Repository {repo_owner}/{repo_name} not found (HTTP 404)"),
                ));
                false
            }
            Err(SymphonyError::GithubApiStatus { status, .. }) => {
                results.push(DoctorCheckResult::warning(
                    "GitHub Repo",
                    format!("Repository {repo_owner}/{repo_name} check returned HTTP {status}"),
                ));
                false
            }
            Err(err) => {
                results.push(DoctorCheckResult::warning(
                    "GitHub Repo",
                    format!("Repository {repo_owner}/{repo_name} check failed: {err}"),
                ));
                false
            }
        }
    } else {
        results.push(DoctorCheckResult::skipped(
            "GitHub Repo",
            "Skipped because PAT authentication failed",
        ));
        false
    };

    if let Some(project_number) = config.github_project_number {
        if !pat_ok {
            results.push(DoctorCheckResult::skipped(
                "GitHub Project",
                "Skipped because PAT authentication failed",
            ));
        } else {
            let projects_client = ProjectsV2Client::new(client.clone());
            match projects_client
                .resolve_status_field(repo_owner, project_number)
                .await
            {
                Ok(status_field) => {
                    results.push(DoctorCheckResult::pass(
                        "GitHub Project",
                        format!(
                            "Project #{project_number} found with Status field ({} options)",
                            status_field.options.len()
                        ),
                    ));
                }
                Err(err) => {
                    let err_text = err.to_string();
                    let err_lower = err_text.to_ascii_lowercase();
                    let message = if err_lower.contains("status field") {
                        format!(
                            "Project #{project_number} found but Status field is missing or inaccessible"
                        )
                    } else if err_lower.contains("not found") {
                        format!("Project #{project_number} not found or not accessible")
                    } else {
                        format!(
                            "Project #{project_number} not found or not accessible ({err_text})"
                        )
                    };
                    results.push(DoctorCheckResult::error("GitHub Project", message));
                }
            }
        }

        results.push(DoctorCheckResult::skipped(
            "GitHub Labels",
            "Projects v2 mode configured (github_project_number set)",
        ));
        return results;
    }

    results.push(DoctorCheckResult::skipped(
        "GitHub Project",
        "No github_project_number configured — label mode assumed",
    ));

    if !pat_ok {
        results.push(DoctorCheckResult::skipped(
            "GitHub Labels",
            "Skipped because PAT authentication failed",
        ));
        return results;
    }

    if !repo_ok {
        results.push(DoctorCheckResult::skipped(
            "GitHub Labels",
            "Skipped because repository check failed",
        ));
        return results;
    }

    match client.list_labels().await {
        Ok(labels) => {
            let normalized_labels: BTreeSet<String> = labels
                .iter()
                .map(|label| label.name.trim().to_ascii_lowercase())
                .filter(|label| !label.is_empty())
                .collect();

            let expected_labels: BTreeSet<String> = config
                .active_states
                .iter()
                .chain(config.terminal_states.iter())
                .map(|state| normalize_state_for_label(state))
                .filter(|state| !state.is_empty())
                .map(|state| format!("{label_prefix}:{state}"))
                .collect();

            let mut missing_labels = Vec::new();
            for expected in expected_labels {
                if !normalized_labels.contains(&expected.to_ascii_lowercase()) {
                    missing_labels.push(expected);
                }
            }

            if missing_labels.is_empty() {
                results.push(DoctorCheckResult::pass(
                    "GitHub Labels",
                    "All configured state labels exist on repository",
                ));
            } else {
                for missing in missing_labels {
                    results.push(DoctorCheckResult::warning(
                        "GitHub Labels",
                        format!(
                            "Label {missing} not found on repository — create it before running Symphony"
                        ),
                    ));
                }
            }
        }
        Err(err) => {
            results.push(DoctorCheckResult::warning(
                "GitHub Labels",
                format!("Failed to list repository labels: {err}"),
            ));
        }
    }

    results
}

pub async fn check_linear(config: &TrackerConfig) -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();
    let client = LinearClient::new(config.clone());

    let viewer_body = match client
        .graphql_raw(VIEWER_QUERY, serde_json::json!({}))
        .await
    {
        Ok(body) => body,
        Err(err) => {
            results.push(DoctorCheckResult::error(
                "Linear Auth",
                format_linear_error("Failed to authenticate with Linear", &err),
            ));
            return results;
        }
    };

    if let Some(message) = graphql_error_message(&viewer_body) {
        results.push(DoctorCheckResult::error(
            "Linear Auth",
            format!("Failed to authenticate with Linear: {message}"),
        ));
        return results;
    }

    let viewer_id = viewer_body
        .get("data")
        .and_then(|data| data.get("viewer"))
        .and_then(|viewer| viewer.get("id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);

    let viewer_id = match viewer_id {
        Some(id) => {
            results.push(DoctorCheckResult::pass(
                "Linear Auth",
                format!("Authenticated as viewer {id}"),
            ));
            id
        }
        None => {
            results.push(DoctorCheckResult::error(
                "Linear Auth",
                "Linear viewer query returned no id",
            ));
            return results;
        }
    };

    let Some(project_slug) = config
        .project_slug
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        results.push(DoctorCheckResult::error(
            "Linear Project",
            "tracker.project_slug is missing",
        ));
        return results;
    };

    let project_body = match client
        .graphql_raw(PROJECT_QUERY, serde_json::json!({ "slug": project_slug }))
        .await
    {
        Ok(body) => body,
        Err(err) => {
            results.push(DoctorCheckResult::error(
                "Linear Project",
                format_linear_error("Failed to fetch project by slug", &err),
            ));
            return results;
        }
    };

    if let Some(message) = graphql_error_message(&project_body) {
        results.push(DoctorCheckResult::error(
            "Linear Project",
            format!("Failed to fetch project by slug: {message}"),
        ));
        return results;
    }

    let project_node = project_body
        .get("data")
        .and_then(|data| data.get("projects"))
        .and_then(|projects| projects.get("nodes"))
        .and_then(|nodes| nodes.as_array())
        .and_then(|nodes| nodes.first());

    let Some(project_node) = project_node else {
        results.push(DoctorCheckResult::error(
            "Linear Project",
            format!("No Linear project found for slug '{project_slug}'"),
        ));
        return results;
    };

    let project_name = project_node
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(project_slug);

    results.push(DoctorCheckResult::pass(
        "Linear Project",
        format!("Resolved project '{project_name}' ({project_slug})"),
    ));

    let mut team_states = HashSet::new();
    if let Some(teams) = project_node
        .get("teams")
        .and_then(|teams| teams.get("nodes"))
        .and_then(|nodes| nodes.as_array())
    {
        for team in teams {
            let Some(states) = team
                .get("states")
                .and_then(|states| states.get("nodes"))
                .and_then(|nodes| nodes.as_array())
            else {
                continue;
            };

            for state in states {
                if let Some(name) = state.get("name").and_then(|name| name.as_str()) {
                    let normalized = name.trim().to_ascii_lowercase();
                    if !normalized.is_empty() {
                        team_states.insert(normalized);
                    }
                }
            }
        }
    }

    if team_states.is_empty() {
        results.push(DoctorCheckResult::warning(
            "Linear States",
            "Could not resolve team workflow states from project",
        ));
    } else {
        let configured_states: BTreeSet<String> = config
            .active_states
            .iter()
            .chain(config.terminal_states.iter())
            .map(|state| state.trim())
            .filter(|state| !state.is_empty())
            .map(|state| state.to_string())
            .collect();

        let mut missing = Vec::new();
        for configured_state in configured_states {
            if !team_states.contains(&configured_state.to_ascii_lowercase()) {
                missing.push(configured_state);
            }
        }

        if missing.is_empty() {
            results.push(DoctorCheckResult::pass(
                "Linear States",
                "Configured active/terminal states match team workflow",
            ));
        } else {
            for missing_state in missing {
                results.push(DoctorCheckResult::warning(
                    "Linear States",
                    format!("Configured state '{missing_state}' not found in team workflow"),
                ));
            }
        }
    }

    let assignee = config
        .assignee
        .as_deref()
        .map(str::trim)
        .filter(|assignee| !assignee.is_empty());

    let Some(assignee) = assignee else {
        results.push(DoctorCheckResult::skipped(
            "Linear Assignee",
            "tracker.assignee is not configured",
        ));
        return results;
    };

    if assignee.eq_ignore_ascii_case("me") {
        results.push(DoctorCheckResult::pass(
            "Linear Assignee",
            format!("tracker.assignee=me resolved to viewer {viewer_id}"),
        ));
        return results;
    }

    match resolve_assignee(client, assignee).await {
        Ok(Some(user_id)) => results.push(DoctorCheckResult::pass(
            "Linear Assignee",
            format!("Resolved assignee '{assignee}' to user {user_id}"),
        )),
        Ok(None) => results.push(DoctorCheckResult::warning(
            "Linear Assignee",
            format!("Could not resolve assignee '{assignee}' to a Linear user"),
        )),
        Err(err) => results.push(DoctorCheckResult::warning(
            "Linear Assignee",
            format_linear_error("Assignee lookup failed", &err),
        )),
    }

    results
}

pub fn check_backend(config: &ServiceConfig) -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();

    let command = match config.agent_backend {
        AgentBackend::KataCli => &config.pi_agent.command,
        AgentBackend::Codex => &config.codex.command,
    };

    if command.is_empty() {
        results.push(DoctorCheckResult::error(
            "Backend",
            "Configured backend command is empty",
        ));
        return results;
    }

    let executable = &command[0];
    match Command::new(executable).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version_line = String::from_utf8_lossy(&output.stdout)
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(str::trim)
                .map(str::to_string)
                .or_else(|| {
                    String::from_utf8_lossy(&output.stderr)
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .map(str::trim)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "backend responded to --version".to_string());

            results.push(DoctorCheckResult::pass(
                "Backend",
                format!("{executable} is available ({version_line})"),
            ));
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr)
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(str::trim)
                .map(str::to_string)
                .unwrap_or_else(|| "no stderr output".to_string());
            results.push(DoctorCheckResult::error(
                "Backend",
                format!(
                    "{executable} --version exited with status {} ({stderr})",
                    output
                        .status
                        .code()
                        .map_or_else(|| "signal".to_string(), |code| code.to_string())
                ),
            ));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            results.push(DoctorCheckResult::error(
                "Backend",
                format!("'{executable}' not found on PATH"),
            ));
        }
        Err(err) => {
            results.push(DoctorCheckResult::error(
                "Backend",
                format!("Failed to execute '{executable} --version': {err}"),
            ));
        }
    }

    results
}

pub fn check_workspace(config: &WorkspaceConfig) -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();
    let workspace_root = PathBuf::from(&config.root);

    if workspace_root.exists() {
        if !workspace_root.is_dir() {
            results.push(DoctorCheckResult::error(
                "Workspace Root",
                format!(
                    "workspace.root exists but is not a directory: {}",
                    workspace_root.display()
                ),
            ));
        } else if let Err(err) = assert_directory_writable(&workspace_root) {
            results.push(DoctorCheckResult::error(
                "Workspace Root",
                format!(
                    "workspace.root is not writable ({}): {err}",
                    workspace_root.display()
                ),
            ));
        } else {
            results.push(DoctorCheckResult::pass(
                "Workspace Root",
                format!("workspace.root is writable: {}", workspace_root.display()),
            ));
        }
    } else {
        match fs::create_dir_all(&workspace_root) {
            Ok(()) => {
                if let Err(err) = assert_directory_writable(&workspace_root) {
                    results.push(DoctorCheckResult::error(
                        "Workspace Root",
                        format!(
                            "workspace.root was created but is not writable ({}): {err}",
                            workspace_root.display()
                        ),
                    ));
                } else {
                    results.push(DoctorCheckResult::pass(
                        "Workspace Root",
                        format!("Created workspace.root: {}", workspace_root.display()),
                    ));
                }
            }
            Err(err) => {
                results.push(DoctorCheckResult::error(
                    "Workspace Root",
                    format!(
                        "Could not create workspace.root {}: {err}",
                        workspace_root.display()
                    ),
                ));
            }
        }
    }

    match config
        .repo
        .as_deref()
        .map(str::trim)
        .filter(|repo| !repo.is_empty())
    {
        None => results.push(DoctorCheckResult::skipped(
            "Workspace Repo",
            "workspace.repo is not configured",
        )),
        Some(repo) if repo_is_remote(repo) => {
            if remote_repo_format_is_valid(repo) {
                results.push(DoctorCheckResult::pass(
                    "Workspace Repo",
                    format!("Remote repo reference format looks valid: {repo}"),
                ));
            } else {
                results.push(DoctorCheckResult::warning(
                    "Workspace Repo",
                    format!("Remote repo reference looks invalid: {repo}"),
                ));
            }
        }
        Some(repo) => {
            let repo_path = Path::new(repo);
            if repo_path.exists() {
                results.push(DoctorCheckResult::pass(
                    "Workspace Repo",
                    format!("Local repo path exists: {}", repo_path.display()),
                ));
            } else {
                results.push(DoctorCheckResult::warning(
                    "Workspace Repo",
                    format!("Local repo path does not exist: {}", repo_path.display()),
                ));
            }
        }
    }

    let repo_is_remote = config.repo.as_deref().map(repo_is_remote);
    match config.strategy {
        WorkspaceRepoStrategy::Worktree | WorkspaceRepoStrategy::CloneLocal
            if repo_is_remote == Some(true) =>
        {
            results.push(DoctorCheckResult::error(
                "Workspace Strategy",
                format!(
                    "workspace.git_strategy '{:?}' requires a local workspace.repo path",
                    config.strategy
                ),
            ));
        }
        WorkspaceRepoStrategy::CloneRemote if repo_is_remote == Some(false) => {
            results.push(DoctorCheckResult::error(
                "Workspace Strategy",
                "workspace.git_strategy 'clone-remote' requires a remote workspace.repo URL",
            ));
        }
        _ => results.push(DoctorCheckResult::pass(
            "Workspace Strategy",
            "workspace.git_strategy is compatible with workspace.repo",
        )),
    }

    if config.isolation == WorkspaceIsolation::Docker {
        match Command::new("docker")
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let version = if version.is_empty() {
                    "daemon reachable".to_string()
                } else {
                    format!("daemon reachable (version {version})")
                };
                results.push(DoctorCheckResult::pass("Workspace Docker", version));
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let status_code = output
                    .status
                    .code()
                    .map_or_else(|| "signal".to_string(), |code| code.to_string());
                results.push(DoctorCheckResult::error(
                    "Workspace Docker",
                    format!("docker info failed (status {status_code}): {stderr}"),
                ));
            }
            Err(err) => {
                results.push(DoctorCheckResult::error(
                    "Workspace Docker",
                    format!("Failed to execute 'docker info': {err}"),
                ));
            }
        }
    } else {
        results.push(DoctorCheckResult::skipped(
            "Workspace Docker",
            "workspace.isolation is not docker",
        ));
    }

    results
}

pub async fn check_orphans(
    config: &ServiceConfig,
    adapter: &dyn TrackerAdapter,
) -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();

    let workspace_root = PathBuf::from(&config.workspace.root);
    if !workspace_root.exists() {
        results.push(DoctorCheckResult::warning(
            "Orphans",
            format!(
                "workspace.root does not exist ({}); orphan scan skipped",
                workspace_root.display()
            ),
        ));

        if !config.worker.ssh_hosts.is_empty() {
            results.push(DoctorCheckResult::skipped(
                "SSH Hosts",
                "worker.ssh_hosts configured; SSH reachability checks are not yet implemented",
            ));
        }

        return results;
    }

    let discovered =
        workspace::scan_workspace_root(&workspace_root, &config.workspace.branch_prefix);
    if discovered.is_empty() {
        results.push(DoctorCheckResult::pass(
            "Orphans",
            format!("No workspaces found under {}", workspace_root.display()),
        ));
    } else {
        let active_issues = match adapter
            .fetch_issues_by_states(&config.tracker.active_states)
            .await
        {
            Ok(issues) => issues,
            Err(err) => {
                results.push(DoctorCheckResult::error(
                    "Orphans",
                    format!("Failed to fetch active issues from tracker: {err}"),
                ));

                if !config.worker.ssh_hosts.is_empty() {
                    results.push(DoctorCheckResult::skipped(
                        "SSH Hosts",
                        "worker.ssh_hosts configured; SSH reachability checks are not yet implemented",
                    ));
                }

                return results;
            }
        };

        let active_identifiers: HashSet<String> = active_issues
            .iter()
            .map(|issue| issue.identifier.to_ascii_uppercase())
            .collect();

        let mut orphan_count = 0usize;
        let mut discovered_entries = discovered.into_iter().collect::<Vec<_>>();
        discovered_entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (identifier, path) in discovered_entries {
            if !active_identifiers.contains(&identifier.to_ascii_uppercase()) {
                orphan_count += 1;
                results.push(DoctorCheckResult::warning(
                    "Orphans",
                    format!(
                        "Workspace {} has no matching active issue ({identifier})",
                        path.display()
                    ),
                ));
            }
        }

        if orphan_count == 0 {
            results.push(DoctorCheckResult::pass(
                "Orphans",
                "All on-disk workspaces map to active issues",
            ));
        }
    }

    if !config.worker.ssh_hosts.is_empty() {
        results.push(DoctorCheckResult::skipped(
            "SSH Hosts",
            "worker.ssh_hosts configured; SSH reachability checks are not yet implemented",
        ));
    }

    results
}

pub fn check_triage(config: &ServiceConfig, workflow_path: &Path) -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();

    if !config.triage.enabled {
        results.push(DoctorCheckResult::skipped(
            "Triage",
            "triage.enabled is false — triage checks skipped",
        ));
        return results;
    }

    let workflow_dir = workflow_path.parent().unwrap_or(Path::new("."));
    let forge_host =
        crate::triage::storage_path::forge_host_from_endpoint(&config.tracker.endpoint);
    let owner = config.tracker.repo_owner.as_deref().unwrap_or("unknown");
    let repo = config.tracker.repo_name.as_deref().unwrap_or("unknown");
    let storage_path = crate::triage::storage_path::resolve_storage_path(
        &config.storage,
        &forge_host,
        owner,
        repo,
    );
    let lock_path = crate::triage::storage_path::lock_path_for_storage(&storage_path);

    if let Some(parent) = storage_path.parent() {
        match fs::create_dir_all(parent) {
            Ok(()) => {
                let probe = parent.join(".symphony-doctor-write-probe");
                match fs::write(&probe, b"ok") {
                    Ok(()) => {
                        let _ = fs::remove_file(&probe);
                        results.push(DoctorCheckResult::pass(
                            "Triage Storage",
                            format!("Storage path writable at {}", storage_path.display()),
                        ));
                    }
                    Err(err) => results.push(DoctorCheckResult::error(
                        "Triage Storage",
                        format!("Storage parent {} is not writable: {err}", parent.display()),
                    )),
                }
            }
            Err(err) => results.push(DoctorCheckResult::error(
                "Triage Storage",
                format!(
                    "Could not create storage parent {}: {err}",
                    parent.display()
                ),
            )),
        }
    } else {
        results.push(DoctorCheckResult::error(
            "Triage Storage",
            format!(
                "Storage path {} has no parent directory",
                storage_path.display()
            ),
        ));
    }

    if let Some(lock_parent) = lock_path.parent() {
        match fs::create_dir_all(lock_parent) {
            Ok(()) => {
                use fs2::FileExt;
                use std::fs::OpenOptions;
                match OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(&lock_path)
                {
                    Ok(file) => match file.try_lock_exclusive() {
                        Ok(()) => {
                            let _ = file.unlock();
                            results.push(DoctorCheckResult::pass(
                                "Triage Lock",
                                format!(
                                    "Exclusive lock available at {}",
                                    lock_path.display()
                                ),
                            ));
                        }
                        Err(err) => results.push(DoctorCheckResult::error(
                            "Triage Lock",
                            format!(
                                "Could not acquire exclusive lock {}: {err}. Another Symphony instance may hold the triage store.",
                                lock_path.display()
                            ),
                        )),
                    },
                    Err(err) => results.push(DoctorCheckResult::error(
                        "Triage Lock",
                        format!("Could not open lock file {}: {err}", lock_path.display()),
                    )),
                }
            }
            Err(err) => results.push(DoctorCheckResult::error(
                "Triage Lock",
                format!(
                    "Could not create lock parent {}: {err}",
                    lock_parent.display()
                ),
            )),
        }
    }

    let prompt_path = resolve_prompt_path(workflow_dir, &config.triage.prompt);
    if prompt_path.is_file() {
        results.push(DoctorCheckResult::pass(
            "Triage Prompt",
            format!("Prompt file exists at {}", prompt_path.display()),
        ));
    } else {
        results.push(DoctorCheckResult::error(
            "Triage Prompt",
            format!(
                "triage.prompt points to missing file {} — create prompts/triage.md or update the path",
                prompt_path.display()
            ),
        ));
    }

    if config.triage.intake_label.trim().is_empty() {
        results.push(DoctorCheckResult::error(
            "Triage Intake",
            "triage.intake_label must be non-empty",
        ));
    } else {
        results.push(DoctorCheckResult::pass(
            "Triage Intake",
            format!("Intake label '{}'", config.triage.intake_label.trim()),
        ));
    }

    let route_labels = [
        ("implement", config.triage.routes.implement.label.trim()),
        ("spec", config.triage.routes.spec.label.trim()),
        (
            "needs_information",
            config.triage.routes.needs_information.label.trim(),
        ),
        ("park", config.triage.routes.park.label.trim()),
        ("human_owned", config.triage.routes.human_owned.label.trim()),
    ];
    let mut seen = HashSet::new();
    let mut duplicate = None;
    let mut empty_route = None;
    for (route, label) in route_labels {
        if label.is_empty() {
            empty_route = Some(route);
            break;
        }
        if !seen.insert(label.to_ascii_lowercase()) {
            duplicate = Some((route, label.to_string()));
            break;
        }
    }
    if let Some(route) = empty_route {
        results.push(DoctorCheckResult::error(
            "Triage Routes",
            format!("triage.routes.{route}.label must be non-empty"),
        ));
    } else if let Some((route, label)) = duplicate {
        results.push(DoctorCheckResult::error(
            "Triage Routes",
            format!("Route labels must be distinct; duplicate '{label}' on route '{route}'"),
        ));
    } else {
        results.push(DoctorCheckResult::pass(
            "Triage Routes",
            "Route labels are present and distinct",
        ));
    }

    let agent_command_configured =
        !config.pi_agent.command.is_empty() || !config.codex.command.is_empty();
    if !agent_command_configured {
        results.push(DoctorCheckResult::warning(
            "Triage Harness Auth",
            "isolated harness authentication: deferred deep check (agent command empty)",
        ));
    } else if !prompt_path.is_file() {
        results.push(DoctorCheckResult::warning(
            "Triage Harness Auth",
            "isolated harness authentication: deferred deep check (triage prompt missing)",
        ));
    } else {
        results.push(DoctorCheckResult::warning(
            "Triage Harness Auth",
            "isolated harness authentication: deferred deep check",
        ));
    }

    results
}

pub fn check_spec(config: &ServiceConfig, workflow_path: &Path) -> Vec<DoctorCheckResult> {
    if !config.spec.enabled {
        return vec![DoctorCheckResult::skipped(
            "Spec",
            "spec.enabled is false — spec checks skipped",
        )];
    }
    let workflow_dir = workflow_path.parent().unwrap_or(Path::new("."));
    let mut results = Vec::new();
    for (kind, configured) in [
        ("Draft", config.spec.prompts.draft.as_str()),
        ("Review", config.spec.prompts.review.as_str()),
        ("Revise", config.spec.prompts.revise.as_str()),
    ] {
        let path = resolve_prompt_path(workflow_dir, configured);
        if path.is_file() {
            results.push(DoctorCheckResult::pass(
                format!("Spec {kind} Prompt"),
                format!("Resolved {}", path.display()),
            ));
        } else {
            results.push(DoctorCheckResult::error(
                format!("Spec {kind} Prompt"),
                format!("Missing spec prompt {}", path.display()),
            ));
        }
    }
    if config.triage.enabled
        && !config
            .triage
            .routes
            .spec
            .label
            .eq_ignore_ascii_case(&config.spec.intake_label)
    {
        results.push(DoctorCheckResult::error(
            "Spec Route Consistency",
            format!(
                "triage.routes.spec.label '{}' does not match spec.intake_label '{}'",
                config.triage.routes.spec.label, config.spec.intake_label
            ),
        ));
    } else {
        results.push(DoctorCheckResult::pass(
            "Spec Route Consistency",
            "Spec intake composes with the triage spec route",
        ));
    }
    results
}

pub async fn check_triage_github(config: &ServiceConfig) -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();

    if !config.triage.enabled && !config.spec.enabled {
        return results;
    }

    let tracker_kind = config.tracker.kind.as_deref().unwrap_or("linear");
    if tracker_kind != "github" {
        results.push(DoctorCheckResult::error(
            "Triage GitHub",
            "triage.enabled requires tracker.kind=github",
        ));
        return results;
    }

    let Some(resolved_token) = resolve_github_token(&config.tracker) else {
        results.push(DoctorCheckResult::error(
            "Triage Labels",
            github_token_missing_message(),
        ));
        return results;
    };

    let Some(repo_owner) = config
        .tracker
        .repo_owner
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        results.push(DoctorCheckResult::error(
            "Triage Labels",
            "tracker.repo_owner is required for triage label checks",
        ));
        return results;
    };
    let Some(repo_name) = config
        .tracker
        .repo_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        results.push(DoctorCheckResult::error(
            "Triage Labels",
            "tracker.repo_name is required for triage label checks",
        ));
        return results;
    };

    let label_prefix = config
        .tracker
        .label_prefix
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("symphony");
    let endpoint = {
        let endpoint = config.tracker.endpoint.trim();
        if endpoint.is_empty() {
            "https://api.github.com"
        } else {
            endpoint
        }
    };

    let client = GithubClient::with_base_url(
        resolved_token.token,
        repo_owner.to_string(),
        repo_name.to_string(),
        label_prefix.to_string(),
        endpoint,
    );

    let expected_labels: Vec<String> = {
        let mut labels = Vec::new();
        if config.triage.enabled {
            labels.push(config.triage.intake_label.trim().to_string());
            labels.extend(config.triage.routes.managed_labels());
        }
        if config.spec.enabled {
            labels.extend(config.spec.managed_labels());
        }
        labels
            .into_iter()
            .filter(|label| !label.trim().is_empty())
            .collect()
    };

    match client.list_labels().await {
        Ok(labels) => {
            let normalized: BTreeSet<String> = labels
                .iter()
                .map(|label| label.name.trim().to_ascii_lowercase())
                .filter(|label| !label.is_empty())
                .collect();
            let mut missing = Vec::new();
            for expected in &expected_labels {
                if !normalized.contains(&expected.to_ascii_lowercase()) {
                    missing.push(expected.clone());
                }
            }
            if missing.is_empty() {
                results.push(DoctorCheckResult::pass(
                    "Triage Labels",
                    "Intake and route labels exist on the repository",
                ));
            } else {
                for label in missing {
                    results.push(DoctorCheckResult::error(
                        "Triage Labels",
                        format!(
                            "Label '{label}' not found on repository — create it before enabling triage"
                        ),
                    ));
                }
            }
        }
        Err(err) => results.push(DoctorCheckResult::error(
            "Triage Labels",
            format!("Failed to list repository labels: {err}"),
        )),
    }

    let configured_states: Vec<String> = [
        config.triage.routes.implement.state.as_deref(),
        config.triage.routes.spec.state.as_deref(),
        config.triage.routes.needs_information.state.as_deref(),
        config.triage.routes.park.state.as_deref(),
        config.triage.routes.human_owned.state.as_deref(),
        config.spec.approval_route.state.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_string)
    .collect();

    if configured_states.is_empty() {
        results.push(DoctorCheckResult::skipped(
            "Triage Project States",
            "No route project states configured",
        ));
        return results;
    }

    let Some(project_number) = config.tracker.github_project_number else {
        results.push(DoctorCheckResult::error(
            "Triage Project States",
            "Route states are configured but tracker.github_project_number is missing",
        ));
        return results;
    };

    let projects_client = ProjectsV2Client::new(client);
    match projects_client
        .resolve_status_field(repo_owner, project_number)
        .await
    {
        Ok(status_field) => {
            let option_names: BTreeSet<String> = status_field
                .options
                .iter()
                .map(|option| option.name.trim().to_ascii_lowercase())
                .collect();
            let mut missing = Vec::new();
            for state in &configured_states {
                if !option_names.contains(&state.to_ascii_lowercase()) {
                    missing.push(state.clone());
                }
            }
            if missing.is_empty() {
                results.push(DoctorCheckResult::pass(
                    "Triage Project States",
                    "Configured route states exist on the Projects v2 Status field",
                ));
            } else {
                for state in missing {
                    results.push(DoctorCheckResult::error(
                        "Triage Project States",
                        format!(
                            "Projects v2 Status option '{state}' is missing — add it to the project or update triage.routes.*.state"
                        ),
                    ));
                }
            }
        }
        Err(err) => results.push(DoctorCheckResult::error(
            "Triage Project States",
            format!("Failed to resolve Projects v2 Status field: {err}"),
        )),
    }

    results
}

pub fn format_results(results: &[DoctorCheckResult]) -> String {
    let mut lines = vec!["Symphony Doctor".to_string()];

    for result in results {
        let mut line = format!(
            "{} {}: {}",
            result.status.icon(),
            result.name,
            result.message
        );
        if let Some(details) = result
            .details
            .as_deref()
            .filter(|details| !details.is_empty())
        {
            line.push_str(&format!(" ({details})"));
        }
        lines.push(line);
    }

    lines.join("\n")
}

pub fn has_errors(results: &[DoctorCheckResult]) -> bool {
    results
        .iter()
        .any(|result| matches!(result.status, CheckStatus::Error))
}

pub fn load_service_config(workflow_path: &Path) -> Result<ServiceConfig, String> {
    let workflow_definition =
        workflow::parse_workflow(workflow_path).map_err(|err| format!("{err}"))?;
    config::from_workflow(&workflow_definition.config).map_err(|err| format!("{err}"))
}

fn format_linear_error(prefix: &str, err: &SymphonyError) -> String {
    match err {
        SymphonyError::LinearApiStatus(status) => format!("{prefix}: HTTP {status}"),
        other => format!("{prefix}: {other}"),
    }
}

fn graphql_error_message(body: &JsonValue) -> Option<String> {
    let errors = body.get("errors")?.as_array()?;
    if errors.is_empty() {
        return None;
    }

    let messages = errors
        .iter()
        .filter_map(|error| {
            error
                .get("message")
                .and_then(|message| message.as_str())
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();

    if messages.is_empty() {
        Some("GraphQL error response".to_string())
    } else {
        Some(messages.join("; "))
    }
}

async fn resolve_assignee(
    client: LinearClient,
    assignee: &str,
) -> crate::error::Result<Option<String>> {
    let lookup = assignee.trim().to_ascii_lowercase();
    if lookup.is_empty() {
        return Ok(None);
    }

    let mut after: Option<String> = None;

    loop {
        let mut variables = serde_json::json!({
            "first": 100,
        });
        if let Some(cursor) = after.as_ref() {
            variables["after"] = JsonValue::String(cursor.clone());
        }

        let body = client.graphql_raw(USERS_QUERY, variables).await?;
        if graphql_error_message(&body).is_some() {
            return Ok(None);
        }

        let nodes = body
            .get("data")
            .and_then(|data| data.get("users"))
            .and_then(|users| users.get("nodes"))
            .and_then(|nodes| nodes.as_array())
            .cloned()
            .unwrap_or_default();

        for node in nodes {
            let user_id = node
                .get("id")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);

            let Some(user_id) = user_id else {
                continue;
            };

            let mut candidates = vec![user_id.to_ascii_lowercase()];
            if let Some(display_name) = node
                .get("displayName")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                candidates.push(display_name.to_ascii_lowercase());
            }
            if let Some(name) = node
                .get("name")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                candidates.push(name.to_ascii_lowercase());
            }
            if let Some(email) = node
                .get("email")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                candidates.push(email.to_ascii_lowercase());
            }

            if candidates.iter().any(|candidate| candidate == &lookup) {
                return Ok(Some(user_id));
            }
        }

        let page_info = body
            .get("data")
            .and_then(|data| data.get("users"))
            .and_then(|users| users.get("pageInfo"));
        let has_next_page = page_info
            .and_then(|page_info| page_info.get("hasNextPage"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        if has_next_page {
            after = page_info
                .and_then(|page_info| page_info.get("endCursor"))
                .and_then(|value| value.as_str())
                .map(str::to_string);

            if after.is_none() {
                break;
            }
        } else {
            break;
        }
    }

    Ok(None)
}

fn normalize_state_for_label(state_name: &str) -> String {
    state_name
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

fn collect_unresolved_env_refs(value: &YamlValue) -> Vec<EnvReferenceIssue> {
    let mut issues = Vec::new();
    collect_unresolved_env_refs_inner(value, "config", &mut issues);
    issues
}

fn collect_unresolved_env_refs_inner(
    value: &YamlValue,
    path: &str,
    issues: &mut Vec<EnvReferenceIssue>,
) {
    match value {
        YamlValue::Mapping(mapping) => {
            for (key, nested_value) in mapping {
                let key = yaml_key_to_string(key);
                let nested_path = format!("{path}.{key}");
                collect_unresolved_env_refs_inner(nested_value, &nested_path, issues);
            }
        }
        YamlValue::Sequence(items) => {
            for (idx, item) in items.iter().enumerate() {
                let nested_path = format!("{path}[{idx}]");
                collect_unresolved_env_refs_inner(item, &nested_path, issues);
            }
        }
        YamlValue::String(value) => {
            if let Some(var_name) = env_reference_name(value) {
                let resolved = std::env::var(var_name).unwrap_or_default();
                if resolved.trim().is_empty() {
                    issues.push(EnvReferenceIssue {
                        path: path.to_string(),
                        var_name: var_name.to_string(),
                    });
                }
            }
        }
        _ => {}
    }
}

fn collect_invalid_slack_events(value: &YamlValue) -> Vec<String> {
    let mut invalid = Vec::new();

    let notifications = yaml_mapping_get(value, "notifications");
    let slack = notifications.and_then(|value| yaml_mapping_get(value, "slack"));
    let events = slack.and_then(|value| yaml_mapping_get(value, "events"));

    let Some(YamlValue::Sequence(events)) = events else {
        return invalid;
    };

    for event in events {
        if let Some(event_name) = event.as_str() {
            let normalized = event_name.trim().to_ascii_lowercase();
            if !normalized.is_empty() && !notifications::is_supported_slack_event(&normalized) {
                invalid.push(normalized);
            }
        }
    }

    invalid
}

fn is_required_env_reference_path(path: &str) -> bool {
    matches!(
        path,
        "config.tracker.api_key" | "config.tracker.project_slug"
    )
}

fn yaml_mapping_get<'a>(value: &'a YamlValue, key: &str) -> Option<&'a YamlValue> {
    let map = value.as_mapping()?;
    map.get(YamlValue::String(key.to_string()))
}

fn yaml_key_to_string(value: &YamlValue) -> String {
    match value {
        YamlValue::String(value) => value.clone(),
        YamlValue::Bool(value) => value.to_string(),
        YamlValue::Number(value) => value.to_string(),
        YamlValue::Null => "null".to_string(),
        other => format!("{other:?}"),
    }
}

fn env_reference_name(value: &str) -> Option<&str> {
    let variable = value.strip_prefix('$')?;
    let is_bare_identifier = !variable.is_empty()
        && !variable.contains('/')
        && !variable.contains(' ')
        && !variable.contains(':');

    if is_bare_identifier {
        Some(variable)
    } else {
        None
    }
}

fn collect_prompt_paths(config: &ServiceConfig) -> Vec<(String, String)> {
    let mut paths = Vec::new();
    let Some(prompts) = config.prompts.as_ref() else {
        return paths;
    };

    if let Some(path) = prompts.system.as_ref() {
        paths.push(("prompts.system".to_string(), path.clone()));
    }
    if let Some(path) = prompts.repo.as_ref() {
        paths.push(("prompts.repo".to_string(), path.clone()));
    }
    if let Some(path) = prompts.shared.as_ref() {
        paths.push(("prompts.shared".to_string(), path.clone()));
    }
    if let Some(path) = prompts.default.as_ref() {
        paths.push(("prompts.default".to_string(), path.clone()));
    }

    let mut by_state_entries = prompts.by_state.iter().collect::<Vec<_>>();
    by_state_entries.sort_by(|a, b| a.0.cmp(b.0));
    for (state, path) in by_state_entries {
        paths.push((format!("prompts.by_state.{state}"), path.clone()));
    }

    paths
}

fn resolve_prompt_path(workflow_dir: &Path, prompt_path: &str) -> PathBuf {
    let path = Path::new(prompt_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workflow_dir.join(path)
    }
}

fn assert_directory_writable(path: &Path) -> std::io::Result<()> {
    let probe_path = path.join(format!(
        ".symphony-doctor-write-probe-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));

    struct ProbeCleanup(PathBuf);

    impl Drop for ProbeCleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    let cleanup = ProbeCleanup(probe_path.clone());
    fs::write(&probe_path, b"doctor")?;
    fs::remove_file(&probe_path)?;
    std::mem::forget(cleanup);
    Ok(())
}

fn remote_repo_format_is_valid(repo: &str) -> bool {
    if repo.contains("://") {
        return reqwest::Url::parse(repo).is_ok();
    }

    if let Some((host, path)) = repo.split_once(':') {
        return !host.trim().is_empty() && !path.trim().is_empty();
    }

    repo.contains('@')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_results_renders_traffic_lights() {
        let results = vec![
            DoctorCheckResult::pass("Config", "ok"),
            DoctorCheckResult::warning("Prompts", "missing file"),
            DoctorCheckResult::error("Linear", "auth failed"),
            DoctorCheckResult::skipped("SSH", "not implemented"),
        ];

        let rendered = format_results(&results);
        assert!(rendered.contains("✅ Config: ok"));
        assert!(rendered.contains("⚠️ Prompts: missing file"));
        assert!(rendered.contains("🚨 Linear: auth failed"));
        assert!(rendered.contains("⏭️ SSH: not implemented"));
    }

    #[test]
    fn test_has_errors_true_when_error_present() {
        let results = vec![
            DoctorCheckResult::pass("Config", "ok"),
            DoctorCheckResult::error("Linear", "failed"),
        ];

        assert!(has_errors(&results));
    }

    #[test]
    fn test_has_errors_false_when_only_warnings() {
        let results = vec![
            DoctorCheckResult::pass("Config", "ok"),
            DoctorCheckResult::warning("Prompts", "missing"),
        ];

        assert!(!has_errors(&results));
    }

    #[test]
    fn test_check_config_catches_invalid_yaml() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workflow_path = temp_dir.path().join("WORKFLOW.md");
        fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: linear\n  api_key: [broken\n---\nbody",
        )
        .expect("write invalid workflow");

        let results = check_config(&workflow_path);
        assert!(has_errors(&results));
        assert!(results.iter().any(|result| {
            result.status == CheckStatus::Error
                && result.name == "Config Parse"
                && result.message.contains("YAML parse error")
        }));
    }

    #[test]
    fn test_check_config_optional_env_reference_is_warning() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workflow_path = temp_dir.path().join("WORKFLOW.md");
        fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: linear\n  api_key: test-token\n  project_slug: test-project\n  assignee: $SYMPHONY_DOCTOR_OPTIONAL_ENV_UNSET_123\n---\nbody",
        )
        .expect("write workflow");

        let results = check_config(&workflow_path);
        assert!(!has_errors(&results));
        assert!(results.iter().any(|result| {
            result.status == CheckStatus::Warning
                && result.name == "Config Env"
                && result.message.contains("config.tracker.assignee")
        }));
    }

    #[test]
    fn test_check_config_invalid_slack_event_is_fatal() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workflow_path = temp_dir.path().join("WORKFLOW.md");
        fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: linear\n  api_key: test-token\n  project_slug: test-project\nnotifications:\n  slack:\n    webhook_url: https://hooks.slack.com/services/test\n    events:\n      - staleled\n---\nbody",
        )
        .expect("write workflow");

        let results = check_config(&workflow_path);
        assert!(has_errors(&results));
        assert!(results.iter().any(|result| {
            result.status == CheckStatus::Warning
                && result.name == "Config Notifications"
                && result.message.contains("staleled")
        }));
        assert!(results.iter().any(|result| {
            result.status == CheckStatus::Error
                && result.name == "Config Parse"
                && result
                    .message
                    .contains("notifications.slack.events contains unsupported value")
        }));
    }

    #[test]
    fn test_check_triage_skipped_when_disabled() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workflow_path = temp_dir.path().join("WORKFLOW.md");
        fs::write(&workflow_path, "---\ntracker:\n  kind: github\n---\n").expect("write");
        let mut config = ServiceConfig::default();
        config.triage.enabled = false;

        let results = check_triage(&config, &workflow_path);
        assert!(results
            .iter()
            .any(|result| { result.status == CheckStatus::Skipped && result.name == "Triage" }));
    }

    #[test]
    fn test_check_triage_reports_missing_prompt() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workflow_path = temp_dir.path().join("WORKFLOW.md");
        fs::write(&workflow_path, "---\ntracker:\n  kind: github\n---\n").expect("write");
        let mut config = ServiceConfig::default();
        config.triage.enabled = true;
        config.tracker.kind = Some("github".to_string());
        config.tracker.repo_owner = Some("acme".to_string());
        config.tracker.repo_name = Some("widgets".to_string());
        config.storage.path = Some(
            temp_dir
                .path()
                .join("state")
                .join("triage.sqlite3")
                .to_string_lossy()
                .to_string(),
        );

        let results = check_triage(&config, &workflow_path);
        assert!(results.iter().any(|result| {
            result.status == CheckStatus::Error
                && result.name == "Triage Prompt"
                && result.message.contains("missing file")
        }));
        assert!(results.iter().any(|result| {
            result.status == CheckStatus::Pass && result.name == "Triage Storage"
        }));
        assert!(results.iter().any(|result| {
            result.status == CheckStatus::Pass && result.name == "Triage Routes"
        }));
    }
}
