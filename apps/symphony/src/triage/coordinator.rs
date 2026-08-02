use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::domain::{AgentBackend, Issue, ServiceConfig, TriageSessionInfo, TriageSessionRegistry};
use crate::error::{Result, SymphonyError};
use crate::path_safety;
use crate::repo_url::repo_is_remote;
use crate::triage::correction::{compare_route_labels, RouteAgreement, ROUTE_KEYS};
use crate::triage::domain::{
    FactoryError, FactoryEventRecord, FactoryRunStatus, PublicationMode, PublicationStatus,
    TriageMode, TRIAGE_LEASE_RENEW_INTERVAL_MS, TRIAGE_STAGE_NAME,
};
use crate::triage::fingerprint::{
    configuration_revision, issue_revision, route_mapping_hash, ConfigurationRevisionInput,
    IssueCommentFingerprint, IssueMilestoneFingerprint, IssueRevisionInput, VerifiedTriageComment,
};
use crate::triage::intake::{TriageIntakeIssue, TriageIntakePort};
use crate::triage::process_identity;
use crate::triage::publisher::{
    AutomaticPublishRequest, AutomaticPublisher, IneligiblePublishRequest, PreviewPublishRequest,
    PreviewPublisher, TriageCommentPort, TriageRoutingPort,
};
use crate::triage::runner::{
    effective_pi_model, TriageHarness, TriageIssueIdentity, TriageProgress, TriageProgressSink,
    TriageRunner, TriageRunnerFailureKind, TriageRunnerOutcome, TriageRunnerRequest,
    TriageRunnerSuccess, TriageSpawnInfo,
};
use crate::triage::store::{
    ClaimAttemptRequest, CreatePublicationIntentRequest, FactoryRunStore,
    RecordAttemptProcessRequest, RouteObservationKind, StoreArtifactRequest,
    UpsertFactoryRunRequest,
};

const PENDING_INTENT_BATCH: usize = 256;
const CORRECTION_BATCH: usize = 256;
/// Durable-only diagnostic. Deliberately outside the live triage event
/// vocabulary, which the spec fixes to nine names.
const TRIAGE_ROUTE_CONSISTENCY_EVENT: &str = "triage_route_consistency";
const INELIGIBLE_REMEDIATION: &str =
    "Add the issue to the configured GitHub Project, then leave the intake label in place.";
const DESIRED_KIND_PREVIEW: &str = "preview_comment";
const DESIRED_KIND_INELIGIBLE: &str = "ineligible_diagnostic";
const DESIRED_KIND_AUTOMATIC: &str = "automatic_route";

/// Live triage event sink (optional). Durable events are always written to the store.
pub trait EventEmitter: Send + Sync {
    fn emit_triage_event(
        &self,
        event_name: &str,
        issue: Option<&str>,
        run_id: Option<&str>,
        stage_run_id: Option<&str>,
        payload: serde_json::Value,
    );
}

/// Executable triage stage. Production uses [`DefaultTriageExecutor`]; tests inject fakes.
#[async_trait]
pub trait TriageExecutor: Send + Sync {
    async fn execute(&self, request: TriageRunnerRequest) -> Result<TriageRunnerOutcome>;
}

#[derive(Debug, Default)]
pub struct DefaultTriageExecutor;

#[async_trait]
impl TriageExecutor for DefaultTriageExecutor {
    async fn execute(&self, request: TriageRunnerRequest) -> Result<TriageRunnerOutcome> {
        TriageRunner::run(request).await
    }
}

#[derive(Debug, Clone)]
pub struct TriageCoordinatorConfig {
    pub forge_host: String,
    pub repository: String,
    pub owner_instance: String,
    pub workflow_dir: PathBuf,
    pub project_display_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriagePollSummary {
    pub triage_enabled: bool,
    pub issues_seen: u32,
    pub attempts_started: u32,
    pub attempts_completed: u32,
    pub attempts_failed: u32,
    pub ineligible: u32,
    pub skipped: u32,
    pub reconciled_intents: u32,
    pub recovered_attempts: u32,
    pub route_corrections: u32,
}

pub struct TriageCoordinator<S, I, C, X = DefaultTriageExecutor> {
    store: S,
    intake: I,
    comments: C,
    publisher: PreviewPublisher<C>,
    routing: Option<Arc<dyn TriageRoutingPort>>,
    executor: X,
    config: TriageCoordinatorConfig,
    events: Option<Arc<dyn EventEmitter>>,
    sessions: Option<Arc<std::sync::Mutex<TriageSessionRegistry>>>,
    max_pages: u32,
}

impl<S, I, C> TriageCoordinator<S, I, C, DefaultTriageExecutor>
where
    S: FactoryRunStore,
    I: TriageIntakePort,
    C: TriageCommentPort + Clone,
{
    pub fn new(store: S, intake: I, comments: C, config: TriageCoordinatorConfig) -> Self {
        Self::with_executor(store, intake, comments, config, DefaultTriageExecutor)
    }
}

impl<S, I, C, X> TriageCoordinator<S, I, C, X>
where
    S: FactoryRunStore,
    I: TriageIntakePort,
    C: TriageCommentPort + Clone,
    X: TriageExecutor,
{
    pub fn with_executor(
        store: S,
        intake: I,
        comments: C,
        config: TriageCoordinatorConfig,
        executor: X,
    ) -> Self {
        Self {
            store,
            intake,
            comments: comments.clone(),
            publisher: PreviewPublisher::new(comments),
            routing: None,
            executor,
            config,
            events: None,
            sessions: None,
            max_pages: 100,
        }
    }

    pub fn with_routing(mut self, routing: impl TriageRoutingPort + 'static) -> Self {
        self.routing = Some(Arc::new(routing));
        self
    }

    pub fn with_events(mut self, events: Arc<dyn EventEmitter>) -> Self {
        self.events = Some(events);
        self
    }

    /// Attach the shared triage session registry used by the orchestrator
    /// snapshot so running attempts are visible to TUI/API consumers.
    pub fn with_session_registry(
        mut self,
        registry: Arc<std::sync::Mutex<TriageSessionRegistry>>,
    ) -> Self {
        self.sessions = Some(registry);
        self
    }

    fn session_begin(&self, info: TriageSessionInfo) {
        if let Some(registry) = &self.sessions {
            if let Ok(mut guard) = registry.lock() {
                guard.begin(info);
            }
        }
    }

    fn session_finish(&self, stage_run_id: &str) {
        if let Some(registry) = &self.sessions {
            if let Ok(mut guard) = registry.lock() {
                guard.finish(stage_run_id);
            }
        }
    }

    fn progress_sink(&self, stage_run_id: &str) -> Option<TriageProgressSink> {
        let registry = self.sessions.clone()?;
        let stage_run_id = stage_run_id.to_string();
        Some(Arc::new(move |progress: TriageProgress| {
            if let Ok(mut guard) = registry.lock() {
                guard.update(&stage_run_id, |info| {
                    if let Some(event) = &progress.last_event {
                        info.last_event = Some(event.clone());
                    }
                    if let Some(message) = &progress.message {
                        info.last_event_message = Some(message.clone());
                    }
                    info.current_tool_name = progress.current_tool_name.clone();
                    info.current_tool_args_preview = progress.current_tool_args_preview.clone();
                    if let Some(session_id) = &progress.session_id {
                        info.session_id = Some(session_id.clone());
                    }
                    if let Some(tokens) = progress.total_tokens {
                        info.total_tokens = tokens;
                    }
                });
            }
        }))
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub async fn reconcile_pending(&mut self, service: &ServiceConfig) -> Result<()> {
        self.reconcile_pending_intents(service, self.max_pages)
            .await?;
        Ok(())
    }

    /// Alias for [`Self::poll_once`].
    pub async fn reconcile_and_intake(
        &mut self,
        service: &ServiceConfig,
    ) -> Result<TriagePollSummary> {
        self.poll_once(service).await
    }

    pub async fn poll_once(&mut self, service: &ServiceConfig) -> Result<TriagePollSummary> {
        self.max_pages = service.triage.max_intake_pages.max(1);
        let mut summary = TriagePollSummary {
            triage_enabled: service.triage.enabled,
            ..TriagePollSummary::default()
        };

        self.store.interrupt_stale_attempts()?;
        summary.recovered_attempts = self.recover_interrupted_attempts(service).await?;
        summary.reconciled_intents = self
            .reconcile_pending_intents(service, self.max_pages)
            .await?;
        // Runs whether or not intake is enabled: published routes stay under
        // measurement even after triage is switched off.
        summary.route_corrections = self.reconcile_route_corrections().await?;

        if !service.triage.enabled {
            return Ok(summary);
        }

        let issues = self
            .intake
            .fetch_intake_issues(&service.triage.intake_label, self.max_pages)
            .await?;
        summary.issues_seen = issues.len() as u32;

        let prompt_template = load_prompt(&self.config.workflow_dir, &service.triage.prompt)?;
        let harness = triage_harness(service.agent_backend);
        let model = triage_model(service);
        let configuration_revision = configuration_revision(&ConfigurationRevisionInput {
            prompt: prompt_template.clone(),
            turn_timeout_ms: service.triage.turn_timeout_ms,
            max_attempts: service.triage.max_attempts,
            harness: triage_harness_name(harness).to_string(),
            model: model.clone(),
            routes: service.triage.routes.clone(),
        });
        let mapping_hash = route_mapping_hash(&service.triage.routes);
        let command = triage_command(service)?;
        let repo_path = require_repo_path(service)?;
        let workspace_root = resolve_workspace_path(&service.workspace.root, "workspace.root")?;

        for mut issue in issues {
            if issue.comments.is_empty() {
                issue.comments = self
                    .intake
                    .list_issue_comments(issue.issue_number, self.max_pages)
                    .await?;
            }

            if !issue.in_project {
                self.handle_ineligible(&issue, service, &mapping_hash)
                    .await?;
                summary.ineligible += 1;
                continue;
            }

            let outcome = self
                .handle_in_project(
                    &issue,
                    service,
                    &prompt_template,
                    harness,
                    model.as_deref(),
                    &configuration_revision,
                    &mapping_hash,
                    &command,
                    &repo_path,
                    &workspace_root,
                )
                .await?;
            match outcome {
                IssuePollOutcome::StartedCompleted => {
                    summary.attempts_started += 1;
                    summary.attempts_completed += 1;
                }
                IssuePollOutcome::StartedFailed => {
                    summary.attempts_started += 1;
                    summary.attempts_failed += 1;
                }
                IssuePollOutcome::Skipped => summary.skipped += 1,
            }
        }

        Ok(summary)
    }

    /// Reclaim attempts a previous process abandoned.
    ///
    /// An interrupted attempt may still own a live child and a disposable
    /// workspace. The child is signalled only when its PID, group, and OS start
    /// token still match, so a reused PID is never killed. Its executable may
    /// change legitimately through `exec` and is logged as diagnostic drift.
    /// Workspace cleanup and clearing the durable process record happen only
    /// after the orphan is confirmed gone (terminated or no longer present),
    /// so a still-live child keeps its identity for a later poll. The attempt
    /// itself stays interrupted and counts against `max_attempts`.
    async fn recover_interrupted_attempts(&mut self, service: &ServiceConfig) -> Result<u32> {
        let attempts = self.store.list_recoverable_attempts()?;
        let workspace_root = match resolve_workspace_path(&service.workspace.root, "workspace.root")
        {
            Ok(root) => root,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "skipping interrupted-attempt recovery; workspace.root is not usable"
                );
                return Ok(0);
            }
        };
        let mut recovered = 0;

        for attempt in attempts {
            let mut reclaim = true;
            if let Some(identity) = attempt.identity.as_ref() {
                reclaim = match process_identity::assess_signalability(identity) {
                    process_identity::Signalability::Authorized { executable } => {
                        if let process_identity::ExecutableComparison::Changed { recorded, live } =
                            &executable
                        {
                            tracing::info!(
                                stage_run_id = attempt.stage_run_id,
                                pid = identity.pid,
                                recorded_executable =
                                    recorded.as_deref().unwrap_or("<unavailable>"),
                                live_executable = live.as_deref().unwrap_or("<unavailable>"),
                                "triage child executable changed after spawn"
                            );
                        }
                        tracing::info!(
                            stage_run_id = attempt.stage_run_id,
                            process_group_id = identity.process_group_id,
                            "terminating orphaned triage process group"
                        );
                        match process_identity::terminate_process_group(identity).await {
                            process_identity::TerminationOutcome::Terminated => {
                                tracing::info!(
                                    stage_run_id = attempt.stage_run_id,
                                    process_group_id = identity.process_group_id,
                                    "orphaned triage process group terminated"
                                );
                                true
                            }
                            process_identity::TerminationOutcome::NoLongerSignalable(reason) => {
                                let gone = matches!(
                                    reason,
                                    process_identity::SignalBlockReason::ProcessNotFound
                                );
                                tracing::warn!(
                                    stage_run_id = attempt.stage_run_id,
                                    pid = identity.pid,
                                    process_group_id = identity.process_group_id,
                                    reason = %reason,
                                    "triage process identity changed before termination; signal skipped"
                                );
                                gone
                            }
                            process_identity::TerminationOutcome::StillRunning => {
                                tracing::warn!(
                                    stage_run_id = attempt.stage_run_id,
                                    pid = identity.pid,
                                    process_group_id = identity.process_group_id,
                                    "orphaned triage process group remains live after bounded termination; retaining process record"
                                );
                                false
                            }
                        }
                    }
                    process_identity::Signalability::Denied(reason) => {
                        let gone =
                            matches!(reason, process_identity::SignalBlockReason::ProcessNotFound);
                        if gone {
                            tracing::info!(
                                stage_run_id = attempt.stage_run_id,
                                pid = identity.pid,
                                process_group_id = identity.process_group_id,
                                "recorded triage process is already gone; reclaiming attempt directory"
                            );
                        } else {
                            tracing::warn!(
                                stage_run_id = attempt.stage_run_id,
                                pid = identity.pid,
                                process_group_id = identity.process_group_id,
                                reason = %reason,
                                "skipping orphan reclaim; recorded triage process identity is not signalable"
                            );
                        }
                        gone
                    }
                };
            }

            if !reclaim {
                continue;
            }

            if let Some(workspace) = attempt.workspace_path.as_deref() {
                match process_identity::attempt_root_for_cleanup(
                    Path::new(workspace),
                    &workspace_root,
                    &attempt.stage_run_id,
                ) {
                    Some(root) => {
                        if let Err(err) = std::fs::remove_dir_all(&root) {
                            if err.kind() != std::io::ErrorKind::NotFound {
                                // Report rather than swallow: a workspace we
                                // cannot reclaim is operator-visible disk leak.
                                tracing::warn!(
                                    stage_run_id = attempt.stage_run_id,
                                    path = %root.display(),
                                    error = %err,
                                    "failed to remove interrupted triage attempt directory"
                                );
                            }
                        }
                    }
                    None => tracing::warn!(
                        stage_run_id = attempt.stage_run_id,
                        path = workspace,
                        workspace_root = %workspace_root.display(),
                        "recorded triage workspace is not an attempt directory under workspace.root; not removing"
                    ),
                }
            }

            self.store.clear_attempt_process(&attempt.stage_run_id)?;
            recovered += 1;
        }

        Ok(recovered)
    }

