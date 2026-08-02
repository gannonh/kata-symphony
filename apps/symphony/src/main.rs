use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use symphony::domain::ServiceConfig;

#[cfg(not(test))]
use symphony::doctor;
#[cfg(not(test))]
use symphony::orchestrator::OrchestratorPort;
#[cfg(not(test))]
use symphony::workflow_store::WorkflowStore;
#[cfg(not(test))]
use symphony::{config, error};

#[cfg(not(test))]
use std::future::{pending, Future};
#[cfg(not(test))]
use std::io::Write;
#[cfg(not(test))]
use std::process::Command;
#[cfg(not(test))]
use std::sync::{Arc, Mutex, Once};
#[cfg(not(test))]
use std::time::Duration;
#[cfg(not(test))]
use symphony::domain::Issue;
#[cfg(not(test))]
use symphony::github::adapter::GithubAdapter;
#[cfg(not(test))]
use symphony::github::client::GithubClient;
#[cfg(not(test))]
use symphony::http_server::{
    bind_http_listener_with_fallback, start_http_server, HttpServerState, HTTP_PORT_RETRY_LIMIT,
};
#[cfg(not(test))]
use symphony::linear::adapter::{LinearAdapter, TrackerAdapter};
#[cfg(not(test))]
use symphony::linear::client::LinearClient;
#[cfg(not(test))]
use symphony::logging;
#[cfg(not(test))]
use symphony::orchestrator::Orchestrator;
#[cfg(not(test))]
use symphony::tui;
#[cfg(not(test))]
use tokio::net::TcpListener;
#[cfg(not(test))]
use tracing_appender::non_blocking::WorkerGuard;
#[cfg(not(test))]
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub enum CliCommand {
    /// Initialize a project-local .symphony directory
    Init {
        /// Overwrite existing starter files without backups
        #[arg(long)]
        force: bool,
    },
    /// Run preflight diagnostics without starting the orchestrator
    Doctor {
        /// Path to WORKFLOW.md
        workflow_path: Option<String>,
    },
    /// Run a backend-neutral Symphony helper operation for worker sessions
    Helper {
        /// Helper operation, such as issue.get or issue.update-state
        operation: String,
        /// Path to WORKFLOW.md
        #[arg(long, default_value = "WORKFLOW.md")]
        workflow: String,
        /// JSON input file for the helper operation
        #[arg(long)]
        input: Option<String>,
    },
    /// Inspect and recover implementation publication intents
    Publication {
        #[command(subcommand)]
        action: PublicationAction,
    },
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub enum PublicationAction {
    /// List publication intents blocked after exhausting reconcile retries
    ListBlocked {
        /// Path to WORKFLOW.md (defaults to .symphony/WORKFLOW.md, then WORKFLOW.md)
        #[arg(long)]
        workflow: Option<String>,
    },
    /// Return a blocked publication intent to pending so reconciliation resumes
    ///
    /// Completed publication steps are preserved, so publication continues from
    /// where it stopped. Fix the underlying cause first — a reset intent that
    /// still cannot publish will simply exhaust its retries again.
    Reset {
        /// Intent id, as reported by `symphony publication list-blocked`
        intent_id: String,
        /// Path to WORKFLOW.md (defaults to .symphony/WORKFLOW.md, then WORKFLOW.md)
        #[arg(long)]
        workflow: Option<String>,
    },
}

#[derive(Parser, Debug, Clone)]
#[command(
    name = "symphony",
    about = "Symphony orchestrator — polls a tracker and dispatches agent sessions"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,

    /// Path to WORKFLOW.md
    pub workflow_path: Option<String>,

    /// HTTP server port (overrides WORKFLOW.md server.port; defaults to 8080 if neither is set)
    #[arg(long)]
    pub port: Option<u16>,

    /// Log file root directory
    #[arg(long)]
    pub logs_root: Option<String>,

    /// Legacy compatibility flag. TUI is now enabled by default.
    #[arg(long, hide = true)]
    pub tui: bool,

    /// Disable the live terminal dashboard (Ratatui)
    #[arg(long)]
    pub no_tui: bool,
}

const SYMPHONY_LOG_ENV: &str = "SYMPHONY_LOG";
const LEGACY_RUST_LOG_ENV: &str = "RUST_LOG";
const SYMPHONY_LOG_ROOT_ENV: &str = "SYMPHONY_LOG_ROOT";

pub(crate) fn apply_env_defaults(cli: &mut Cli) {
    if cli.logs_root.is_none() {
        if let Ok(logs_root) = std::env::var(SYMPHONY_LOG_ROOT_ENV) {
            let logs_root = logs_root.trim();
            if !logs_root.is_empty() {
                cli.logs_root = Some(logs_root.to_string());
            }
        }
    }
}

pub(crate) fn resolve_log_filter_directive() -> String {
    env_var_non_empty(SYMPHONY_LOG_ENV)
        .or_else(|| env_var_non_empty(LEGACY_RUST_LOG_ENV))
        .unwrap_or_else(|| "info".to_string())
}

