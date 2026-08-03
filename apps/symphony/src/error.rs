use thiserror::Error;

/// Top-level error types for the Symphony orchestrator.
#[derive(Debug, Error)]
pub enum SymphonyError {
    // ── Workflow / Config ──────────────────────────────────────────────
    #[error("missing workflow file: {path} ({reason})")]
    MissingWorkflowFile { path: String, reason: String },

    #[error("workflow parse error: {0}")]
    WorkflowParseError(String),

    #[error("workflow front matter is not a map")]
    WorkflowFrontMatterNotAMap,

    #[error("invalid workflow config: {0}")]
    InvalidWorkflowConfig(String),

    #[error("template parse error: {0}")]
    TemplateParseError(String),

    #[error("template render error: {0}")]
    TemplateRenderError(String),

    // ── Tracker ────────────────────────────────────────────────────────
    #[error("missing tracker kind")]
    MissingTrackerKind,

    #[error("unsupported tracker kind: {0}")]
    UnsupportedTrackerKind(String),

    #[error("missing Linear API token")]
    MissingLinearApiToken,

    #[error("missing Linear project slug")]
    MissingLinearProjectSlug,

    #[error("missing GitHub API token")]
    MissingGithubApiToken,

    #[error("Linear API request error: {0}")]
    LinearApiRequest(String),

    #[error("Linear API status error: {0}")]
    LinearApiStatus(u16),

    #[error("Linear GraphQL errors: {0}")]
    LinearGraphqlErrors(String),

    #[error("Linear unknown payload")]
    LinearUnknownPayload,

    #[error("Linear missing end cursor")]
    LinearMissingEndCursor,

    #[error("GitHub API request error: {0}")]
    GithubApiRequest(String),

    #[error("GitHub API status error: {status} ({message})")]
    GithubApiStatus { status: u16, message: String },

    #[error("GitHub Projects v2 error: {0}")]
    GithubProjectsV2Error(String),

    // ── Workspace ──────────────────────────────────────────────────────
    #[error("workspace path outside root: {workspace} not under {root}")]
    WorkspaceOutsideRoot { workspace: String, root: String },

    #[error("workspace hook failed: {hook} (status {status})")]
    WorkspaceHookFailed { hook: String, status: i32 },

    #[error("workspace hook timeout: {hook} ({timeout_ms}ms)")]
    WorkspaceHookTimeout { hook: String, timeout_ms: u64 },

    #[error("workspace error: {0}")]
    WorkspaceError(String),

    // ── Codex / Agent ──────────────────────────────────────────────────
    #[error("codex not found")]
    CodexNotFound,

    #[error("invalid workspace cwd: {0}")]
    InvalidWorkspaceCwd(String),

    #[error("codex response timeout")]
    ResponseTimeout,

    #[error("codex turn timeout")]
    TurnTimeout,

    #[error("codex port exit: {0}")]
    PortExit(i32),

    #[error("codex response error: {0}")]
    ResponseError(String),

    #[error("codex turn failed: {0}")]
    TurnFailed(String),

    #[error("codex turn cancelled: {0}")]
    TurnCancelled(String),

    #[error("codex turn input required")]
    TurnInputRequired,

    #[error("pi-agent error: {0}")]
    PiAgentError(String),

    // ── Triage / Storage ────────────────────────────────────────────────
    #[error("triage error: {0}")]
    TriageError(String),

    #[error("review cycle reopened: {0}")]
    ReviewCycleReopened(String),

    #[error("review publication conflict: {0}")]
    ReviewPublicationConflict(String),

    #[error("storage error: {0}")]
    StorageError(String),

    // ── SSH ────────────────────────────────────────────────────────────
    #[error("ssh launch failed: {0}")]
    SshLaunchFailed(String),

    // ── Docker ─────────────────────────────────────────────────────────
    #[error("docker daemon is not available")]
    DockerNotAvailable,

    #[error("docker container failed: {0}")]
    DockerContainerFailed(String),

    #[error("docker image build failed: {0}")]
    DockerImageBuildFailed(String),

    #[error("docker auth error: {0}")]
    DockerAuthError(String),

    // ── Generic ────────────────────────────────────────────────────────
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SymphonyError>;