    /// Measure whether humans kept or changed the routes Symphony published.
    ///
    /// Published issues are re-read from durable records, independently of the
    /// intake and implementation-candidate queries, and compared against the
    /// five route labels recorded on their publication intent. A correction is
    /// recorded at most once per artifact and observed route.
    async fn reconcile_route_corrections(&mut self) -> Result<u32> {
        let Some(routing) = self.routing.clone() else {
            return Ok(0);
        };
        let candidates = self.store.list_correction_candidates(CORRECTION_BATCH)?;
        let mut corrections = 0;

        for candidate in candidates {
            let Some(recorded_labels) =
                route_labels_from_desired_effects(&candidate.desired_effects)
            else {
                continue;
            };
            let issue_number = match parse_issue_number(&candidate.issue_id) {
                Ok(number) => number,
                Err(err) => {
                    tracing::warn!(
                        issue = candidate.issue_identifier,
                        error = %err,
                        "skipping correction check for unparsable durable issue id"
                    );
                    continue;
                }
            };

            let current_labels = match routing.list_issue_labels(issue_number).await {
                Ok(labels) => labels,
                Err(err) => {
                    // A transient read failure must not look like agreement;
                    // the next poll retries this candidate.
                    tracing::warn!(
                        issue = candidate.issue_identifier,
                        error = %err,
                        "failed to read issue labels for correction measurement"
                    );
                    continue;
                }
            };

            // Re-applied intake means a fresh triage attempt is coming, so the
            // published route is no longer the decision under measurement.
            if current_labels.iter().any(|label| {
                label
                    .trim()
                    .eq_ignore_ascii_case(candidate.intake_label.trim())
            }) {
                continue;
            }

            match compare_route_labels(
                &candidate.published_route,
                &recorded_labels,
                &current_labels,
            ) {
                RouteAgreement::Agreed => {}
                RouteAgreement::Corrected { route, label } => {
                    if self.store.record_route_observation(
                        &candidate.artifact_id,
                        RouteObservationKind::Corrected,
                        &route,
                    )? {
                        self.emit_event(
                            "triage_route_corrected",
                            Some(&candidate.issue_identifier),
                            Some(&candidate.run_id),
                            candidate.stage_run_id.as_deref(),
                            serde_json::json!({
                                "run_id": candidate.run_id,
                                "stage_run_id": candidate.stage_run_id,
                                "artifact_id": candidate.artifact_id,
                                "publication_intent_id": candidate.intent_id,
                                "route": candidate.published_route,
                                "corrected_route": route,
                                "corrected_label": label,
                                "status": "corrected",
                                "error_code": serde_json::Value::Null,
                            }),
                        )?;
                        corrections += 1;
                    }
                }
                RouteAgreement::Inconsistent { observed } => {
                    // Durable diagnostic only: an ambiguous label set is not an
                    // agreement result and must not reach the live event
                    // vocabulary or the correction count.
                    let value = observed.join(",");
                    if self.store.record_route_observation(
                        &candidate.artifact_id,
                        RouteObservationKind::Inconsistent,
                        &value,
                    )? {
                        self.store.record_event(FactoryEventRecord {
                            event_id: Uuid::now_v7().to_string(),
                            run_id: Some(candidate.run_id.clone()),
                            stage_run_id: candidate.stage_run_id.clone(),
                            event_type: TRIAGE_ROUTE_CONSISTENCY_EVENT.to_string(),
                            timestamp: Utc::now(),
                            payload: serde_json::json!({
                                "run_id": candidate.run_id,
                                "stage_run_id": candidate.stage_run_id,
                                "artifact_id": candidate.artifact_id,
                                "publication_intent_id": candidate.intent_id,
                                "route": candidate.published_route,
                                "observed_route_labels": observed,
                                "status": "inconsistent",
                                "error_code": "route_label_inconsistent",
                            }),
                        })?;
                    }
                }
            }
        }

        Ok(corrections)
    }