fn env_var_non_empty(key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub trait BootstrapDeps {
    fn workflow_exists(&mut self, workflow_path: &Path) -> bool;
    fn startup_validate(&mut self, workflow_path: &Path) -> Result<(), String>;
    fn start_orchestrator(&mut self, workflow_path: &Path, cli: &Cli) -> Result<(), String>;
}

#[cfg(not(test))]
struct StartupContext {
    workflow_path: PathBuf,
    workflow_store: WorkflowStore,
    effective_config: ServiceConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpBinding {
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[cfg(not(test))]
struct PreparedHttpServer {
    host: String,
    configured_port: u16,
    bound_port: u16,
    banner_binding: HttpBinding,
    listener: TcpListener,
}

#[cfg(not(test))]
struct LinearOrchestratorPort {
    workflow_store: Arc<WorkflowStore>,
}

#[cfg(not(test))]
impl LinearOrchestratorPort {
    fn new(workflow_store: Arc<WorkflowStore>) -> Self {
        Self { workflow_store }
    }

    fn block_on<T>(&self, future: impl Future<Output = error::Result<T>>) -> error::Result<T> {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
    }

    fn tracker_adapter(&self) -> LinearAdapter {
        let (_, effective_config) = self.workflow_store.effective_config();
        LinearAdapter::new(LinearClient::new(effective_config.tracker))
    }
}

#[cfg(not(test))]
impl OrchestratorPort for LinearOrchestratorPort {
    fn startup_terminal_issues(&mut self, terminal_states: &[String]) -> error::Result<Vec<Issue>> {
        let adapter = self.tracker_adapter();
        self.block_on(adapter.fetch_issues_by_states(terminal_states))
    }

    fn reconcile_running_issues(
        &mut self,
        running_issue_ids: &[String],
    ) -> error::Result<Vec<Issue>> {
        if running_issue_ids.is_empty() {
            return Ok(vec![]);
        }

        let adapter = self.tracker_adapter();
        self.block_on(adapter.fetch_issue_states_by_ids(running_issue_ids))
    }

    fn validate_dispatch_preflight(&mut self, config: &ServiceConfig) -> error::Result<()> {
        config::validate(config).map(|_| ())
    }

    fn fetch_candidate_issues(&mut self) -> error::Result<Vec<Issue>> {
        let adapter = self.tracker_adapter();
        self.block_on(adapter.fetch_candidate_issues())
    }

    fn refresh_issue(&mut self, issue_id: &str) -> error::Result<Option<Issue>> {
        let adapter = self.tracker_adapter();
        let issue_ids = vec![issue_id.to_string()];
        let issues = self.block_on(adapter.fetch_issue_states_by_ids(&issue_ids))?;
        Ok(issues.into_iter().next())
    }

    fn create_issue_comment(&mut self, issue_id: &str, body: &str) -> error::Result<()> {
        let adapter = self.tracker_adapter();
        self.block_on(adapter.create_comment(issue_id, body))
    }

    fn update_issue_state(&mut self, issue_id: &str, state_name: &str) -> error::Result<()> {
        let adapter = self.tracker_adapter();
        self.block_on(adapter.update_issue_state(issue_id, state_name))
    }
}

#[cfg(not(test))]
struct GithubOrchestratorPort {
    workflow_store: Arc<WorkflowStore>,
    cached_adapter: Option<GithubAdapter>,
    adapter_cache_key: Option<String>,
}

#[cfg(not(test))]
impl GithubOrchestratorPort {
    fn new(workflow_store: Arc<WorkflowStore>) -> Self {
        Self {
            workflow_store,
            cached_adapter: None,
            adapter_cache_key: None,
        }
    }

    fn block_on<T>(future: impl Future<Output = error::Result<T>>) -> error::Result<T> {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
    }

    fn build_adapter_cache_key(
        tracker: &symphony::domain::TrackerConfig,
        token: &str,
        repo_owner: &str,
        repo_name: &str,
        label_prefix: &str,
        endpoint: &str,
    ) -> String {
        format!(
            "{token}\u{1e}{repo_owner}\u{1e}{repo_name}\u{1e}{label_prefix}\u{1e}{endpoint}\u{1e}{project_number}\u{1e}{assignee}\u{1e}{active_states}\u{1e}{terminal_states}\u{1e}{exclude_labels}",
            project_number = tracker
                .github_project_number
                .map(|value| value.to_string())
                .unwrap_or_default(),
            assignee = tracker.assignee.as_deref().unwrap_or_default(),
            active_states = tracker.active_states.join("\u{1f}"),
            terminal_states = tracker.terminal_states.join("\u{1f}"),
            exclude_labels = tracker.exclude_labels.join("\u{1f}"),
        )
    }

    fn tracker_adapter(&mut self) -> error::Result<&GithubAdapter> {
        let (_, effective_config) = self.workflow_store.effective_config();
        let tracker = effective_config.tracker;

        let inputs = symphony::helper::github_adapter_inputs(&tracker)?;

        let cache_key = Self::build_adapter_cache_key(
            &tracker,
            &inputs.token,
            &inputs.repo_owner,
            &inputs.repo_name,
            &inputs.label_prefix,
            inputs.endpoint.as_str(),
        );

        let should_refresh = self.adapter_cache_key.as_deref() != Some(cache_key.as_str());
        if should_refresh {
            let client = GithubClient::with_base_url(
                inputs.token,
                inputs.repo_owner,
                inputs.repo_name,
                inputs.label_prefix,
                inputs.endpoint.as_str(),
            );
            self.cached_adapter = Some(GithubAdapter::new(client, tracker));
            self.adapter_cache_key = Some(cache_key);
        }

        self.cached_adapter.as_ref().ok_or_else(|| {
            error::SymphonyError::InvalidWorkflowConfig(
                "failed to initialize github tracker adapter".to_string(),
            )
        })
    }
}

#[cfg(not(test))]
impl OrchestratorPort for GithubOrchestratorPort {
    fn startup_terminal_issues(&mut self, terminal_states: &[String]) -> error::Result<Vec<Issue>> {
        let adapter = self.tracker_adapter()?;
        Self::block_on(adapter.fetch_issues_by_states(terminal_states))
    }

    fn reconcile_running_issues(
        &mut self,
        running_issue_ids: &[String],
    ) -> error::Result<Vec<Issue>> {
        if running_issue_ids.is_empty() {
            return Ok(vec![]);
        }

        let adapter = self.tracker_adapter()?;
        Self::block_on(adapter.fetch_issue_states_by_ids(running_issue_ids))
    }

    fn validate_dispatch_preflight(&mut self, config: &ServiceConfig) -> error::Result<()> {
        config::validate(config).map(|_| ())
    }

    fn fetch_candidate_issues(&mut self) -> error::Result<Vec<Issue>> {
        let adapter = self.tracker_adapter()?;
        Self::block_on(adapter.fetch_candidate_issues())
    }

    fn refresh_issue(&mut self, issue_id: &str) -> error::Result<Option<Issue>> {
        let adapter = self.tracker_adapter()?;
        let issue_ids = vec![issue_id.to_string()];
        let issues = Self::block_on(adapter.fetch_issue_states_by_ids(&issue_ids))?;
        Ok(issues.into_iter().next())
    }

    fn create_issue_comment(&mut self, issue_id: &str, body: &str) -> error::Result<()> {
        let adapter = self.tracker_adapter()?;
        Self::block_on(adapter.create_comment(issue_id, body))
    }

    fn update_issue_state(&mut self, issue_id: &str, state_name: &str) -> error::Result<()> {
        let adapter = self.tracker_adapter()?;
        Self::block_on(adapter.update_issue_state(issue_id, state_name))
    }
}

#[cfg(not(test))]
#[derive(Default)]
pub struct RuntimeBootstrapDeps {
    startup_context: Option<StartupContext>,
}

#[cfg(not(test))]
impl RuntimeBootstrapDeps {
    fn load_startup_context(workflow_path: &Path) -> Result<StartupContext, String> {
        let workflow_store = WorkflowStore::new(workflow_path)
            .map_err(|err| format!("failed to load workflow store: {err}"))?;

        let (_, effective_config) = workflow_store.effective_config();

        Ok(StartupContext {
            workflow_path: workflow_path.to_path_buf(),
            workflow_store,
            effective_config,
        })
    }

    fn take_or_load_validated_context(
        &mut self,
        workflow_path: &Path,
    ) -> Result<StartupContext, String> {
        if let Some(context) = self.startup_context.take() {
            if context.workflow_path == workflow_path {
                return Ok(context);
            }
        }

        let context = Self::load_startup_context(workflow_path)?;
        config::validate(&context.effective_config)
            .map_err(|err| format!("invalid startup config: {err}"))?;
        Ok(context)
    }
}

#[cfg(not(test))]
impl BootstrapDeps for RuntimeBootstrapDeps {
    fn workflow_exists(&mut self, workflow_path: &Path) -> bool {
        workflow_path.is_file()
    }

    fn startup_validate(&mut self, workflow_path: &Path) -> Result<(), String> {
        tracing::info!(
            phase = "startup",
            stage = "validate",
            workflow_path = %workflow_path.display(),
            "validating startup workflow and config"
        );

        let context = Self::load_startup_context(workflow_path)?;
        config::validate(&context.effective_config)
            .map_err(|err| format!("invalid startup config: {err}"))?;

        self.startup_context = Some(context);

        tracing::info!(
            phase = "startup",
            stage = "validate",
            workflow_path = %workflow_path.display(),
            "startup workflow and config validation succeeded"
        );

        Ok(())
    }

    fn start_orchestrator(&mut self, workflow_path: &Path, cli: &Cli) -> Result<(), String> {
        let context = self.take_or_load_validated_context(workflow_path)?;
        let prepared_http_server =
            prepare_http_server_binding(effective_http_binding(&context.effective_config, cli))?;
        if cli.tui {
            tui::validate_terminal_for_tui()
                .map_err(|err| format!("tui preflight failed: {err}"))?;
        }
        if !cli.tui {
            print_startup_banner(
                cli,
                &context.effective_config,
                prepared_http_server
                    .as_ref()
                    .map(|server| &server.banner_binding),
            );
        }

        let tracker_kind = context
            .effective_config
            .tracker
            .kind
            .clone()
            .unwrap_or_else(|| "linear".to_string());

        let workflow_store = Arc::new(context.workflow_store);
        let mut tracker_port: Box<dyn OrchestratorPort> = if tracker_kind == "github" {
            Box::new(GithubOrchestratorPort::new(Arc::clone(&workflow_store)))
        } else {
            Box::new(LinearOrchestratorPort::new(Arc::clone(&workflow_store)))
        };

        tracing::info!(tracker_kind = %tracker_kind, "orchestrator port selected");

        let server_port_override = prepared_http_server
            .as_ref()
            .map(|server| server.bound_port)
            .or(cli.port);
        let mut orchestrator = Orchestrator::new_with_workflow_store_and_port_override(
            Arc::clone(&workflow_store),
            server_port_override,
        );

        let snapshot_handle = orchestrator.create_snapshot_handle();
        let tui_snapshot_handle = snapshot_handle.clone();
        let refresh_sender = orchestrator.create_refresh_channel();
        let event_hub = orchestrator.create_event_hub();
        let tui_event_hub = event_hub.clone();
        let triage_event_hub = event_hub.clone();
        let steer_sender = orchestrator.create_steer_sender();

        match symphony::triage::runtime::TriageRuntime::try_start(
            &context.effective_config,
            workflow_path,
            Some(triage_event_hub),
        ) {
            Ok(Some(runtime)) => {
                tracing::info!(
                    event = "triage_runtime_started",
                    mode = %context.effective_config.triage.mode,
                    "A1 triage runtime attached"
                );
                orchestrator.attach_triage_runtime(runtime);
            }
            Ok(None) => {
                tracing::info!(
                    event = "triage_runtime_disabled",
                    "triage.enabled is false; triage runtime not started"
                );
                match symphony::triage::runtime::TriageRuntime::try_open_dispatch_guard_store(
                    &context.effective_config,
                ) {
                    Ok(Some(store)) => {
                        orchestrator.attach_dispatch_guard_store(store);
                    }
                    Ok(None) => {}
                    Err(err) => {
                        return Err(format!(
                            "failed to open triage dispatch guard store while triage is disabled: {err}"
                        ));
                    }
                }
            }
            Err(err) => {
                return Err(format!("failed to start triage runtime: {err}"));
            }
        }

        let mut http_state = HttpServerState::with_event_stream(
            Arc::new(snapshot_handle),
            Arc::new(refresh_sender),
            orchestrator.escalation_registry(),
            event_hub,
            symphony::http_server::EventStreamConfig::default(),
        )
        .with_shared_context_store(orchestrator.shared_context_store())
        .with_steer_sender(steer_sender);
        if let Some(query) = orchestrator.take_triage_factory_query() {
            http_state = http_state.with_factory_run_query(query);
        }

        let mut tui_shutdown = None;
        let mut tui_exit = None;
        let mut tui_task = None;
        if cli.tui {
            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
            let (exit_tx, exit_rx) = tokio::sync::watch::channel(None::<tui::TuiExitReason>);
            tui_shutdown = Some(shutdown_tx);
            tui_exit = Some(exit_rx);
            tui_task = Some(tokio::spawn(async move {
                let reason = tui::run_tui(tui_snapshot_handle, tui_event_hub, shutdown_rx).await;
                let _ = exit_tx.send(Some(reason));
            }));
        }

        tracing::info!(
            phase = "startup",
            stage = "runtime_init",
            workflow_path = %workflow_path.display(),
            http_enabled = prepared_http_server.is_some(),
            http_host = prepared_http_server.as_ref().map(|server| server.host.as_str()).unwrap_or("n/a"),
            http_port = prepared_http_server.as_ref().map(|server| server.bound_port),
            logs_root_configured = cli.logs_root.is_some(),
            tui_enabled = cli.tui,

            "constructed orchestrator runtime"
        );

        if let Some(server) = &prepared_http_server {
            tracing::info!(
                event = "http_server_enabled",
                host = %server.host,
                configured_port = server.configured_port,
                port = server.bound_port,
                "HTTP server binding enabled at startup"
            );
        } else {
            tracing::info!(
                event = "http_server_disabled",
                reason = "no_port_configured",
                "HTTP server disabled; running orchestrator-only mode"
            );
        }

        let runtime_result = run_runtime_until_shutdown(
            &mut orchestrator,
            &mut *tracker_port,
            workflow_path,
            prepared_http_server,
            http_state,
            tui_exit,
        );

        if let Some(shutdown_tx) = tui_shutdown {
            let _ = shutdown_tx.send(true);
        }

        if let Some(task) = tui_task {
            let handle = tokio::runtime::Handle::try_current()
                .map_err(|err| format!("missing tokio runtime for tui shutdown: {err}"))?;
            tokio::task::block_in_place(|| {
                handle.block_on(async {
                    match tokio::time::timeout(Duration::from_secs(2), task).await {
                        Ok(Ok(())) => {}
                        Ok(Err(err)) => {
                            tracing::warn!(error = %err, "tui task ended with join error");
                        }
                        Err(_) => {
                            tracing::warn!("timed out waiting for tui task shutdown");
                        }
                    }
                });
            });
        }

        runtime_result
    }
}

pub fn parse_cli_from<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut cli = Cli::try_parse_from(args)?;
    // TUI is enabled by default; --no-tui is the explicit opt-out.
    cli.tui = !cli.no_tui;
    Ok(cli)
}

pub fn resolve_default_workflow_path() -> PathBuf {
    let project_home_workflow = PathBuf::from(".symphony").join("WORKFLOW.md");
    if project_home_workflow.is_file() {
        project_home_workflow
    } else {
        PathBuf::from("WORKFLOW.md")
    }
}

pub fn resolve_workflow_path(cli: &Cli) -> PathBuf {
    match &cli.command {
        Some(CliCommand::Doctor { workflow_path }) => workflow_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(resolve_default_workflow_path),
        Some(CliCommand::Helper { workflow, .. }) => PathBuf::from(workflow),
        Some(CliCommand::Publication { action }) => match action {
            PublicationAction::ListBlocked { workflow }
            | PublicationAction::Reset { workflow, .. } => workflow
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(resolve_default_workflow_path),
        },
        Some(CliCommand::Init { .. }) => resolve_default_workflow_path(),
        None => cli
            .workflow_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(resolve_default_workflow_path),
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn effective_http_binding(config: &ServiceConfig, cli: &Cli) -> Option<HttpBinding> {
    let port = cli.port.or(config.server.port).unwrap_or(8080);
    Some(HttpBinding {
        host: config.server.host.clone(),
        port,
    })
}

fn startup_banner_binding(configured_binding: &HttpBinding, bound_port: u16) -> HttpBinding {
    HttpBinding {
        host: configured_binding.host.clone(),
        port: if configured_binding.port == 0 {
            0
        } else {
            bound_port
        },
    }
}

#[cfg(not(test))]
fn prepare_http_server_binding(
    configured_binding: Option<HttpBinding>,
) -> Result<Option<PreparedHttpServer>, String> {
    let Some(configured_binding) = configured_binding else {
        return Ok(None);
    };

    let host = configured_binding.host.clone();
    let configured_port = configured_binding.port;
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|err| format!("missing tokio runtime for HTTP server bind: {err}"))?;
    let (listener, bound_port) = tokio::task::block_in_place(|| {
        runtime.block_on(bind_http_listener_with_fallback(
            &host,
            configured_port,
            HTTP_PORT_RETRY_LIMIT,
        ))
    })
    .map_err(|err| err.to_string())?;

    Ok(Some(PreparedHttpServer {
        host,
        configured_port,
        bound_port,
        banner_binding: startup_banner_binding(&configured_binding, bound_port),
        listener,
    }))
}

#[cfg_attr(test, allow(dead_code))]
fn format_polling_interval(interval_ms: u64) -> String {
    if interval_ms.is_multiple_of(1_000) {
        format!("every {}s", interval_ms / 1_000)
    } else {
        format!("every {interval_ms}ms")
    }
}

#[cfg_attr(test, allow(dead_code))]
fn display_path_with_home_alias(path: &Path) -> String {
    let home = match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home),
        Err(_) => return path.display().to_string(),
    };

    match path.strip_prefix(&home) {
        Ok(stripped) if stripped.as_os_str().is_empty() => "~".to_string(),
        Ok(stripped) => format!("~/{}", stripped.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg_attr(test, allow(dead_code))]
fn format_dashboard_url(host: &str, port: u16) -> String {
    let host = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(_)) if !host.starts_with('[') => format!("[{host}]"),
        _ if host.contains(':') && !host.starts_with('[') && !host.ends_with(']') => {
            format!("[{host}]")
        }
        _ => host.to_string(),
    };

    if port == 0 {
        format!("http://{host}:<ephemeral>")
    } else {
        format!("http://{host}:{port}")
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn build_startup_banner(
    cli: &Cli,
    config: &ServiceConfig,
    http_binding: Option<&HttpBinding>,
) -> String {
    let dashboard = http_binding
        .map(|binding| format_dashboard_url(&binding.host, binding.port))
        .unwrap_or_else(|| "disabled".to_string());

    let logs = cli
        .logs_root
        .as_deref()
        .map(|logs_root| Path::new(logs_root).join("log").join("symphony.log"))
        .map(|path| display_path_with_home_alias(&path))
        .unwrap_or_else(|| "stdout".to_string());

    let project_slug = config
        .tracker
        .project_slug
        .as_deref()
        .unwrap_or("unknown_project_slug");

    let triage = if config.triage.enabled {
        format!("enabled ({})", config.triage.mode)
    } else {
        "disabled".to_string()
    };

    format!(
        "Symphony v{version}\nDashboard: {dashboard}\nLogs: {logs}\nProject: {project_slug}\nTriage: {triage}\nWorkers: {workers} max concurrent\nPolling: {polling}\n\nPress Ctrl+C to stop.\n",
        version = env!("CARGO_PKG_VERSION"),
        workers = config.agent.max_concurrent_agents,
        polling = format_polling_interval(config.polling.interval_ms),
    )
}

#[cfg(not(test))]
fn print_startup_banner(cli: &Cli, config: &ServiceConfig, http_binding: Option<&HttpBinding>) {
    print!("{}", build_startup_banner(cli, config, http_binding));
    if let Err(err) = std::io::stdout().flush() {
        eprintln!("failed to flush startup banner to stdout: {err}");
    }
}

pub fn execute_cli(cli: &Cli, deps: &mut dyn BootstrapDeps) -> Result<(), String> {
    let workflow_path = resolve_workflow_path(cli);

    tracing::info!(
        phase = "startup",
        stage = "bootstrap",
        workflow_path = %workflow_path.display(),
        "starting CLI bootstrap"
    );

    if !deps.workflow_exists(&workflow_path) {
        return Err(format!(
            "workflow file not found: {}",
            workflow_path.display()
        ));
    }

    deps.startup_validate(&workflow_path).map_err(|err| {
        format!(
            "startup validation failed for {}: {err}",
            workflow_path.display()
        )
    })?;

    deps.start_orchestrator(&workflow_path, cli).map_err(|err| {
        format!(
            "orchestrator startup failed for {}: {err}",
            workflow_path.display()
        )
    })
}

#[cfg(not(test))]
fn run_doctor(workflow_path: &Path) -> Result<i32, String> {
    let mut results = doctor::check_config(workflow_path);

    if !doctor::has_errors(&results) {
        match doctor::load_service_config(workflow_path) {
            Ok(service_config) => {
                let runtime = tokio::runtime::Handle::try_current()
                    .map_err(|err| format!("missing tokio runtime for doctor checks: {err}"))?;

                let tracker_kind = service_config.tracker.kind.as_deref().unwrap_or("linear");

                if tracker_kind == "github" {
                    let github_results = tokio::task::block_in_place(|| {
                        runtime.block_on(doctor::check_github(&service_config.tracker))
                    });
                    results.extend(github_results);
                    let review_results = tokio::task::block_in_place(|| {
                        runtime.block_on(doctor::check_review(&service_config))
                    });
                    results.extend(review_results);

                    results.extend(doctor::check_backend(&service_config));
                    results.extend(doctor::check_workspace(&service_config.workspace));
                    results.extend(doctor::check_triage(&service_config, workflow_path));
                    results.extend(doctor::check_spec(&service_config, workflow_path));
                    results.extend(doctor::check_implementation(&service_config, workflow_path));
                    let triage_github = tokio::task::block_in_place(|| {
                        runtime.block_on(doctor::check_triage_github(&service_config))
                    });
                    results.extend(triage_github);

                    match symphony::helper::github_adapter_inputs(&service_config.tracker) {
                        Ok(inputs) => {
                            let client = GithubClient::with_base_url(
                                inputs.token,
                                inputs.repo_owner,
                                inputs.repo_name,
                                inputs.label_prefix,
                                inputs.endpoint.as_str(),
                            );
                            let adapter =
                                GithubAdapter::new(client, service_config.tracker.clone());
                            let orphan_results = tokio::task::block_in_place(|| {
                                runtime.block_on(doctor::check_orphans(&service_config, &adapter))
                            });
                            results.extend(orphan_results);
                        }
                        Err(err) => {
                            results.push(doctor::DoctorCheckResult {
                                name: "Orphans".to_string(),
                                status: doctor::CheckStatus::Error,
                                message: format!(
                                    "Failed to initialize GitHub adapter for orphan checks: {err}"
                                ),
                                details: None,
                            });
                        }
                    }
                } else {
                    let linear_results = tokio::task::block_in_place(|| {
                        runtime.block_on(doctor::check_linear(&service_config.tracker))
                    });
                    results.extend(linear_results);

                    results.extend(doctor::check_backend(&service_config));
                    results.extend(doctor::check_workspace(&service_config.workspace));
                    results.extend(doctor::check_triage(&service_config, workflow_path));
                    results.extend(doctor::check_spec(&service_config, workflow_path));
                    results.extend(doctor::check_implementation(&service_config, workflow_path));

                    let adapter =
                        LinearAdapter::new(LinearClient::new(service_config.tracker.clone()));
                    let orphan_results = tokio::task::block_in_place(|| {
                        runtime.block_on(doctor::check_orphans(&service_config, &adapter))
                    });
                    results.extend(orphan_results);
                }
            }
            Err(err) => results.push(doctor::DoctorCheckResult {
                name: "Config Parse".to_string(),
                status: doctor::CheckStatus::Error,
                message: format!("Failed to load workflow for runtime checks: {err}"),
                details: None,
            }),
        }
    } else {
        results.push(doctor::DoctorCheckResult {
            name: "Runtime Checks".to_string(),
            status: doctor::CheckStatus::Skipped,
            message: "Skipped Linear/backend/workspace/orphan checks because config check reported errors".to_string(),
            details: None,
        });
    }

    println!("{}", doctor::format_results(&results));

    if doctor::has_errors(&results) {
        Ok(1)
    } else {
        Ok(0)
    }
}

/// Load the effective workflow config for a publication recovery command.
#[cfg(not(test))]
fn load_publication_config(workflow_path: &Path) -> std::result::Result<ServiceConfig, String> {
    RuntimeBootstrapDeps::load_startup_context(workflow_path)
        .map(|context| context.effective_config)
        .map_err(|err| err.to_string())
}

/// Open the durable factory store for the workflow's configured tracker repo.
///
/// Fails while Symphony runs: the orchestrator holds an exclusive lock on the
/// store for its lifetime. Callers try the admin HTTP surface first and treat
/// this as the fallback for when nothing is listening.
#[cfg(not(test))]
fn open_factory_store_for_config(
    config: &ServiceConfig,
) -> std::result::Result<symphony::triage::runtime::SharedFactoryStore, String> {
    use symphony::triage::storage_path::{forge_host_from_endpoint, resolve_storage_path};

    let owner = config
        .tracker
        .repo_owner
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "tracker.repo_owner required".to_string())?;
    let repo = config
        .tracker
        .repo_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "tracker.repo_name required".to_string())?;
    let forge_host = forge_host_from_endpoint(&config.tracker.endpoint);
    let storage_path = resolve_storage_path(&config.storage, &forge_host, owner, repo);
    if !storage_path.exists() {
        return Err(format!(
            "no durable factory store at {}; run Symphony against this workflow first",
            storage_path.display()
        ));
    }
    symphony::triage::runtime::SharedFactoryStore::open(
        &storage_path,
        config.storage.busy_timeout_ms,
    )
    .map_err(|err| err.to_string())
}

/// The operator identity recorded on a reset, for the run timeline audit event.
#[cfg(not(test))]
fn publication_operator() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Drive a recovery request to completion on the ambient tokio runtime.
#[cfg(not(test))]
fn block_on_recovery<F: Future>(future: F) -> std::result::Result<F::Output, String> {
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|err| format!("missing tokio runtime for publication recovery: {err}"))?;
    Ok(tokio::task::block_in_place(|| runtime.block_on(future)))
}