    async fn reconcile_pending_intents(
        &mut self,
        service: &ServiceConfig,
        max_pages: u32,
    ) -> Result<u32> {
        let pending = self.store.list_pending_intents(PENDING_INTENT_BATCH)?;
        let mut reconciled = 0u32;
        for intent in pending {
            let Some(run) = self.store.get_run_by_id(&intent.run_id)? else {
                continue;
            };
            let issue_number = parse_issue_number(&run.issue_id)?;
            let kind = desired_kind(&intent.desired_effects);

            if intent.mode == PublicationMode::Automatic || kind == Some(DESIRED_KIND_AUTOMATIC) {
                let Some(artifact_id) = intent.artifact_id.as_deref() else {
                    return Err(SymphonyError::TriageError(format!(
                        "automatic intent {} is missing artifact_id",
                        intent.intent_id
                    )));
                };
                let Some(artifact) = self.store.get_artifact_by_id(artifact_id)? else {
                    return Err(SymphonyError::TriageError(format!(
                        "artifact {artifact_id} missing for intent {}",
                        intent.intent_id
                    )));
                };
                let stage = self
                    .store
                    .get_stage_run(&artifact.stage_run_id)?
                    .ok_or_else(|| {
                        SymphonyError::TriageError(format!(
                            "stage run {} missing for artifact {artifact_id}",
                            artifact.stage_run_id
                        ))
                    })?;
                self.publish_automatic_intent(
                    service,
                    &intent,
                    issue_number,
                    &run.run_id,
                    &stage.stage_run_id,
                    stage.attempt,
                    &artifact.artifact,
                )
                .await?;
                reconciled += 1;
                continue;
            }

            if intent.artifact_id.is_some() || kind == Some(DESIRED_KIND_PREVIEW) {
                let Some(artifact_id) = intent.artifact_id.as_deref() else {
                    return Err(SymphonyError::TriageError(format!(
                        "preview intent {} is missing artifact_id",
                        intent.intent_id
                    )));
                };
                let Some(artifact) = self.store.get_artifact_by_id(artifact_id)? else {
                    return Err(SymphonyError::TriageError(format!(
                        "artifact {artifact_id} missing for intent {}",
                        intent.intent_id
                    )));
                };
                let stage = self
                    .store
                    .get_stage_run(&artifact.stage_run_id)?
                    .ok_or_else(|| {
                        SymphonyError::TriageError(format!(
                            "stage run {} missing for artifact {artifact_id}",
                            artifact.stage_run_id
                        ))
                    })?;
                self.publisher
                    .reconcile_preview(
                        &mut self.store,
                        PreviewPublishRequest {
                            intent: &intent,
                            issue_number,
                            run_id: &run.run_id,
                            stage_run_id: &stage.stage_run_id,
                            attempt: stage.attempt,
                            artifact: &artifact.artifact,
                            max_pages,
                        },
                    )
                    .await?;
                reconciled += 1;
                continue;
            }

            if kind == Some(DESIRED_KIND_INELIGIBLE) {
                let project_name = intent
                    .desired_effects
                    .get("project_name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(self.config.project_display_name.as_str());
                let remediation = intent
                    .desired_effects
                    .get("remediation")
                    .and_then(|value| value.as_str())
                    .unwrap_or(INELIGIBLE_REMEDIATION);
                self.publisher
                    .reconcile_ineligible_diagnostic(
                        &mut self.store,
                        IneligiblePublishRequest {
                            intent: &intent,
                            issue_number,
                            run_id: &run.run_id,
                            issue_identifier: &run.issue_identifier,
                            project_name,
                            remediation,
                            max_pages,
                        },
                    )
                    .await?;
                reconciled += 1;
            }
        }
        Ok(reconciled)
    }

    async fn handle_ineligible(
        &mut self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        mapping_hash: &str,
    ) -> Result<()> {
        let run = self.store.upsert_factory_run(UpsertFactoryRunRequest {
            forge_host: self.config.forge_host.clone(),
            repository: self.config.repository.clone(),
            issue_id: issue.issue_id.clone(),
            issue_identifier: issue.identifier.clone(),
            issue_revision: None,
            status: FactoryRunStatus::Ineligible,
            current_stage: Some(TRIAGE_STAGE_NAME.to_string()),
        })?;

        let existing = self.store.list_intents_for_run(&run.run_id)?;
        let mut intent = existing
            .into_iter()
            .find(|record| desired_kind(&record.desired_effects) == Some(DESIRED_KIND_INELIGIBLE));
        if intent.is_none() {
            intent = Some(self.store.create_publication_intent(
                CreatePublicationIntentRequest {
                    run_id: run.run_id.clone(),
                    artifact_id: None,
                    mode: PublicationMode::Preview,
                    intake_label: service.triage.intake_label.clone(),
                    route_label: String::new(),
                    project_state: None,
                    route_mapping_hash: mapping_hash.to_string(),
                    desired_effects: serde_json::json!({
                        "kind": DESIRED_KIND_INELIGIBLE,
                        "project_name": self.config.project_display_name,
                        "remediation": INELIGIBLE_REMEDIATION,
                    }),
                    observed_baseline: serde_json::json!({}),
                    expected_projection: serde_json::json!({}),
                },
            )?);
        }

        let intent = intent.expect("diagnostic intent present");
        if intent.status != PublicationStatus::Applied {
            self.publisher
                .reconcile_ineligible_diagnostic(
                    &mut self.store,
                    IneligiblePublishRequest {
                        intent: &intent,
                        issue_number: issue.issue_number,
                        run_id: &run.run_id,
                        issue_identifier: &issue.identifier,
                        project_name: &self.config.project_display_name,
                        remediation: INELIGIBLE_REMEDIATION,
                        max_pages: self.max_pages,
                    },
                )
                .await?;
        }

        let payload = serde_json::json!({
            "run_id": run.run_id,
            "stage_run_id": serde_json::Value::Null,
            "status": "ineligible",
            "error_code": "not_in_project",
            "publication_intent_id": intent.intent_id,
        });
        self.emit_event(
            "triage_ineligible",
            Some(&issue.identifier),
            Some(&run.run_id),
            None,
            payload,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_in_project(
        &mut self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        prompt_template: &str,
        harness: TriageHarness,
        model: Option<&str>,
        configuration_revision: &str,
        mapping_hash: &str,
        command: &[String],
        repo_path: &Path,
        workspace_root: &Path,
    ) -> Result<IssuePollOutcome> {
        let existing_run = self.store.get_run_by_issue(
            &self.config.forge_host,
            &self.config.repository,
            &issue.issue_id,
        )?;
        let verified = match existing_run.as_ref() {
            Some(run) => verified_comments_from_store(&self.store, &run.run_id)?,
            None => Vec::new(),
        };
        let issue_rev = issue_revision(&build_issue_revision_input(issue, service, verified));

        if let Some(run) = existing_run.as_ref() {
            if let Some(artifact) = self.store.get_artifact_for_revision(
                &run.run_id,
                &issue_rev,
                configuration_revision,
            )? {
                // Artifact may exist without a publication intent if the process
                // crashed (or intent creation failed) after store_artifact.
                // In automatic mode, promote a matching preview artifact or
                // resume a pending automatic intent without re-running the agent.
                self.recover_or_promote_artifact(issue, service, mapping_hash, &artifact)
                    .await?;
                return Ok(IssuePollOutcome::Skipped);
            }
            let attempts = self.store.list_stage_attempts_for_revision(
                crate::triage::domain::TRIAGE_STAGE_NAME,
                &run.run_id,
                &issue_rev,
                configuration_revision,
            )?;
            if attempts.iter().any(|attempt| !attempt.status.is_terminal()) {
                return Ok(IssuePollOutcome::Skipped);
            }
            if attempts.len() as u32 >= service.triage.max_attempts {
                return Ok(IssuePollOutcome::Skipped);
            }
        }

        let stage = match self.store.claim_attempt(ClaimAttemptRequest {
            forge_host: self.config.forge_host.clone(),
            repository: self.config.repository.clone(),
            issue_id: issue.issue_id.clone(),
            issue_identifier: issue.identifier.clone(),
            issue_revision: issue_rev.clone(),
            configuration_revision: configuration_revision.to_string(),
            owner_instance: self.config.owner_instance.clone(),
            harness: triage_harness_name(harness).to_string(),
            model: model.map(str::to_string),
            workspace_path: None,
            output_path: None,
            pid: None,
            process_group_id: None,
            process_start_token: None,
            executable_identity: None,
        }) {
            Ok(stage) => stage,
            Err(err) => {
                // Concurrent nonterminal claim or unique constraint — treat as skip.
                if matches!(err, SymphonyError::StorageError(_)) {
                    return Ok(IssuePollOutcome::Skipped);
                }
                return Err(err);
            }
        };

        self.emit_event(
            "triage_started",
            Some(&issue.identifier),
            Some(&stage.run_id),
            Some(&stage.stage_run_id),
            serde_json::json!({
                "run_id": stage.run_id,
                "stage_run_id": stage.stage_run_id,
                "status": "running",
                "error_code": serde_json::Value::Null,
                "attempt": stage.attempt,
            }),
        )?;

        let rendered_prompt = render_triage_prompt(prompt_template, issue, service)?;
        self.session_begin(TriageSessionInfo {
            issue_identifier: issue.identifier.clone(),
            run_id: stage.run_id.clone(),
            stage_run_id: stage.stage_run_id.clone(),
            attempt: stage.attempt,
            harness: triage_harness_name(harness).to_string(),
            model: model.map(str::to_string),
            started_at: Utc::now(),
            last_activity_at: None,
            last_event: Some("triage_started".to_string()),
            last_event_message: Some("preparing workspace".to_string()),
            session_id: None,
            current_tool_name: None,
            current_tool_args_preview: None,
            total_tokens: 0,
        });
        // The spawn notification arrives on a channel because the runner holds
        // no store handle; the lease-renewal loop drains it and persists the
        // identity while the turn is still in flight.
        let (spawn_tx, spawn_rx) = tokio::sync::mpsc::channel::<TriageSpawnInfo>(1);
        let outcome = self
            .execute_with_lease_renewal(
                &stage.stage_run_id,
                spawn_rx,
                TriageRunnerRequest {
                    attempt_id: stage.stage_run_id.clone(),
                    workspace_root: workspace_root.to_path_buf(),
                    repo_path: repo_path.to_path_buf(),
                    command: command.to_vec(),
                    prompt: rendered_prompt,
                    turn_timeout_ms: service.triage.turn_timeout_ms,
                    model: model.map(str::to_string),
                    harness,
                    issue: TriageIssueIdentity {
                        id: issue.issue_id.clone(),
                        identifier: issue.identifier.clone(),
                        title: issue.title.clone(),
                    },
                    codex: (harness == TriageHarness::Codex).then(|| service.codex.clone()),
                    progress: self.progress_sink(&stage.stage_run_id),
                    spawned: Some(Arc::new(move |info: TriageSpawnInfo| {
                        let _ = spawn_tx.try_send(info);
                    })),
                },
            )
            .await;
        self.session_finish(&stage.stage_run_id);
        let outcome = outcome?;

        match outcome {
            TriageRunnerOutcome::Success(success) => {
                self.complete_preview_success(
                    issue,
                    service,
                    &stage.stage_run_id,
                    &issue_rev,
                    configuration_revision,
                    mapping_hash,
                    *success,
                )
                .await?;
                Ok(IssuePollOutcome::StartedCompleted)
            }
            TriageRunnerOutcome::Failure(failure) => {
                let error = factory_error_from_runner(&failure.kind, &failure.message);
                self.store
                    .fail_attempt(&stage.stage_run_id, error.clone())?;
                let attempts = self.store.list_stage_attempts_for_revision(
                    crate::triage::domain::TRIAGE_STAGE_NAME,
                    &stage.run_id,
                    &issue_rev,
                    configuration_revision,
                )?;
                if attempts.len() as u32 >= service.triage.max_attempts {
                    self.store
                        .mark_run_status(&stage.run_id, FactoryRunStatus::Failed)?;
                }
                self.emit_event(
                    "triage_failed",
                    Some(&issue.identifier),
                    Some(&stage.run_id),
                    Some(&stage.stage_run_id),
                    serde_json::json!({
                        "run_id": stage.run_id,
                        "stage_run_id": stage.stage_run_id,
                        "status": "failed",
                        "error_code": error.code,
                        "attempt": stage.attempt,
                    }),
                )?;
                Ok(IssuePollOutcome::StartedFailed)
            }
        }
    }

    /// Run a triage turn while renewing the stage lease so concurrent polls do
    /// not mark the attempt interrupted after `TRIAGE_LEASE_STALE_AFTER_MS`.
    async fn execute_with_lease_renewal(
        &mut self,
        stage_run_id: &str,
        mut spawned: tokio::sync::mpsc::Receiver<TriageSpawnInfo>,
        request: TriageRunnerRequest,
    ) -> Result<TriageRunnerOutcome> {
        let owner_instance = self.config.owner_instance.clone();
        let mut execute = std::pin::pin!(self.executor.execute(request));
        let interval = std::time::Duration::from_millis(TRIAGE_LEASE_RENEW_INTERVAL_MS as u64);
        // First tick is immediate; skip so we renew after a full interval.
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;

        loop {
            tokio::select! {
                biased;
                outcome = &mut execute => return outcome,
                Some(info) = spawned.recv() => {
                    if let Err(err) = self.store.record_attempt_process(RecordAttemptProcessRequest {
                        stage_run_id: stage_run_id.to_string(),
                        owner_instance: owner_instance.clone(),
                        identity: info.identity,
                        workspace_path: Some(info.workspace_path.display().to_string()),
                        output_path: Some(info.output_path.display().to_string()),
                    }) {
                        // Losing this record only costs a restart the ability to
                        // reclaim the child, so warn instead of failing the turn.
                        tracing::warn!(
                            stage_run_id,
                            error = %err,
                            "failed to record triage attempt process identity"
                        );
                    }
                }
                _ = ticker.tick() => {
                    match self.store.renew_lease(stage_run_id, &owner_instance) {
                        Ok(true) => {}
                        Ok(false) => {
                            tracing::warn!(
                                stage_run_id,
                                "triage lease renew skipped (attempt not running or ownership lost)"
                            );
                        }
                        Err(err) => {
                            tracing::warn!(
                                stage_run_id,
                                error = %err,
                                "triage lease renew failed"
                            );
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn complete_preview_success(
        &mut self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        stage_run_id: &str,
        issue_revision: &str,
        configuration_revision: &str,
        mapping_hash: &str,
        success: TriageRunnerSuccess,
    ) -> Result<()> {
        let bytes_len = serde_json::to_vec(&success.artifact)
            .map(|bytes| bytes.len() as u64)
            .unwrap_or(0);
        let artifact = self.store.store_artifact(StoreArtifactRequest {
            stage_run_id: stage_run_id.to_string(),
            issue_revision: issue_revision.to_string(),
            configuration_revision: configuration_revision.to_string(),
            route_mapping_hash: mapping_hash.to_string(),
            artifact: success.artifact.clone(),
            bytes_len,
            usage: success.usage,
        })?;

        let stage = self.store.get_stage_run(stage_run_id)?.ok_or_else(|| {
            SymphonyError::TriageError(format!("stage run {stage_run_id} missing after store"))
        })?;

        let intent = match service.triage.mode {
            TriageMode::Preview => {
                self.ensure_preview_intent_for_artifact(issue, service, mapping_hash, &artifact)?
            }
            TriageMode::Automatic => {
                self.ensure_automatic_intent_for_artifact(issue, service, mapping_hash, &artifact)?
            }
        };

        self.emit_event(
            "triage_completed",
            Some(&issue.identifier),
            Some(&artifact.run_id),
            Some(stage_run_id),
            serde_json::json!({
                "run_id": artifact.run_id,
                "stage_run_id": stage_run_id,
                "artifact_id": artifact.artifact_id,
                "publication_intent_id": intent.intent_id,
                "route": success.artifact.route.as_str(),
                "status": "completed",
                "error_code": serde_json::Value::Null,
            }),
        )?;
        self.emit_event(
            "triage_publication_started",
            Some(&issue.identifier),
            Some(&artifact.run_id),
            Some(stage_run_id),
            serde_json::json!({
                "run_id": artifact.run_id,
                "stage_run_id": stage_run_id,
                "artifact_id": artifact.artifact_id,
                "publication_intent_id": intent.intent_id,
                "route": success.artifact.route.as_str(),
                "status": "pending",
                "error_code": serde_json::Value::Null,
            }),
        )?;

        match service.triage.mode {
            TriageMode::Preview => {
                self.publisher
                    .reconcile_preview(
                        &mut self.store,
                        PreviewPublishRequest {
                            intent: &intent,
                            issue_number: issue.issue_number,
                            run_id: &artifact.run_id,
                            stage_run_id,
                            attempt: stage.attempt,
                            artifact: &success.artifact,
                            max_pages: self.max_pages,
                        },
                    )
                    .await?;
                self.store
                    .mark_run_status(&artifact.run_id, FactoryRunStatus::Completed)?;
            }
            TriageMode::Automatic => {
                let status = self
                    .publish_automatic_intent(
                        service,
                        &intent,
                        issue.issue_number,
                        &artifact.run_id,
                        stage_run_id,
                        stage.attempt,
                        &success.artifact,
                    )
                    .await?;
                if status == PublicationStatus::Applied {
                    self.store
                        .mark_run_status(&artifact.run_id, FactoryRunStatus::Completed)?;
                }
            }
        }
        Ok(())
    }

    /// Ensure a preview publication intent exists for a stored artifact.
    ///
    /// If an intent already references this artifact, return it. Otherwise create
    /// one so `reconcile_pending_intents` can surface the preview comment after a
    /// crash between `store_artifact` and intent creation.
    fn ensure_preview_intent_for_artifact(
        &mut self,
        _issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        mapping_hash: &str,
        artifact: &crate::triage::domain::ArtifactRecord,
    ) -> Result<crate::triage::domain::PublicationIntentRecord> {
        let existing = self.store.list_intents_for_run(&artifact.run_id)?;
        if let Some(intent) = existing.into_iter().find(|record| {
            record.artifact_id.as_deref() == Some(artifact.artifact_id.as_str())
                && desired_kind(&record.desired_effects) == Some(DESIRED_KIND_PREVIEW)
        }) {
            return Ok(intent);
        }

        let route_mapping = service.triage.routes.mapping_for(artifact.artifact.route);
        self.store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact.run_id.clone(),
                artifact_id: Some(artifact.artifact_id.clone()),
                mode: PublicationMode::Preview,
                intake_label: service.triage.intake_label.clone(),
                route_label: route_mapping.label.clone(),
                project_state: route_mapping.state.clone(),
                route_mapping_hash: mapping_hash.to_string(),
                desired_effects: serde_json::json!({
                    "kind": DESIRED_KIND_PREVIEW,
                    "route": artifact.artifact.route.as_str(),
                }),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
    }

    fn ensure_automatic_intent_for_artifact(
        &mut self,
        _issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        mapping_hash: &str,
        artifact: &crate::triage::domain::ArtifactRecord,
    ) -> Result<crate::triage::domain::PublicationIntentRecord> {
        let existing = self.store.list_intents_for_run(&artifact.run_id)?;
        if let Some(intent) = existing.into_iter().find(|record| {
            record.artifact_id.as_deref() == Some(artifact.artifact_id.as_str())
                && record.mode == PublicationMode::Automatic
        }) {
            return Ok(intent);
        }

        let route_mapping = service.triage.routes.mapping_for(artifact.artifact.route);
        self.store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact.run_id.clone(),
                artifact_id: Some(artifact.artifact_id.clone()),
                mode: PublicationMode::Automatic,
                intake_label: service.triage.intake_label.clone(),
                route_label: route_mapping.label.clone(),
                project_state: route_mapping.state.clone(),
                route_mapping_hash: mapping_hash.to_string(),
                desired_effects: serde_json::json!({
                    "kind": DESIRED_KIND_AUTOMATIC,
                    "route": artifact.artifact.route.as_str(),
                    "route_labels": {
                        "implement": service.triage.routes.implement.label.clone(),
                        "spec": service.triage.routes.spec.label.clone(),
                        "needs_information": service.triage.routes.needs_information.label.clone(),
                        "park": service.triage.routes.park.label.clone(),
                        "human_owned": service.triage.routes.human_owned.label.clone(),
                    },
                }),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
    }

    #[allow(clippy::too_many_arguments)]
    async fn publish_automatic_intent(
        &mut self,
        service: &ServiceConfig,
        intent: &crate::triage::domain::PublicationIntentRecord,
        issue_number: u64,
        run_id: &str,
        stage_run_id: &str,
        attempt: u32,
        artifact: &crate::triage::domain::TriageArtifact,
    ) -> Result<PublicationStatus> {
        let routing = self.routing.clone().ok_or_else(|| {
            SymphonyError::TriageError(
                "automatic triage publication requires a routing port".to_string(),
            )
        })?;
        let publisher = AutomaticPublisher::new(self.comments.clone(), routing);
        // Prefer the five route labels persisted on the intent so a live mapping
        // reload cannot reinterpret an in-flight automatic publication.
        let managed_from_intent = managed_labels_from_automatic_intent(intent);
        let live_managed = service.triage.routes.managed_labels();
        let managed_labels = managed_from_intent
            .as_deref()
            .unwrap_or(live_managed.as_slice());
        let prior_status = intent.status;
        let status = publisher
            .reconcile_automatic(
                &mut self.store,
                AutomaticPublishRequest {
                    intent,
                    issue_number,
                    run_id,
                    stage_run_id,
                    attempt,
                    artifact,
                    managed_labels,
                    max_pages: self.max_pages,
                },
            )
            .await?;

        match status {
            PublicationStatus::Applied => {
                self.emit_event(
                    "triage_route_applied",
                    Some(&format!("#{issue_number}")),
                    Some(run_id),
                    Some(stage_run_id),
                    serde_json::json!({
                        "run_id": run_id,
                        "stage_run_id": stage_run_id,
                        "publication_intent_id": intent.intent_id,
                        "route": artifact.route.as_str(),
                        "route_label": intent.route_label,
                        "project_state": intent.project_state,
                        "status": "applied",
                        "error_code": serde_json::Value::Null,
                    }),
                )?;
            }
            PublicationStatus::Conflict if prior_status != PublicationStatus::Conflict => {
                self.emit_event(
                    "triage_publication_conflict",
                    Some(&format!("#{issue_number}")),
                    Some(run_id),
                    Some(stage_run_id),
                    serde_json::json!({
                        "run_id": run_id,
                        "stage_run_id": stage_run_id,
                        "publication_intent_id": intent.intent_id,
                        "status": "conflict",
                        "error_code": "human_conflict",
                    }),
                )?;
            }
            PublicationStatus::Blocked if prior_status != PublicationStatus::Blocked => {
                self.emit_event(
                    "triage_publication_blocked",
                    Some(&format!("#{issue_number}")),
                    Some(run_id),
                    Some(stage_run_id),
                    serde_json::json!({
                        "run_id": run_id,
                        "stage_run_id": stage_run_id,
                        "publication_intent_id": intent.intent_id,
                        "status": "blocked",
                        "error_code": "publication_blocked",
                    }),
                )?;
            }
            PublicationStatus::Pending
            | PublicationStatus::None
            | PublicationStatus::Conflict
            | PublicationStatus::Blocked => {}
        }
        Ok(status)
    }

    /// Finish run completion + durable applied event when an intent is already
    /// applied but a prior crash skipped finalization.
    fn finalize_applied_automatic_publication(
        &mut self,
        issue_number: u64,
        run_id: &str,
        stage_run_id: &str,
        intent: &crate::triage::domain::PublicationIntentRecord,
        artifact: &crate::triage::domain::TriageArtifact,
    ) -> Result<()> {
        let Some(run) = self.store.get_run_by_id(run_id)? else {
            return Ok(());
        };
        if run.status == FactoryRunStatus::Completed {
            return Ok(());
        }
        self.emit_event(
            "triage_route_applied",
            Some(&format!("#{issue_number}")),
            Some(run_id),
            Some(stage_run_id),
            serde_json::json!({
                "run_id": run_id,
                "stage_run_id": stage_run_id,
                "publication_intent_id": intent.intent_id,
                "route": artifact.route.as_str(),
                "route_label": intent.route_label,
                "project_state": intent.project_state,
                "status": "applied",
                "error_code": serde_json::Value::Null,
            }),
        )?;
        self.store
            .mark_run_status(run_id, FactoryRunStatus::Completed)?;
        Ok(())
    }

    /// Recover preview publication, or promote/resume automatic publication.
    async fn recover_or_promote_artifact(
        &mut self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        mapping_hash: &str,
        artifact: &crate::triage::domain::ArtifactRecord,
    ) -> Result<()> {
        match service.triage.mode {
            TriageMode::Preview => {
                self.recover_orphaned_preview_artifact(issue, service, mapping_hash, artifact)
                    .await
            }
            TriageMode::Automatic => {
                self.promote_or_resume_automatic(issue, service, mapping_hash, artifact)
                    .await
            }
        }
    }

    async fn promote_or_resume_automatic(
        &mut self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        mapping_hash: &str,
        artifact: &crate::triage::domain::ArtifactRecord,
    ) -> Result<()> {
        // Resume an existing automatic intent for this artifact when present.
        let existing = self.store.list_intents_for_run(&artifact.run_id)?;
        if let Some(intent) = existing.iter().find(|record| {
            record.artifact_id.as_deref() == Some(artifact.artifact_id.as_str())
                && record.mode == PublicationMode::Automatic
        }) {
            if intent.status == PublicationStatus::Applied {
                self.finalize_applied_automatic_publication(
                    issue.issue_number,
                    &artifact.run_id,
                    &artifact.stage_run_id,
                    intent,
                    &artifact.artifact,
                )?;
                return Ok(());
            }
            let stage = self
                .store
                .get_stage_run(&artifact.stage_run_id)?
                .ok_or_else(|| {
                    SymphonyError::TriageError(format!(
                        "stage run {} missing for automatic artifact {}",
                        artifact.stage_run_id, artifact.artifact_id
                    ))
                })?;
            let status = self
                .publish_automatic_intent(
                    service,
                    intent,
                    issue.issue_number,
                    &artifact.run_id,
                    &artifact.stage_run_id,
                    stage.attempt,
                    &artifact.artifact,
                )
                .await?;
            if status == PublicationStatus::Applied {
                self.store
                    .mark_run_status(&artifact.run_id, FactoryRunStatus::Completed)?;
            }
            return Ok(());
        }

        // Promote an immutable preview artifact only when revision + mapping still match.
        if artifact.route_mapping_hash != mapping_hash {
            return Ok(());
        }
        let intent =
            self.ensure_automatic_intent_for_artifact(issue, service, mapping_hash, artifact)?;
        let stage = self
            .store
            .get_stage_run(&artifact.stage_run_id)?
            .ok_or_else(|| {
                SymphonyError::TriageError(format!(
                    "stage run {} missing for promoted artifact {}",
                    artifact.stage_run_id, artifact.artifact_id
                ))
            })?;
        self.emit_event(
            "triage_publication_started",
            Some(&issue.identifier),
            Some(&artifact.run_id),
            Some(&artifact.stage_run_id),
            serde_json::json!({
                "run_id": artifact.run_id,
                "stage_run_id": artifact.stage_run_id,
                "artifact_id": artifact.artifact_id,
                "publication_intent_id": intent.intent_id,
                "route": artifact.artifact.route.as_str(),
                "status": "pending",
                "error_code": serde_json::Value::Null,
                "promoted_from_preview": true,
            }),
        )?;
        let status = self
            .publish_automatic_intent(
                service,
                &intent,
                issue.issue_number,
                &artifact.run_id,
                &artifact.stage_run_id,
                stage.attempt,
                &artifact.artifact,
            )
            .await?;
        if status == PublicationStatus::Applied {
            self.store
                .mark_run_status(&artifact.run_id, FactoryRunStatus::Completed)?;
        }
        Ok(())
    }

    /// Recover when a durable artifact exists without a published preview.
    async fn recover_orphaned_preview_artifact(
        &mut self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        mapping_hash: &str,
        artifact: &crate::triage::domain::ArtifactRecord,
    ) -> Result<()> {
        let intent =
            self.ensure_preview_intent_for_artifact(issue, service, mapping_hash, artifact)?;
        if intent.status == PublicationStatus::Applied {
            return Ok(());
        }

        let stage = self
            .store
            .get_stage_run(&artifact.stage_run_id)?
            .ok_or_else(|| {
                SymphonyError::TriageError(format!(
                    "stage run {} missing for orphaned artifact {}",
                    artifact.stage_run_id, artifact.artifact_id
                ))
            })?;
        self.publisher
            .reconcile_preview(
                &mut self.store,
                PreviewPublishRequest {
                    intent: &intent,
                    issue_number: issue.issue_number,
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    attempt: stage.attempt,
                    artifact: &artifact.artifact,
                    max_pages: self.max_pages,
                },
            )
            .await?;
        self.store
            .mark_run_status(&artifact.run_id, FactoryRunStatus::Completed)?;
        Ok(())
    }

    fn emit_event(
        &mut self,
        event_name: &str,
        issue: Option<&str>,
        run_id: Option<&str>,
        stage_run_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<()> {
        if let Some(emitter) = &self.events {
            emitter.emit_triage_event(event_name, issue, run_id, stage_run_id, payload.clone());
        }
        self.store.record_event(FactoryEventRecord {
            event_id: Uuid::now_v7().to_string(),
            run_id: run_id.map(str::to_string),
            stage_run_id: stage_run_id.map(str::to_string),
            event_type: event_name.to_string(),
            timestamp: Utc::now(),
            payload,
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssuePollOutcome {
    StartedCompleted,
    StartedFailed,
    Skipped,
}

fn load_prompt(workflow_dir: &Path, relative: &str) -> Result<String> {
    let path = workflow_dir.join(relative);
    std::fs::read_to_string(&path).map_err(|err| {
        SymphonyError::TriageError(format!(
            "failed to read triage prompt {}: {err}",
            path.display()
        ))
    })
}

fn require_repo_path(service: &ServiceConfig) -> Result<PathBuf> {
    let repo = service
        .workspace
        .repo
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            SymphonyError::TriageError(
                "workspace.repo local path is required for triage".to_string(),
            )
        })?;
    if repo_is_remote(repo) {
        return Err(SymphonyError::TriageError(
            "workspace.repo must be a local path for triage (remote URLs are not supported)"
                .to_string(),
        ));
    }
    // Resolve against process cwd once. Triage clones with cwd set to the empty
    // workspace directory, so a bare "." would otherwise mean that empty dir.
    resolve_workspace_path(repo, "workspace.repo")
}

fn resolve_workspace_path(configured: &str, field: &str) -> Result<PathBuf> {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        return Err(SymphonyError::TriageError(format!(
            "{field} cannot be empty"
        )));
    }
    path_safety::canonicalize(Path::new(trimmed)).map_err(|err| {
        SymphonyError::TriageError(format!("failed to resolve {field} '{trimmed}': {err}"))
    })
}

fn triage_harness(backend: AgentBackend) -> TriageHarness {
    match backend {
        AgentBackend::KataCli => TriageHarness::Pi,
        AgentBackend::Codex => TriageHarness::Codex,
    }
}

fn triage_harness_name(harness: TriageHarness) -> &'static str {
    match harness {
        TriageHarness::Pi => "pi",
        TriageHarness::Codex => "codex",
    }
}

/// Render the triage prompt template with issue context (Liquid, strict mode).
fn render_triage_prompt(
    template: &str,
    issue: &TriageIntakeIssue,
    service: &ServiceConfig,
) -> Result<String> {
    let domain_issue = Issue {
        id: issue.issue_id.clone(),
        identifier: issue.identifier.clone(),
        title: issue.title.clone(),
        description: Some(issue.body.clone()),
        priority: None,
        state: String::new(),
        branch_name: None,
        url: Some(format!(
            "https://{}/{}/issues/{}",
            issue.forge_host, issue.repository, issue.issue_number
        )),
        assignee_id: None,
        labels: issue.labels.clone(),
        blocked_by: Vec::new(),
        assigned_to_worker: true,
        created_at: Some(issue.created_at),
        updated_at: Some(issue.updated_at),
        children_count: 0,
        parent_identifier: None,
    };
    crate::prompt_builder::render_prompt(
        template,
        &domain_issue,
        None,
        service.workspace.base_branch.as_deref(),
    )
}

fn triage_command(service: &ServiceConfig) -> Result<Vec<String>> {
    let command = match service.agent_backend {
        AgentBackend::KataCli => service.pi_agent.command.clone(),
        AgentBackend::Codex => service.codex.command.clone(),
    };
    if command.is_empty() {
        return Err(SymphonyError::TriageError(
            "triage agent command cannot be empty".to_string(),
        ));
    }
    Ok(command)
}

fn triage_model(service: &ServiceConfig) -> Option<String> {
    match service.agent_backend {
        AgentBackend::KataCli => effective_pi_model(
            service.triage.model.as_deref(),
            service.pi_agent.model.as_deref(),
        ),
        AgentBackend::Codex => None,
    }
}

fn desired_kind(value: &serde_json::Value) -> Option<&str> {
    value.get("kind").and_then(|kind| kind.as_str())
}

/// Reconstruct the managed route-label vocabulary from a persisted automatic intent.
fn managed_labels_from_automatic_intent(
    intent: &crate::triage::domain::PublicationIntentRecord,
) -> Option<Vec<String>> {
    route_labels_from_desired_effects(&intent.desired_effects)
}

/// The five route labels an automatic intent recorded, in [`ROUTE_KEYS`] order.
///
/// Publication and correction measurement both read the mapping from here so a
/// live `WORKFLOW.md` reload cannot reinterpret an existing publication.
fn route_labels_from_desired_effects(desired_effects: &serde_json::Value) -> Option<Vec<String>> {
    let labels = desired_effects.get("route_labels")?.as_object()?;
    let mut out = Vec::with_capacity(ROUTE_KEYS.len());
    for key in ROUTE_KEYS {
        let label = labels.get(key)?.as_str()?.trim();
        if label.is_empty() {
            return None;
        }
        out.push(label.to_string());
    }
    Some(out)
}

fn parse_issue_number(issue_id: &str) -> Result<u64> {
    issue_id.parse::<u64>().map_err(|err| {
        SymphonyError::TriageError(format!("invalid factory run issue_id '{issue_id}': {err}"))
    })
}

fn verified_comments_from_store(
    store: &dyn FactoryRunStore,
    run_id: &str,
) -> Result<Vec<VerifiedTriageComment>> {
    let mut out = Vec::new();
    for identity in store.list_verified_comment_identities(run_id)? {
        let Ok(comment_id) = identity.comment_id.parse::<u64>() else {
            continue;
        };
        out.push(VerifiedTriageComment {
            comment_id,
            intent_id: identity.intent_id,
            publisher_login: identity.publisher_login,
        });
    }
    Ok(out)
}

fn build_issue_revision_input(
    issue: &TriageIntakeIssue,
    service: &ServiceConfig,
    verified_triage_comments: Vec<VerifiedTriageComment>,
) -> IssueRevisionInput {
    IssueRevisionInput {
        title: issue.title.clone(),
        body: issue.body.clone(),
        labels: issue.labels.clone(),
        managed_labels: service.triage.routes.managed_labels(),
        intake_label: service.triage.intake_label.clone(),
        assignees: issue.assignees.clone(),
        milestone: issue
            .milestone
            .as_ref()
            .map(|milestone| IssueMilestoneFingerprint {
                number: milestone.number,
                title: milestone.title.clone(),
            }),
        comments: issue
            .comments
            .iter()
            .map(|comment| IssueCommentFingerprint {
                id: comment.id,
                author_login: comment.author_login.clone(),
                body: comment.body.clone(),
                created_at: comment.created_at.to_rfc3339(),
                updated_at: comment.updated_at.to_rfc3339(),
            })
            .collect(),
        verified_triage_comments,
    }
}

fn factory_error_from_runner(kind: &TriageRunnerFailureKind, message: &str) -> FactoryError {
    let (code, retryable) = match kind {
        TriageRunnerFailureKind::Setup => ("triage_setup_failed", false),
        TriageRunnerFailureKind::Spawn => ("triage_spawn_failed", true),
        TriageRunnerFailureKind::Timeout => ("triage_timeout", true),
        TriageRunnerFailureKind::NonZeroExit => ("triage_nonzero_exit", true),
        TriageRunnerFailureKind::MissingOutput => ("triage_missing_output", true),
        TriageRunnerFailureKind::InvalidArtifact => ("triage_invalid_artifact", true),
        TriageRunnerFailureKind::Integrity => ("triage_integrity_failed", true),
    };
    FactoryError::new(
        code,
        "triage_runner",
        format!("{message}. Fix the underlying cause and wait for the next triage poll retry."),
        retryable,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::{GithubIssueComment, GithubUser};
    use crate::triage::domain::{
        EvidenceKind, RiskClass, StageUsage, TriageArtifact, TriageEvidence, TriageRoute,
        TRIAGE_SCHEMA_VERSION,
    };
    use crate::triage::intake::IntakeComment;
    use crate::triage::integrity::RepoBaseline;
    use crate::triage::store::SqliteFactoryStore;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingEmitter {
        events: Mutex<Vec<(String, Option<String>)>>,
    }

    impl EventEmitter for RecordingEmitter {
        fn emit_triage_event(
            &self,
            event_name: &str,
            issue: Option<&str>,
            _run_id: Option<&str>,
            _stage_run_id: Option<&str>,
            _payload: serde_json::Value,
        ) {
            self.events
                .lock()
                .unwrap()
                .push((event_name.to_string(), issue.map(str::to_string)));
        }
    }

    #[derive(Default)]
    struct FakeIntake {
        issues: Mutex<Vec<TriageIntakeIssue>>,
        comments: Mutex<HashMap<u64, Vec<IntakeComment>>>,
    }

    #[async_trait]
    impl TriageIntakePort for Arc<FakeIntake> {
        async fn fetch_intake_issues(
            &self,
            _intake_label: &str,
            _max_pages: u32,
        ) -> Result<Vec<TriageIntakeIssue>> {
            Ok(self.issues.lock().unwrap().clone())
        }

        async fn list_issue_comments(
            &self,
            issue_number: u64,
            _max_pages: u32,
        ) -> Result<Vec<IntakeComment>> {
            Ok(self
                .comments
                .lock()
                .unwrap()
                .get(&issue_number)
                .cloned()
                .unwrap_or_default())
        }

        async fn authenticated_login(&self) -> Result<String> {
            Ok("symphony-bot".to_string())
        }
    }

    #[derive(Default)]
    struct FakeComments {
        login: String,
        comments: Mutex<HashMap<u64, GithubIssueComment>>,
        next_id: Mutex<u64>,
        create_count: Mutex<u32>,
        list_count: Mutex<u32>,
    }

    impl FakeComments {
        fn new(login: &str) -> Arc<Self> {
            Arc::new(Self {
                login: login.to_string(),
                comments: Mutex::new(HashMap::new()),
                next_id: Mutex::new(500),
                create_count: Mutex::new(0),
                list_count: Mutex::new(0),
            })
        }
    }

    #[async_trait]
    impl TriageCommentPort for Arc<FakeComments> {
        async fn authenticated_login(&self) -> Result<String> {
            Ok(self.login.clone())
        }

        async fn list_comments(
            &self,
            _issue_number: u64,
            _max_pages: u32,
        ) -> Result<Vec<GithubIssueComment>> {
            *self.list_count.lock().unwrap() += 1;
            let mut values: Vec<_> = self.comments.lock().unwrap().values().cloned().collect();
            values.sort_by_key(|comment| comment.id);
            Ok(values)
        }

        async fn get_comment(&self, comment_id: u64) -> Result<GithubIssueComment> {
            self.comments
                .lock()
                .unwrap()
                .get(&comment_id)
                .cloned()
                .ok_or_else(|| SymphonyError::GithubApiStatus {
                    status: 404,
                    message: "comment not found".to_string(),
                })
        }

        async fn create_comment(
            &self,
            _issue_number: u64,
            body: &str,
        ) -> Result<GithubIssueComment> {
            let mut next = self.next_id.lock().unwrap();
            let id = *next;
            *next += 1;
            *self.create_count.lock().unwrap() += 1;
            let comment = GithubIssueComment {
                id,
                user: Some(GithubUser {
                    login: self.login.clone(),
                }),
                body: Some(body.to_string()),
                html_url: None,
                created_at: None,
                updated_at: None,
            };
            self.comments.lock().unwrap().insert(id, comment.clone());
            Ok(comment)
        }

        async fn update_comment(&self, comment_id: u64, body: &str) -> Result<GithubIssueComment> {
            let mut comments = self.comments.lock().unwrap();
            let comment =
                comments
                    .get_mut(&comment_id)
                    .ok_or_else(|| SymphonyError::GithubApiStatus {
                        status: 404,
                        message: "comment not found".to_string(),
                    })?;
            comment.body = Some(body.to_string());
            Ok(comment.clone())
        }
    }

    struct FakeExecutor {
        outcome: Mutex<Option<TriageRunnerOutcome>>,
        delay: std::time::Duration,
    }

    impl FakeExecutor {
        fn success(artifact: TriageArtifact) -> Self {
            Self::success_after(artifact, std::time::Duration::ZERO)
        }

        fn success_after(artifact: TriageArtifact, delay: std::time::Duration) -> Self {
            Self {
                outcome: Mutex::new(Some(TriageRunnerOutcome::Success(Box::new(
                    TriageRunnerSuccess {
                        artifact,
                        usage: StageUsage::default(),
                        workspace_path: PathBuf::from("/tmp/ws"),
                        output_path: PathBuf::from("/tmp/out.json"),
                        baseline: RepoBaseline {
                            head: "abc".to_string(),
                            submodules: BTreeMap::new(),
                        },
                    },
                )))),
                delay,
            }
        }
    }

    #[async_trait]
    impl TriageExecutor for FakeExecutor {
        async fn execute(&self, _request: TriageRunnerRequest) -> Result<TriageRunnerOutcome> {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            Ok(self
                .outcome
                .lock()
                .unwrap()
                .take()
                .expect("fake executor outcome"))
        }
    }

    fn sample_artifact() -> TriageArtifact {
        TriageArtifact {
            schema_version: TRIAGE_SCHEMA_VERSION,
            route: TriageRoute::Implement,
            risk_class: RiskClass::Low,
            rationale: "Bounded docs fix.".to_string(),
            evidence: vec![TriageEvidence {
                kind: EvidenceKind::Issue,
                reference: "body".to_string(),
                summary: "Exact file named.".to_string(),
            }],
            next_action: "Apply the change.".to_string(),
            clarification_question: None,
            reproduction: None,
        }
    }

    fn sample_issue(in_project: bool, number: u64) -> TriageIntakeIssue {
        TriageIntakeIssue {
            issue_id: number.to_string(),
            issue_number: number,
            identifier: format!("#{number}"),
            title: "Fix docs".to_string(),
            body: "Update README".to_string(),
            labels: vec!["needs-triage".to_string(), "bug".to_string()],
            non_managed_labels: vec!["bug".to_string()],
            assignees: vec![],
            milestone: None,
            comments: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            forge_host: "github.com".to_string(),
            repository: "owner/repo".to_string(),
            in_project,
        }
    }

    fn service_config(enabled: bool, workflow_dir: &Path) -> ServiceConfig {
        let mut service = ServiceConfig::default();
        service.triage.enabled = enabled;
        service.triage.prompt = "prompt.md".to_string();
        service.workspace.repo = Some("/tmp/repo".to_string());
        service.workspace.root = workflow_dir
            .join("workspaces")
            .to_string_lossy()
            .to_string();
        service.agent_backend = AgentBackend::KataCli;
        service.pi_agent.command = vec!["pi".to_string(), "--mode".to_string(), "rpc".to_string()];
        service
    }

    fn prep_workflow(temp: &tempfile::TempDir) -> PathBuf {
        let workflow_dir = temp.path().join("workflow");
        std::fs::create_dir_all(&workflow_dir).unwrap();
        std::fs::write(workflow_dir.join("prompt.md"), "triage prompt").unwrap();
        workflow_dir
    }

    fn open_store(temp: &tempfile::TempDir) -> SqliteFactoryStore {
        SqliteFactoryStore::acquire_lock_and_migrate(&temp.path().join("state.db"), 5_000).unwrap()
    }

    #[tokio::test]
    async fn off_project_writes_ineligible_diagnostic_without_attempt() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let store = open_store(&temp);
        let intake = Arc::new(FakeIntake::default());
        intake.issues.lock().unwrap().push(sample_issue(false, 42));
        let comments = FakeComments::new("symphony-bot");
        let emitter = Arc::new(RecordingEmitter::default());
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            intake,
            comments.clone(),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        )
        .with_events(emitter.clone());

        let summary = coordinator
            .poll_once(&service_config(true, &workflow_dir))
            .await
            .unwrap();

        assert_eq!(summary.ineligible, 1);
        assert_eq!(summary.attempts_started, 0);
        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        let run = coordinator
            .store_mut()
            .get_run_by_issue("github.com", "owner/repo", "42")
            .unwrap()
            .unwrap();
        assert_eq!(run.status, FactoryRunStatus::Ineligible);
        assert!(coordinator
            .store_mut()
            .list_stage_runs(&run.run_id)
            .unwrap()
            .is_empty());
        let events = emitter.events.lock().unwrap().clone();
        assert!(events
            .iter()
            .any(|(name, issue)| name == "triage_ineligible" && issue.as_deref() == Some("#42")));
        let body = comments
            .comments
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .body
            .clone()
            .unwrap();
        assert!(body.contains("not eligible for triage"));
    }

    #[tokio::test]
    async fn in_project_preview_success_creates_intent_and_comment() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let store = open_store(&temp);
        let intake = Arc::new(FakeIntake::default());
        intake.issues.lock().unwrap().push(sample_issue(true, 7));
        let comments = FakeComments::new("symphony-bot");
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            intake,
            comments.clone(),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        );

        let summary = coordinator
            .poll_once(&service_config(true, &workflow_dir))
            .await
            .unwrap();

        assert_eq!(summary.attempts_completed, 1);
        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        let run = coordinator
            .store_mut()
            .get_run_by_issue("github.com", "owner/repo", "7")
            .unwrap()
            .unwrap();
        assert_eq!(run.status, FactoryRunStatus::Completed);
        let artifact = coordinator
            .store_mut()
            .get_latest_artifact(&run.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(artifact.artifact.route, TriageRoute::Implement);
        let intent = coordinator
            .store_mut()
            .get_latest_publication(&run.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(intent.mode, PublicationMode::Preview);
        assert_eq!(intent.status, PublicationStatus::Applied);
        assert!(intent.comment_id.is_some());
        let body = comments
            .comments
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .body
            .clone()
            .unwrap();
        assert!(body.contains("Symphony triage preview"));
        assert!(body.contains(&intent.intent_id));
    }

    #[tokio::test(start_paused = true)]
    async fn long_running_attempt_renews_lease_before_stale_interrupt() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let store = open_store(&temp);
        let intake = Arc::new(FakeIntake::default());
        intake.issues.lock().unwrap().push(sample_issue(true, 33));
        let comments = FakeComments::new("symphony-bot");
        // Longer than one renew interval so execute_with_lease_renewal must tick.
        let delay = std::time::Duration::from_millis((TRIAGE_LEASE_RENEW_INTERVAL_MS as u64) + 50);
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            intake,
            comments.clone(),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success_after(sample_artifact(), delay),
        );

        let summary = coordinator
            .poll_once(&service_config(true, &workflow_dir))
            .await
            .unwrap();

        assert_eq!(summary.attempts_completed, 1);
        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        let run = coordinator
            .store_mut()
            .get_run_by_issue("github.com", "owner/repo", "33")
            .unwrap()
            .unwrap();
        assert_eq!(run.status, FactoryRunStatus::Completed);
        let stages = coordinator
            .store_mut()
            .list_stage_runs(&run.run_id)
            .unwrap();
        assert_eq!(stages.len(), 1);
        assert_ne!(
            stages[0].status,
            crate::triage::domain::StageStatus::Interrupted
        );
    }

    /// After a crash the previous process's triage child can still be running
    /// against a workspace this process is about to reclaim. A poll must kill
    /// the recorded process group and delete the attempt directory, so the
    /// retry starts from a clean workspace instead of racing a live agent.
    #[tokio::test]
    async fn recovery_terminates_orphaned_child_and_removes_attempt_directory() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let mut store = open_store(&temp);

        // Capture a shell launcher, then let it replace itself with the worker.
        // This is the live failure shape that previously made executable
        // equality reject a legitimate orphan.
        let mut orphan = crate::triage::process_identity::ExecTransitionChild::spawn();
        let identity = orphan.identity().clone();
        orphan.release_and_wait_for_exec();
        assert!(matches!(
            crate::triage::process_identity::assess_signalability(&identity),
            crate::triage::process_identity::Signalability::Authorized {
                executable: crate::triage::process_identity::ExecutableComparison::Changed { .. }
            }
        ));

        let stage = store
            .claim_attempt(ClaimAttemptRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: "77".to_string(),
                issue_identifier: "#77".to_string(),
                issue_revision: "rev".to_string(),
                configuration_revision: "config".to_string(),
                owner_instance: "prior-owner".to_string(),
                harness: "pi".to_string(),
                model: None,
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })
            .unwrap();
        // Mirror the runner layout: triage-<stage_run_id> under workspace.root.
        let attempt_root = workflow_dir
            .join("workspaces")
            .join(format!("triage-{}", stage.stage_run_id));
        let workspace_path = attempt_root.join("workspace");
        std::fs::create_dir_all(&workspace_path).unwrap();
        std::fs::write(workspace_path.join("README.md"), "clone\n").unwrap();

        store
            .record_attempt_process(RecordAttemptProcessRequest {
                stage_run_id: stage.stage_run_id.clone(),
                owner_instance: "prior-owner".to_string(),
                identity: identity.clone(),
                workspace_path: Some(workspace_path.display().to_string()),
                output_path: Some(
                    attempt_root
                        .join("stage-output")
                        .join("result.json")
                        .display()
                        .to_string(),
                ),
            })
            .unwrap();
        store
            .interrupt_attempt(&stage.stage_run_id, "prior-owner")
            .unwrap();

        let mut coordinator = TriageCoordinator::with_executor(
            store,
            Arc::new(FakeIntake::default()),
            FakeComments::new("symphony-bot"),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "new-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        );

        assert!(
            crate::triage::process_identity::is_signalable(&identity),
            "precondition: the orphan must be identifiable and signalable"
        );
        assert_eq!(
            coordinator
                .store_mut()
                .list_recoverable_attempts()
                .unwrap()
                .len(),
            1,
            "precondition: the interrupted attempt must be recoverable work"
        );

        coordinator
            .poll_once(&service_config(true, &workflow_dir))
            .await
            .unwrap();

        assert!(
            orphan.wait_for_exit(),
            "orphaned child must be terminated and reaped by recovery"
        );
        assert!(
            !attempt_root.exists(),
            "attempt directory must be reclaimed before retry"
        );

        // Cleared so a later poll does not try to reclaim the same attempt.
        let stage = coordinator
            .store_mut()
            .get_stage_run(&stage.stage_run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            stage.status,
            crate::triage::domain::StageStatus::Interrupted,
            "recovery must not resurrect the attempt"
        );
        assert_eq!(stage.pid, None);
        assert_eq!(stage.process_group_id, None);
        assert_eq!(stage.process_start_token, None);
        assert_eq!(stage.executable_identity, None);
        assert_eq!(stage.workspace_path, None);
        assert_eq!(stage.output_path, None);
        assert!(coordinator
            .store_mut()
            .list_recoverable_attempts()
            .unwrap()
            .is_empty());
    }

    #[derive(Default)]
    struct FakeRouting {
        labels: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl TriageRoutingPort for Arc<FakeRouting> {
        async fn list_issue_labels(&self, _issue_number: u64) -> Result<Vec<String>> {
            Ok(self.labels.lock().unwrap().clone())
        }

        async fn issue_project_state(&self, _issue_number: u64) -> Result<Option<String>> {
            Ok(None)
        }

        async fn add_issue_label(&self, _issue_number: u64, _label: &str) -> Result<()> {
            Ok(())
        }

        async fn remove_issue_label(&self, _issue_number: u64, _label: &str) -> Result<()> {
            Ok(())
        }

        async fn set_issue_project_state(&self, _issue_number: u64, _state: &str) -> Result<()> {
            Ok(())
        }
    }

    /// Build an applied automatic publication so correction measurement has a
    /// published route to compare against.
    fn seed_applied_automatic_publication(
        store: &mut SqliteFactoryStore,
        service: &ServiceConfig,
        issue_number: u64,
    ) -> String {
        let stage = store
            .claim_attempt(ClaimAttemptRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: issue_number.to_string(),
                issue_identifier: format!("#{issue_number}"),
                issue_revision: "rev".to_string(),
                configuration_revision: "config".to_string(),
                owner_instance: "test-owner".to_string(),
                harness: "pi".to_string(),
                model: None,
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })
            .unwrap();
        let artifact = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: stage.stage_run_id.clone(),
                issue_revision: "rev".to_string(),
                configuration_revision: "config".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: sample_artifact(),
                bytes_len: 200,
                usage: crate::triage::domain::StageUsage::default(),
            })
            .unwrap();
        let routes = &service.triage.routes;
        let intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact.run_id.clone(),
                artifact_id: Some(artifact.artifact_id.clone()),
                mode: PublicationMode::Automatic,
                intake_label: service.triage.intake_label.clone(),
                route_label: routes.implement.label.clone(),
                project_state: routes.implement.state.clone(),
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({
                    "kind": DESIRED_KIND_AUTOMATIC,
                    "route": "implement",
                    "route_labels": {
                        "implement": routes.implement.label.clone(),
                        "spec": routes.spec.label.clone(),
                        "needs_information": routes.needs_information.label.clone(),
                        "park": routes.park.label.clone(),
                        "human_owned": routes.human_owned.label.clone(),
                    },
                }),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();
        store
            .update_publication_step(
                &intent.intent_id,
                "comment_applied",
                PublicationStatus::Applied,
                None,
            )
            .unwrap();
        artifact.artifact_id
    }

    /// A maintainer replacing Symphony's route label is the disagreement signal
    /// A1 measures. It must be recorded exactly once no matter how often the
    /// reconciler re-reads the issue, otherwise the correction rate inflates on
    /// every poll.
    #[tokio::test]
    async fn human_route_swap_records_one_correction_across_repeated_polls() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let service = service_config(true, &workflow_dir);
        let mut store = open_store(&temp);
        seed_applied_automatic_publication(&mut store, &service, 42);

        // Maintainer swapped ready-for-agent for needs-info.
        let routing = Arc::new(FakeRouting {
            labels: Mutex::new(vec!["needs-info".to_string(), "bug".to_string()]),
        });
        let emitter = Arc::new(RecordingEmitter::default());
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            Arc::new(FakeIntake::default()),
            FakeComments::new("symphony-bot"),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        )
        .with_routing(routing.clone())
        .with_events(emitter.clone());

        let first = coordinator.poll_once(&service).await.unwrap();
        let second = coordinator.poll_once(&service).await.unwrap();

        assert_eq!(first.route_corrections, 1);
        assert_eq!(second.route_corrections, 0, "correction must not repeat");
        let corrected: Vec<_> = emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "triage_route_corrected")
            .cloned()
            .collect();
        assert_eq!(corrected.len(), 1);
        assert_eq!(corrected[0].1.as_deref(), Some("#42"));

        // The agreement metrics on the read surface come from the same durable
        // events, so repeated polling must not inflate them either.
        let metrics = coordinator.store_mut().triage_metrics().unwrap();
        assert_eq!(metrics.correction_count, 1);
        assert_eq!(metrics.correction_rate, 1.0);
    }

    /// Leaving Symphony's label in place is agreement, so nothing is recorded.
    #[tokio::test]
    async fn unchanged_route_label_records_no_correction() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let service = service_config(true, &workflow_dir);
        let mut store = open_store(&temp);
        seed_applied_automatic_publication(&mut store, &service, 43);

        let routing = Arc::new(FakeRouting {
            labels: Mutex::new(vec!["ready-for-agent".to_string()]),
        });
        let emitter = Arc::new(RecordingEmitter::default());
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            Arc::new(FakeIntake::default()),
            FakeComments::new("symphony-bot"),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        )
        .with_routing(routing.clone())
        .with_events(emitter.clone());

        let summary = coordinator.poll_once(&service).await.unwrap();

        assert_eq!(summary.route_corrections, 0);
        assert!(!emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "triage_route_corrected"));
    }

    /// Two route labels at once is a diagnostic, not disagreement. It must stay
    /// out of the live event vocabulary and out of the correction count, while
    /// still being durable for operators.
    #[tokio::test]
    async fn ambiguous_route_labels_record_durable_diagnostic_only() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let service = service_config(true, &workflow_dir);
        let mut store = open_store(&temp);
        seed_applied_automatic_publication(&mut store, &service, 44);

        let routing = Arc::new(FakeRouting {
            labels: Mutex::new(vec![
                "ready-for-agent".to_string(),
                "needs-info".to_string(),
            ]),
        });
        let emitter = Arc::new(RecordingEmitter::default());
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            Arc::new(FakeIntake::default()),
            FakeComments::new("symphony-bot"),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        )
        .with_routing(routing.clone())
        .with_events(emitter.clone());

        let summary = coordinator.poll_once(&service).await.unwrap();

        assert_eq!(
            summary.route_corrections, 0,
            "ambiguity is not a correction"
        );
        let live = emitter.events.lock().unwrap();
        assert!(
            !live
                .iter()
                .any(|(name, _)| name == "triage_route_corrected"),
            "ambiguity must not be counted as disagreement"
        );
        assert!(
            !live
                .iter()
                .any(|(name, _)| name == TRIAGE_ROUTE_CONSISTENCY_EVENT),
            "consistency diagnostics stay out of the live event vocabulary"
        );
        assert_eq!(
            coordinator
                .store_mut()
                .triage_metrics()
                .unwrap()
                .correction_count,
            0
        );
    }

    /// Re-applying the intake label means a new triage attempt is coming, so the
    /// old published route is no longer the decision under measurement.
    #[tokio::test]
    async fn reapplied_intake_label_suspends_correction_measurement() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let service = service_config(true, &workflow_dir);
        let mut store = open_store(&temp);
        seed_applied_automatic_publication(&mut store, &service, 45);

        let routing = Arc::new(FakeRouting {
            labels: Mutex::new(vec![
                "needs-info".to_string(),
                service.triage.intake_label.clone(),
            ]),
        });
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            Arc::new(FakeIntake::default()),
            FakeComments::new("symphony-bot"),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        )
        .with_routing(routing.clone());

        let summary = coordinator.poll_once(&service).await.unwrap();

        assert_eq!(summary.route_corrections, 0);
    }

    #[tokio::test]
    async fn orphaned_artifact_recreates_preview_intent_on_next_poll() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let mut store = open_store(&temp);
        let service = service_config(true, &workflow_dir);
        let issue = sample_issue(true, 21);
        let verified = Vec::new();
        let issue_rev = issue_revision(&build_issue_revision_input(&issue, &service, verified));
        let configuration_revision = configuration_revision(&ConfigurationRevisionInput {
            prompt: "triage prompt".to_string(),
            turn_timeout_ms: service.triage.turn_timeout_ms,
            max_attempts: service.triage.max_attempts,
            harness: "pi".to_string(),
            model: None,
            routes: service.triage.routes.clone(),
        });
        let mapping_hash = route_mapping_hash(&service.triage.routes);

        let stage = store
            .claim_attempt(ClaimAttemptRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: issue.issue_id.clone(),
                issue_identifier: issue.identifier.clone(),
                issue_revision: issue_rev.clone(),
                configuration_revision: configuration_revision.clone(),
                owner_instance: "test-owner".to_string(),
                harness: "pi".to_string(),
                model: None,
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })
            .unwrap();
        let artifact = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: stage.stage_run_id,
                issue_revision: issue_rev,
                configuration_revision,
                route_mapping_hash: mapping_hash,
                artifact: sample_artifact(),
                bytes_len: 32,
                usage: StageUsage::default(),
            })
            .unwrap();
        // Simulate crash after store_artifact and before create_publication_intent.
        assert!(store
            .list_intents_for_run(&artifact.run_id)
            .unwrap()
            .is_empty());

        let intake = Arc::new(FakeIntake::default());
        intake.issues.lock().unwrap().push(issue);
        let comments = FakeComments::new("symphony-bot");
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            intake,
            comments.clone(),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        );

        let summary = coordinator.poll_once(&service).await.unwrap();

        // No new attempt; orphaned artifact is recovered via a recreated intent.
        assert_eq!(summary.attempts_started, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        let intent = coordinator
            .store_mut()
            .get_latest_publication(&artifact.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            intent.artifact_id.as_deref(),
            Some(artifact.artifact_id.as_str())
        );
        assert_eq!(intent.status, PublicationStatus::Applied);
        let run = coordinator
            .store_mut()
            .get_run_by_id(&artifact.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(run.status, FactoryRunStatus::Completed);
    }

    #[tokio::test]
    async fn disabled_triage_still_reconciles_pending() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = prep_workflow(&temp);
        let mut store = open_store(&temp);
        let run = store
            .upsert_factory_run(UpsertFactoryRunRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: "9".to_string(),
                issue_identifier: "#9".to_string(),
                issue_revision: None,
                status: FactoryRunStatus::Ineligible,
                current_stage: Some(TRIAGE_STAGE_NAME.to_string()),
            })
            .unwrap();
        store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: run.run_id,
                artifact_id: None,
                mode: PublicationMode::Preview,
                intake_label: "needs-triage".to_string(),
                route_label: String::new(),
                project_state: None,
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({
                    "kind": DESIRED_KIND_INELIGIBLE,
                    "project_name": "Factory",
                    "remediation": INELIGIBLE_REMEDIATION,
                }),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();

        let intake = Arc::new(FakeIntake::default());
        intake.issues.lock().unwrap().push(sample_issue(true, 9));
        let comments = FakeComments::new("symphony-bot");
        let mut coordinator = TriageCoordinator::with_executor(
            store,
            intake,
            comments.clone(),
            TriageCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "test-owner".to_string(),
                workflow_dir: workflow_dir.clone(),
                project_display_name: "Factory".to_string(),
            },
            FakeExecutor::success(sample_artifact()),
        );

        let summary = coordinator
            .poll_once(&service_config(false, &workflow_dir))
            .await
            .unwrap();

        assert!(!summary.triage_enabled);
        assert_eq!(summary.issues_seen, 0);
        assert_eq!(summary.attempts_started, 0);
        assert_eq!(summary.reconciled_intents, 1);
        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        assert!(*comments.list_count.lock().unwrap() >= 1);
    }

    #[test]
    fn require_repo_path_resolves_dot_against_process_cwd() {
        let mut service = ServiceConfig::default();
        service.workspace.repo = Some(".".to_string());

        let resolved = require_repo_path(&service).expect("relative repo should resolve");
        let cwd = std::env::current_dir().expect("cwd");
        let expected = path_safety::canonicalize(&cwd).expect("cwd canonicalize");

        assert!(resolved.is_absolute(), "resolved path must be absolute");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn require_repo_path_rejects_remote_urls() {
        let mut service = ServiceConfig::default();
        service.workspace.repo = Some("https://github.com/org/repo.git".to_string());

        let err = require_repo_path(&service).expect_err("remote repo must fail");
        assert!(
            err.to_string().contains("local path"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn resolve_workspace_path_expands_relative_root() {
        let resolved =
            resolve_workspace_path(".symphony/workspaces", "workspace.root").expect("resolve");
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with(std::path::Path::new(".symphony/workspaces")));
    }
}