/// Recover blocked publication intents, preferring the running orchestrator.
///
/// The store is locked by Symphony while it runs, so the admin HTTP surface is
/// tried first and the direct-store path is the fallback for when nothing
/// answers. See `symphony::publication_recovery`.
#[cfg(not(test))]
fn run_publication(action: &PublicationAction, workflow_path: &Path, cli: &Cli) -> i32 {
    use symphony::http_server::FactoryRunQuery;
    use symphony::publication_recovery as recovery;

    let config = match load_publication_config(workflow_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };
    let base_url = match effective_http_binding(&config, cli) {
        Some(binding) => recovery::admin_base_url(&binding.host, binding.port),
        None => recovery::admin_base_url("127.0.0.1", recovery::DEFAULT_ADMIN_PORT),
    };

    match action {
        PublicationAction::ListBlocked { .. } => {
            let served = match block_on_recovery(recovery::fetch_blocked_publications(&base_url)) {
                Ok(served) => served,
                Err(err) => {
                    eprintln!("{err}");
                    return 1;
                }
            };
            let http_error = match served {
                Ok(blocked) => {
                    println!("{}", recovery::format_blocked_publications(&blocked));
                    return 0;
                }
                Err(err) if err.is_unreachable() => err,
                Err(err) => {
                    eprintln!("{err}");
                    return 1;
                }
            };

            let store = match open_factory_store_for_config(&config) {
                Ok(store) => store,
                Err(store_error) => {
                    eprintln!(
                        "{}",
                        recovery::store_unavailable_message(&http_error, &store_error)
                    );
                    return 1;
                }
            };
            match store.blocked_publications() {
                Ok(blocked) => {
                    println!("{}", recovery::format_blocked_publications(&blocked));
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
        PublicationAction::Reset { intent_id, .. } => {
            let operator = publication_operator();
            let served = match block_on_recovery(recovery::reset_blocked_publication(
                &base_url, intent_id, &operator,
            )) {
                Ok(served) => served,
                Err(err) => {
                    eprintln!("{err}");
                    return 1;
                }
            };
            let http_error = match served {
                Ok(reset) => {
                    println!(
                        "{}",
                        recovery::format_publication_reset(
                            &reset,
                            recovery::RecoverySource::RunningOrchestrator
                        )
                    );
                    return 0;
                }
                Err(err) if err.is_unreachable() => err,
                Err(err) => {
                    eprintln!("{err}");
                    return 1;
                }
            };

            let store = match open_factory_store_for_config(&config) {
                Ok(store) => store,
                Err(store_error) => {
                    eprintln!(
                        "{}",
                        recovery::store_unavailable_message(&http_error, &store_error)
                    );
                    return 1;
                }
            };
            match store.reset_blocked_publication(intent_id, &operator) {
                Ok(reset) => {
                    println!(
                        "{}",
                        recovery::format_publication_reset(
                            &reset,
                            recovery::RecoverySource::DirectStore
                        )
                    );
                    0
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
    }
}

#[cfg(not(test))]
fn run_helper(workflow_path: &Path, operation: &str, input_path: Option<&str>) -> i32 {
    let input = match symphony::helper::read_helper_input(input_path) {
        Ok(input) => input,
        Err(err) => {
            println!("{}", symphony::helper::error_envelope(err));
            return 1;
        }
    };

    let context = match RuntimeBootstrapDeps::load_startup_context(workflow_path) {
        Ok(context) => context,
        Err(err) => {
            println!("{}", symphony::helper::error_envelope(err.to_string()));
            return 1;
        }
    };

    if let Err(err) = config::validate(&context.effective_config) {
        println!(
            "{}",
            symphony::helper::error_envelope(format!("invalid workflow config: {err}"))
        );
        return 1;
    }

    let tracker = context.effective_config.tracker;
    let runtime = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle,
        Err(err) => {
            println!(
                "{}",
                symphony::helper::error_envelope(format!(
                    "missing tokio runtime for helper: {err}"
                ))
            );
            return 1;
        }
    };

    let result = tokio::task::block_in_place(|| {
        runtime
            .block_on(async { symphony::helper::run_operation(&tracker, operation, input).await })
    });

    match result {
        Ok(data) => {
            println!("{}", symphony::helper::success_envelope(data));
            0
        }
        Err(err) => {
            println!("{}", symphony::helper::error_envelope(err));
            1
        }
    }
}

#[cfg(not(test))]
async fn wait_for_shutdown_signal() -> Result<&'static str, String> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = signal(SignalKind::terminate())
            .map_err(|err| format!("failed to listen for sigterm: {err}"))?;

        tokio::select! {
            ctrl_c_result = tokio::signal::ctrl_c() => {
                ctrl_c_result
                    .map(|()| "ctrl_c")
                    .map_err(|err| format!("failed to listen for ctrl_c: {err}"))
            }
            terminate_result = terminate.recv() => {
                terminate_result
                    .map(|_| "sigterm")
                    .ok_or_else(|| "sigterm signal stream ended unexpectedly".to_string())
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map(|()| "ctrl_c")
            .map_err(|err| format!("failed to listen for ctrl_c: {err}"))
    }
}

#[cfg(not(test))]
fn run_runtime_until_shutdown(
    orchestrator: &mut Orchestrator,
    port: &mut dyn OrchestratorPort,
    workflow_path: &Path,
    prepared_http_server: Option<PreparedHttpServer>,
    http_state: HttpServerState,
    mut tui_exit: Option<tokio::sync::watch::Receiver<Option<tui::TuiExitReason>>>,
) -> Result<(), String> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|err| format!("missing tokio runtime for orchestrator startup: {err}"))?;

    tokio::task::block_in_place(|| {
        handle.block_on(async {
            tracing::info!(
                phase = "runtime",
                stage = "start",
                workflow_path = %workflow_path.display(),
                http_enabled = prepared_http_server.is_some(),
                "starting orchestrator runtime"
            );

            let http_future = async {
                if let Some(server) = prepared_http_server {
                    start_http_server(
                        http_state,
                        server.listener,
                        &server.host,
                        server.configured_port,
                        server.bound_port,
                    )
                        .await
                        .map_err(|err| format!("http server failed: {err}"))
                } else {
                    pending::<Result<(), String>>().await
                }
            };
            let tui_exit_future = async {
                match tui_exit.as_mut() {
                    Some(exit_rx) => loop {
                        if let Some(reason) = *exit_rx.borrow() {
                            break Some(reason);
                        }
                        if exit_rx.changed().await.is_err() {
                            break Some(tui::TuiExitReason::ShutdownSignal);
                        }
                    },
                    None => pending::<Option<tui::TuiExitReason>>().await,
                }
            };

            let runtime_result = tokio::select! {
                run_result = orchestrator.run(port) => {
                    run_result.map_err(|err| format!("orchestrator runtime failed: {err}"))?;
                    tracing::info!(
                        phase = "runtime",
                        stage = "stopped",
                        reason = "run_returned",
                        workflow_path = %workflow_path.display(),
                        "orchestrator loop stopped"
                    );
                    Ok(())
                }
                http_result = http_future => {
                    http_result?;
                    tracing::info!(
                        phase = "runtime",
                        stage = "stopped",
                        reason = "http_server_returned",
                        workflow_path = %workflow_path.display(),
                        "HTTP server stopped"
                    );
                    Ok(())
                }
                signal_reason = wait_for_shutdown_signal() => {
                    let reason = signal_reason?;
                    tracing::info!(
                        phase = "runtime",
                        stage = "stopped",
                        reason = reason,
                        workflow_path = %workflow_path.display(),
                        "received shutdown signal"
                    );
                    Ok(())
                }
                tui_reason = tui_exit_future => {
                    match tui_reason {
                        Some(tui_reason) => match tui_reason {
                            tui::TuiExitReason::CtrlC | tui::TuiExitReason::ShutdownSignal => {
                                tracing::info!(
                                    phase = "runtime",
                                    stage = "stopped",
                                    reason = "tui_exit",
                                    workflow_path = %workflow_path.display(),
                                    tui_reason = ?tui_reason,
                                    "tui requested runtime shutdown"
                                );
                                Ok(())
                            }
                            tui::TuiExitReason::SetupFailed => Err("tui failed to initialize terminal".to_string()),
                            tui::TuiExitReason::InputError => Err("tui failed while reading terminal input".to_string()),
                            tui::TuiExitReason::DrawError => Err("tui failed while drawing dashboard".to_string()),
                        },
                        None => Ok(()),
                    }
                }
            };

            orchestrator.shutdown_supervisor().await;
            runtime_result
        })
    })
}

#[cfg(not(test))]
static FILE_LOG_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

#[cfg(not(test))]
fn flush_file_logs() {
    if let Ok(mut guard_slot) = FILE_LOG_GUARD.lock() {
        let _ = guard_slot.take();
    }
}

#[cfg(not(test))]
fn init_tracing(logs_root: Option<&Path>, tui_enabled: bool) {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let filter = EnvFilter::try_new(resolve_log_filter_directive())
            .unwrap_or_else(|_| EnvFilter::new("info"));
        let subscriber_builder = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .json();

        let init_result = match logs_root {
            Some(logs_root_path) => match logging::build_non_blocking_file_writer(logs_root_path) {
                Ok((file_writer, guard)) => match FILE_LOG_GUARD.lock() {
                    Ok(mut guard_slot) => {
                        *guard_slot = Some(guard);
                        subscriber_builder.with_writer(file_writer).try_init()
                    }
                    Err(err) => {
                        eprintln!(
                            "failed to store file log guard (mutex poisoned): {err}; file logging disabled"
                        );
                        if tui_enabled {
                            subscriber_builder.with_writer(std::io::sink).try_init()
                        } else {
                            subscriber_builder.try_init()
                        }
                    }
                },
                Err(err) => {
                    eprintln!(
                        "failed to initialize rotating file logging at {}: {err}",
                        logs_root_path.display()
                    );
                    if tui_enabled {
                        subscriber_builder.with_writer(std::io::sink).try_init()
                    } else {
                        subscriber_builder.try_init()
                    }
                }
            },
            None if tui_enabled => subscriber_builder.with_writer(std::io::sink).try_init(),
            None => subscriber_builder.try_init(),
        };

        if let Err(err) = init_result {
            eprintln!("failed to initialize tracing subscriber: {err}");
        }
    });
}

#[cfg(not(test))]
fn run_entrypoint(args: impl IntoIterator<Item = OsString>) -> i32 {
    let mut cli = match parse_cli_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("{err}");
            return 2;
        }
    };
    apply_env_defaults(&mut cli);

    init_tracing(cli.logs_root.as_deref().map(Path::new), cli.tui);

    if let Some(CliCommand::Init { force }) = &cli.command {
        let init_root = std::env::current_dir()
            .ok()
            .and_then(|cwd| resolve_git_common_root(&cwd).map(|git_root| (cwd, git_root)))
            .map(|(cwd, git_root)| {
                let canonical_cwd = cwd.canonicalize().unwrap_or(cwd);
                let canonical_git_root = git_root.canonicalize().unwrap_or(git_root);
                if canonical_cwd != canonical_git_root {
                    println!(
                        "using git root {} for .symphony project home",
                        canonical_git_root.display()
                    );
                }
                canonical_git_root
            })
            .unwrap_or_else(|| PathBuf::from("."));

        match symphony::starter_assets::init_project_home(&init_root, *force) {
            Ok(summary) => {
                for path in &summary.written {
                    println!("created {}", path.display());
                }
                for path in &summary.skipped {
                    println!("skipped existing {}", path.display());
                }
                if !summary.skipped.is_empty() && !force {
                    println!("run `symphony init --force` to overwrite existing starter files");
                }
                return 0;
            }
            Err(err) => {
                eprintln!("init failed: {err}");
                return 1;
            }
        }
    }

    if let Some(CliCommand::Doctor { .. }) = &cli.command {
        let workflow_path = resolve_workflow_path(&cli);
        match run_doctor(&workflow_path) {
            Ok(code) => return code,
            Err(err) => {
                eprintln!("{err}");
                return 1;
            }
        }
    }

    if let Some(CliCommand::Helper {
        operation,
        workflow,
        input,
    }) = &cli.command
    {
        return run_helper(Path::new(workflow), operation, input.as_deref());
    }

    if let Some(CliCommand::Publication { action }) = &cli.command {
        let workflow_path = resolve_workflow_path(&cli);
        return run_publication(action, &workflow_path, &cli);
    }

    let mut deps = RuntimeBootstrapDeps::default();

    match execute_cli(&cli, &mut deps) {
        Ok(()) => 0,
        Err(err) => {
            tracing::error!(
                phase = "startup",
                workflow_path = %resolve_workflow_path(&cli).display(),
                error = %err,
                "startup failed"
            );
            eprintln!("{err}");
            1
        }
    }
}

#[cfg(not(test))]
fn resolve_git_common_root(start_dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(start_dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8(output.stdout).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let common_dir = PathBuf::from(trimmed);
    let absolute_common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        start_dir.join(common_dir)
    };

    absolute_common_dir.parent().map(Path::to_path_buf)
}

#[cfg(not(test))]
fn load_canonical_project_env() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            let _ = dotenvy::dotenv();
            return;
        }
    };

    if let Some(common_root) = resolve_git_common_root(&cwd) {
        let env_path = common_root.join(".env");
        if env_path.is_file() {
            let _ = dotenvy::from_path(&env_path);
        }
        // Inside a git repo we only load the canonical <repo-root>/.env.
        return;
    }

    // Backward-compatible fallback outside git repos.
    let _ = dotenvy::dotenv();
}

#[cfg(not(test))]
fn apply_github_token_aliases() {
    let gh_token = match std::env::var("GH_TOKEN") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return,
    };

    if std::env::var("GITHUB_TOKEN")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        std::env::set_var("GITHUB_TOKEN", &gh_token);
    }

    if std::env::var("KATA_GITHUB_TOKEN")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        std::env::set_var("KATA_GITHUB_TOKEN", &gh_token);
    }
}

#[cfg(not(test))]
#[tokio::main]
async fn main() {
    load_canonical_project_env();
    apply_github_token_aliases();

    let code = run_entrypoint(std::env::args_os());
    flush_file_logs();
    if code != 0 {
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_accepts_tui_flag() {
        let cli = parse_cli_from(["symphony", "WORKFLOW.md", "--tui"]).expect("cli parse");
        assert!(cli.tui);
    }

    #[test]
    fn parse_cli_defaults_tui_to_true() {
        let cli = parse_cli_from(["symphony", "WORKFLOW.md"]).expect("cli parse");
        assert!(cli.tui);
    }

    #[test]
    fn parse_cli_accepts_no_tui_flag() {
        let cli = parse_cli_from(["symphony", "WORKFLOW.md", "--no-tui"]).expect("cli parse");
        assert!(!cli.tui);
    }

    #[test]
    fn parse_cli_no_tui_wins_when_both_flags_are_present() {
        let cli =
            parse_cli_from(["symphony", "WORKFLOW.md", "--tui", "--no-tui"]).expect("cli parse");
        assert!(!cli.tui);
    }

    #[test]
    fn startup_banner_binding_uses_bound_port_when_configured_port_changes() {
        let configured = HttpBinding {
            host: "127.0.0.1".to_string(),
            port: 8080,
        };

        let banner_binding = startup_banner_binding(&configured, 8081);
        assert_eq!(banner_binding.host, "127.0.0.1");
        assert_eq!(banner_binding.port, 8081);
    }

    #[test]
    fn startup_banner_binding_keeps_ephemeral_marker_for_zero_port() {
        let configured = HttpBinding {
            host: "127.0.0.1".to_string(),
            port: 0,
        };

        let banner_binding = startup_banner_binding(&configured, 43123);
        assert_eq!(banner_binding.port, 0);
    }
}
