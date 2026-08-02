use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AgentBackend, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::spec::comment::{render_spec_comment, SpecCommentState};
use crate::spec::decision::{detect_decision, DecisionAction, DecisionInput};
use crate::spec::domain::{
    SpecArtifactRecord, SpecPublicationIntent, SpecPublicationKind, SpecTurnStatus,
    SPEC_COMMENT_MARKER_PREFIX, SPEC_STAGE_NAME,
};
use crate::spec::pipeline::{
    resolve_models, run_pipeline, SeededRevision, SpecPipelineConfig, SpecPipelinePrompts,
    SpecPipelineRequest, SpecTurnExecutor, SpecTurnOutcome, SpecTurnOutput, SpecTurnRequest,
};
use crate::triage::coordinator::EventEmitter;
use crate::triage::domain::{
    FactoryError, FactoryEventRecord, FactoryRunStatus, PublicationStatus,
};
use crate::triage::intake::{TriageIntakeIssue, TriageIntakePort};
use crate::triage::publisher::{TriageCommentPort, TriageRoutingPort};
use crate::triage::runner::{
    run_isolated_spec_turn, IsolatedSpecRunnerConfig, TriageHarness, TriageIssueIdentity,
};
use crate::triage::runtime::SharedFactoryStore;
use crate::triage::store::{
    ClaimAttemptRequest, FactoryRunStore, RecordAttemptProcessRequest, StoreSpecArtifactRequest,
    StoreSpecTurnRequest, UpsertFactoryRunRequest,
};

/// After this many consecutive reconcile failures, a spec publication intent is
/// marked blocked so one poison intent cannot stall the whole stage forever.
const SPEC_PUBLICATION_MAX_RETRIES: u32 = 5;

#[derive(Debug, Clone)]
pub struct SpecCoordinatorConfig {
    pub forge_host: String,
    pub repository: String,
    pub owner_instance: String,
    pub workflow_dir: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecPollSummary {
    pub spec_enabled: bool,
    pub issues_seen: u32,
    pub attempts_started: u32,
    pub attempts_completed: u32,
    pub attempts_failed: u32,
    pub ineligible: u32,
    pub skipped: u32,
    pub published: u32,
    pub approved: u32,
    pub revisions_requested: u32,
}

pub struct SpecCoordinator<I, C> {
    store: SharedFactoryStore,
    intake: I,
    comments: C,
    routing: std::sync::Arc<dyn TriageRoutingPort>,
    config: SpecCoordinatorConfig,
    events: Option<std::sync::Arc<dyn EventEmitter>>,
}

impl<I, C> SpecCoordinator<I, C>
where
    I: TriageIntakePort,
    C: TriageCommentPort + Clone,
{
    pub fn new(
        store: SharedFactoryStore,
        intake: I,
        comments: C,
        routing: impl TriageRoutingPort + 'static,
        config: SpecCoordinatorConfig,
    ) -> Self {
        Self {
            store,
            intake,
            comments,
            routing: std::sync::Arc::new(routing),
            config,
            events: None,
        }
    }

    pub fn with_events(mut self, events: std::sync::Arc<dyn EventEmitter>) -> Self {
        self.events = Some(events);
        self
    }

    pub async fn poll_once(&mut self, service: &ServiceConfig) -> Result<SpecPollSummary> {
        let mut summary = SpecPollSummary {
            spec_enabled: service.spec.enabled,
            ..Default::default()
        };
        // Spec-only deployments never construct TriageCoordinator, which is otherwise
        // the sole caller of interrupt_stale_attempts. Without this, a restart mid-turn
        // leaves the stage row `running` and the uniqueness index blocks every retry.
        self.store.interrupt_stale_attempts()?;
        self.reconcile_pending_publications(service).await?;
        if !service.spec.enabled {
            return Ok(summary);
        }

        let prompts = load_prompts(&self.config.workflow_dir, service)?;
        let configuration_revision = spec_configuration_revision(service, &prompts);
        let publisher_login = self.comments.authenticated_login().await?;
        let issues = self
            .intake
            .fetch_intake_issues(&service.spec.intake_label, service.spec.max_intake_pages)
            .await?;
        summary.issues_seen = issues.len() as u32;

        for issue in issues {
            if has_label(&issue.labels, &service.triage.intake_label) && service.triage.enabled {
                // Triage owns this issue's comment surface, so the only record is the
                // durable event. Emit exactly one per (issue, issue_revision) instead of
                // one per poll, which would otherwise grow without bound while a human
                // leaves both intake labels applied.
                let issue_revision = spec_issue_revision(&issue, service, &publisher_login);
                let mut store = self.store.clone();
                let already_recorded = store
                    .get_run_by_issue(
                        &self.config.forge_host,
                        &self.config.repository,
                        &issue.issue_id,
                    )?
                    .is_some_and(|run| {
                        run.status == FactoryRunStatus::Ineligible
                            && run.issue_revision.as_deref() == Some(issue_revision.as_str())
                    });
                if !already_recorded {
                    let run = store.upsert_factory_run(UpsertFactoryRunRequest {
                        forge_host: self.config.forge_host.clone(),
                        repository: self.config.repository.clone(),
                        issue_id: issue.issue_id.clone(),
                        issue_identifier: issue.identifier.clone(),
                        issue_revision: Some(issue_revision),
                        status: FactoryRunStatus::Ineligible,
                        current_stage: Some(SPEC_STAGE_NAME.to_string()),
                    })?;
                    self.record_event(
                        Some(&run.run_id),
                        None,
                        "spec_ineligible",
                        serde_json::json!({
                            "issue": issue.identifier,
                            "status": "ineligible",
                            "error_code": "intake_label_conflict"
                        }),
                    )?;
                }
                summary.skipped += 1;
                continue;
            }
            if !issue.in_project {
                self.handle_ineligible(&issue, service).await?;
                summary.ineligible += 1;
                continue;
            }

            let issue_revision = spec_issue_revision(&issue, service, &publisher_login);
            let existing_run = {
                let store = self.store.clone();
                store.get_run_by_issue(
                    &self.config.forge_host,
                    &self.config.repository,
                    &issue.issue_id,
                )?
            };
            let mut seed = None;
            if let Some(run) = existing_run.as_ref() {
                let artifacts = self.store.list_spec_artifacts(&run.run_id)?;
                if let Some(latest) = artifacts.first() {
                    // The feedback cutoff is the last time the spec itself was
                    // published. Diagnostics republish the same owned comment, so
                    // using the newest intent of any kind would advance the cutoff
                    // past the human's feedback on every poll and the revision
                    // request would never be detected.
                    let publication = self.store.latest_spec_publication_of_kind(
                        &run.run_id,
                        SpecPublicationKind::Preview,
                    )?;
                    let published_at = publication
                        .as_ref()
                        .map(|intent| intent.updated_at)
                        .unwrap_or(latest.received_at);
                    let action = detect_decision(DecisionInput {
                        labels: &issue.labels,
                        comments: &issue.comments,
                        publisher_login: &publisher_login,
                        published_at,
                        revision_is_current: latest.issue_revision == issue_revision
                            && latest.configuration_revision == configuration_revision,
                        intake_revision_changed: latest.issue_revision != issue_revision
                            || latest.configuration_revision != configuration_revision,
                        config: &service.spec,
                    });
                    match action {
                        DecisionAction::Conflict => {
                            self.publish_diagnostic(
                                &issue,
                                run.run_id.as_str(),
                                latest,
                                "Both `spec-approved` and `spec-revise` are present. Remove one label.",
                                service.spec.max_intake_pages,
                            )
                            .await?;
                            summary.skipped += 1;
                            continue;
                        }
                        DecisionAction::Revise { feedback } => {
                            // `max_revision_requests` bounds human revision requests,
                            // while `max_attempts` bounds agent-failure retries for a
                            // revision pair. Count the request only when this pair is
                            // new, otherwise a few failed turns silently exhaust the
                            // human's revision budget for the whole run.
                            let prior_attempts = self
                                .store
                                .list_stage_attempts_for_revision(
                                    SPEC_STAGE_NAME,
                                    &run.run_id,
                                    &issue_revision,
                                    &configuration_revision,
                                )?
                                .len();
                            if prior_attempts == 0 {
                                if let Err(error) = self.store.increment_spec_revision_requests(
                                    &run.run_id,
                                    service.spec.max_revision_requests,
                                ) {
                                    self.publish_diagnostic(
                                        &issue,
                                        &run.run_id,
                                        latest,
                                        &format!(
                                            "Revision request budget exhausted ({error}). Remove `{revise}` or raise `spec.max_revision_requests`.",
                                            revise = service.spec.labels.revise
                                        ),
                                        service.spec.max_intake_pages,
                                    )
                                    .await?;
                                    summary.skipped += 1;
                                    continue;
                                }
                            }
                            seed = Some(SeededRevision {
                                prior_version: latest.version,
                                prior_spec: latest.artifact.clone(),
                                feedback: feedback
                                    .into_iter()
                                    .map(|comment| comment.body)
                                    .collect(),
                            });
                            summary.revisions_requested += 1;
                            self.record_event(
                                Some(&run.run_id),
                                None,
                                "spec_revision_requested",
                                serde_json::json!({"version": latest.version, "status": "running"}),
                            )?;
                        }
                        DecisionAction::ReviseWithoutFeedback => {
                            self.publish_diagnostic(
                                &issue,
                                &run.run_id,
                                latest,
                                "Add a feedback comment, then leave `spec-revise` applied.",
                                service.spec.max_intake_pages,
                            )
                            .await?;
                            summary.skipped += 1;
                            continue;
                        }
                        DecisionAction::Approve => {
                            self.approve(&issue, run.run_id.as_str(), latest, service)
                                .await?;
                            summary.approved += 1;
                            continue;
                        }
                        DecisionAction::StaleApproval => {
                            self.publish_diagnostic(
                                &issue,
                                &run.run_id,
                                latest,
                                "Approval is stale because the issue or spec configuration changed. Remove `spec-approved`, review a new version, and approve again.",
                                service.spec.max_intake_pages,
                            )
                            .await?;
                            self.record_event(
                                Some(&run.run_id),
                                None,
                                "spec_publication_conflict",
                                serde_json::json!({"status":"conflict","error_code":"stale_approval"}),
                            )?;
                            summary.skipped += 1;
                            continue;
                        }
                        DecisionAction::None
                            if latest.issue_revision == issue_revision
                                && latest.configuration_revision == configuration_revision =>
                        {
                            // Recover when store_spec_artifact committed but the
                            // subsequent preview-intent create/publish crashed.
                            let preview = self.store.latest_spec_publication_of_kind(
                                &run.run_id,
                                SpecPublicationKind::Preview,
                            )?;
                            let preview_covers_latest = preview.as_ref().is_some_and(|intent| {
                                intent.artifact_id.as_deref() == Some(latest.artifact_id.as_str())
                            });
                            if !preview_covers_latest {
                                let stage = self
                                    .store
                                    .get_stage_run(&latest.stage_run_id)?
                                    .ok_or_else(|| {
                                        SymphonyError::StorageError(format!(
                                            "spec stage run {} missing for orphaned artifact {}",
                                            latest.stage_run_id, latest.artifact_id
                                        ))
                                    })?;
                                self.publish_artifact(
                                    &issue,
                                    &stage,
                                    latest,
                                    &service.spec.labels.approved,
                                    &service.spec.labels.revise,
                                    service.spec.max_intake_pages,
                                )
                                .await?;
                                summary.published += 1;
                                continue;
                            }
                            summary.skipped += 1;
                            continue;
                        }
                        DecisionAction::None | DecisionAction::ColdRevision => {}
                    }
                }
            }

            summary.attempts_started += 1;
            match self
                .run_attempt(
                    &issue,
                    service,
                    &prompts,
                    &configuration_revision,
                    &issue_revision,
                    seed,
                )
                .await
            {
                Ok(()) => {
                    summary.attempts_completed += 1;
                    summary.published += 1;
                }
                Err(error) => {
                    summary.attempts_failed += 1;
                    tracing::warn!(issue = %issue.identifier, error = %error, "spec attempt failed");
                }
            }
        }
        Ok(summary)
    }

    async fn reconcile_pending_publications(&self, service: &ServiceConfig) -> Result<()> {
        for intent in self.store.list_pending_spec_publications()? {
            if let Err(error) = self.reconcile_one_publication(service, &intent).await {
                let next_retries = intent.retry_count.saturating_add(1);
                let status = if next_retries >= SPEC_PUBLICATION_MAX_RETRIES {
                    PublicationStatus::Blocked
                } else {
                    PublicationStatus::Pending
                };
                let factory_error = FactoryError::new(
                    "spec_publication_failed",
                    "spec_coordinator",
                    error.to_string(),
                    status == PublicationStatus::Pending,
                    None,
                );
                if let Err(persist_error) = self.store.update_spec_publication_step(
                    &intent.intent_id,
                    "",
                    status,
                    Some(factory_error),
                ) {
                    tracing::warn!(
                        intent_id = %intent.intent_id,
                        error = %persist_error,
                        "failed to persist spec publication failure"
                    );
                }
                tracing::warn!(
                    intent_id = %intent.intent_id,
                    error = %error,
                    retry_count = next_retries,
                    status = status.as_str(),
                    "spec publication reconcile failed; continuing with remaining intents"
                );
            }
        }
        Ok(())
    }

    async fn reconcile_one_publication(
        &self,
        service: &ServiceConfig,
        intent: &SpecPublicationIntent,
    ) -> Result<()> {
        let issue_number = intent
            .desired_effects
            .get("issue_number")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "spec intent {} is missing issue_number",
                    intent.intent_id
                ))
            })?;
        let artifact = intent
            .artifact_id
            .as_deref()
            .map(|id| self.store.get_spec_artifact(id))
            .transpose()?
            .flatten();
        match intent.kind {
            SpecPublicationKind::Preview => {
                let artifact = artifact.ok_or_else(|| {
                    SymphonyError::StorageError(format!(
                        "spec preview intent {} has no artifact",
                        intent.intent_id
                    ))
                })?;
                let stage = {
                    let store = self.store.clone();
                    store
                        .get_stage_run(&artifact.stage_run_id)?
                        .ok_or_else(|| {
                            SymphonyError::StorageError("spec stage run is missing".to_string())
                        })?
                };
                let approved_label = intent
                    .desired_effects
                    .get("approved_label")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("spec-approved");
                let revise_label = intent
                    .desired_effects
                    .get("revise_label")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("spec-revise");
                let body = render_spec_comment(
                    &intent.intent_id,
                    &intent.run_id,
                    &artifact.stage_run_id,
                    stage.attempt,
                    artifact.version,
                    &artifact.artifact,
                    &artifact.unresolved_blocking_findings,
                    SpecCommentState::AwaitingDecision {
                        approved_label,
                        revise_label,
                    },
                );
                self.upsert_owned_comment(
                    issue_number,
                    intent,
                    &body,
                    service.spec.max_intake_pages,
                )
                .await?;
                if let Some(revise_label) = intent
                    .desired_effects
                    .get("revise_label")
                    .and_then(serde_json::Value::as_str)
                {
                    self.remove_label_if_present(issue_number, revise_label)
                        .await?;
                }
                self.store
                    .complete_spec_publication(&intent.intent_id, "spec_comment")?;
            }
            SpecPublicationKind::Diagnostic => {
                // Diagnostics with an artifact can be reconstructed exactly.
                if let Some(artifact) = artifact {
                    let message = intent
                        .desired_effects
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Review the issue labels and feedback, then retry.");
                    let body = render_spec_comment(
                        &intent.intent_id,
                        &intent.run_id,
                        &artifact.stage_run_id,
                        0,
                        artifact.version,
                        &artifact.artifact,
                        &artifact.unresolved_blocking_findings,
                        SpecCommentState::Diagnostic(message),
                    );
                    self.upsert_owned_comment(
                        issue_number,
                        intent,
                        &body,
                        service.spec.max_intake_pages,
                    )
                    .await?;
                    self.store
                        .complete_spec_publication(&intent.intent_id, "diagnostic_comment")?;
                } else {
                    let intake_label = intent
                        .desired_effects
                        .get("intake_label")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("ready-to-spec");
                    let body = format!(
                        "<!-- symphony:spec:{} -->\n## Symphony specification\n\nThis issue is not a member of the configured GitHub Project. Add it to the project and leave `{intake_label}` applied.",
                        intent.intent_id
                    );
                    self.upsert_owned_comment(
                        issue_number,
                        intent,
                        &body,
                        service.spec.max_intake_pages,
                    )
                    .await?;
                    self.store
                        .complete_spec_publication(&intent.intent_id, "diagnostic_comment")?;
                }
            }
            SpecPublicationKind::Approval => {
                let artifact = artifact.ok_or_else(|| {
                    SymphonyError::StorageError(format!(
                        "spec approval intent {} has no artifact",
                        intent.intent_id
                    ))
                })?;
                let intake_label = intent
                    .desired_effects
                    .get("intake_label")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("ready-to-spec");
                let route_label = intent
                    .desired_effects
                    .get("route_label")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        SymphonyError::StorageError(
                            "spec approval intent is missing route_label".to_string(),
                        )
                    })?;
                let approved_label = intent
                    .desired_effects
                    .get("approved_label")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("spec-approved");
                let revise_label = intent
                    .desired_effects
                    .get("revise_label")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("spec-revise");
                for label in [intake_label, approved_label, revise_label] {
                    self.remove_label_if_present(issue_number, label).await?;
                }
                self.routing
                    .add_issue_label(issue_number, route_label)
                    .await?;
                let project_state = intent
                    .desired_effects
                    .get("project_state")
                    .and_then(serde_json::Value::as_str);
                if let Some(state) = project_state {
                    self.routing
                        .set_issue_project_state(issue_number, state)
                        .await?;
                }
                self.store
                    .pin_spec_approval(&intent.run_id, &artifact.artifact_id)?;
                let body = render_spec_comment(
                    &intent.intent_id,
                    &intent.run_id,
                    &artifact.stage_run_id,
                    0,
                    artifact.version,
                    &artifact.artifact,
                    &artifact.unresolved_blocking_findings,
                    SpecCommentState::Approved {
                        route_label,
                        project_state,
                    },
                );
                self.upsert_owned_comment(
                    issue_number,
                    intent,
                    &body,
                    service.spec.max_intake_pages,
                )
                .await?;
                self.store.finalize_spec_approval(
                    &intent.run_id,
                    &intent.intent_id,
                    "comment_final",
                )?;
                // A crash between the decision and the final step resumes here, so
                // the approval events must be emitted on this path too. Without
                // them a recovered approval is invisible to event consumers.
                self.record_event(
                    Some(&intent.run_id),
                    None,
                    "spec_route_applied",
                    serde_json::json!({"intent_id":intent.intent_id,"version":artifact.version,"status":"applied"}),
                )?;
                self.record_event(
                    Some(&intent.run_id),
                    None,
                    "spec_approved",
                    serde_json::json!({"intent_id":intent.intent_id,"artifact_id":artifact.artifact_id,"version":artifact.version,"status":"approved"}),
                )?;
            }
        }
        Ok(())
    }

    async fn run_attempt(
        &mut self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        prompts: &LoadedPrompts,
        configuration_revision: &str,
        issue_revision: &str,
        seed: Option<SeededRevision>,
    ) -> Result<()> {
        let harness = match service.agent_backend {
            AgentBackend::Codex => TriageHarness::Codex,
            AgentBackend::KataCli => TriageHarness::Pi,
        };
        let (draft_model, review_model) = resolve_models(
            service.spec.model.as_deref(),
            service.spec.review_model.as_deref(),
            service.pi_agent.model.as_deref(),
        );
        // Resolve every fallible input before claiming the attempt. A failure after
        // the claim would leave a nonterminal attempt that blocks the stage-scoped
        // uniqueness index on every later poll.
        let workspace_root = resolve_path(&service.workspace.root, "workspace.root")?;
        let repo_path = resolve_path(
            service.workspace.repo.as_deref().ok_or_else(|| {
                SymphonyError::InvalidWorkflowConfig(
                    "workspace.repo is required for the spec stage".to_string(),
                )
            })?,
            "workspace.repo",
        )?;
        let command = agent_command(service)?;
        let store = self.store.clone();
        if let Some(run) = store.get_run_by_issue(
            &self.config.forge_host,
            &self.config.repository,
            &issue.issue_id,
        )? {
            let attempts = store.list_stage_attempts_for_revision(
                SPEC_STAGE_NAME,
                &run.run_id,
                issue_revision,
                configuration_revision,
            )?;
            let spec_attempts = attempts.len() as u32;
            if spec_attempts >= service.spec.max_attempts {
                return Err(SymphonyError::TriageError(format!(
                    "spec.max_attempts exhausted for {}",
                    issue.identifier
                )));
            }
        }
        let stage = self.store.claim_spec_attempt(ClaimAttemptRequest {
            forge_host: self.config.forge_host.clone(),
            repository: self.config.repository.clone(),
            issue_id: issue.issue_id.clone(),
            issue_identifier: issue.identifier.clone(),
            issue_revision: issue_revision.to_string(),
            configuration_revision: configuration_revision.to_string(),
            owner_instance: self.config.owner_instance.clone(),
            harness: harness_name(harness).to_string(),
            model: draft_model.clone(),
            workspace_path: None,
            output_path: None,
            pid: None,
            process_group_id: None,
            process_start_token: None,
            executable_identity: None,
        })?;
        self.record_event(
            Some(&stage.run_id),
            Some(&stage.stage_run_id),
            "spec_started",
            serde_json::json!({"status":"running","attempt":stage.attempt}),
        )?;

        // Every failure after the claim must terminate the attempt, otherwise the
        // nonterminal row blocks this revision pair on every later poll.
        let outcome = self
            .execute_claimed_attempt(
                issue,
                service,
                prompts,
                configuration_revision,
                issue_revision,
                seed,
                &stage,
                harness,
                draft_model,
                review_model,
                workspace_root,
                repo_path,
                command,
            )
            .await;
        if let Err(error) = &outcome {
            let factory_error = FactoryError::new(
                "spec_attempt_failed",
                "spec_coordinator",
                error.to_string(),
                true,
                None,
            );
            let mut store = self.store.clone();
            store.fail_attempt(&stage.stage_run_id, factory_error)?;
            self.record_event(
                Some(&stage.run_id),
                Some(&stage.stage_run_id),
                "spec_failed",
                serde_json::json!({"status":"failed","error_code":"spec_attempt_failed"}),
            )?;
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_claimed_attempt(
        &mut self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
        prompts: &LoadedPrompts,
        configuration_revision: &str,
        issue_revision: &str,
        seed: Option<SeededRevision>,
        stage: &crate::triage::domain::StageRunRecord,
        harness: TriageHarness,
        draft_model: Option<String>,
        review_model: Option<String>,
        workspace_root: PathBuf,
        repo_path: PathBuf,
        command: Vec<String>,
    ) -> Result<()> {
        let runner = IsolatedSpecRunnerConfig {
            workspace_root,
            repo_path,
            command,
            harness,
            issue: TriageIssueIdentity {
                id: issue.issue_id.clone(),
                identifier: issue.identifier.clone(),
                title: issue.title.clone(),
            },
            codex: (harness == TriageHarness::Codex).then(|| service.codex.clone()),
            spawned: {
                let store = self.store.clone();
                let stage_run_id = stage.stage_run_id.clone();
                let owner_instance = self.config.owner_instance.clone();
                Some(std::sync::Arc::new(
                    move |spawn: crate::triage::runner::TriageSpawnInfo| {
                        let mut durable = store.clone();
                        if let Err(error) =
                            durable.record_attempt_process(RecordAttemptProcessRequest {
                                stage_run_id: stage_run_id.clone(),
                                owner_instance: owner_instance.clone(),
                                identity: spawn.identity,
                                workspace_path: Some(
                                    spawn.workspace_path.to_string_lossy().to_string(),
                                ),
                                output_path: Some(spawn.output_path.to_string_lossy().to_string()),
                            })
                        {
                            tracing::error!(%error, "failed to persist spawned spec process identity");
                        }
                    },
                ))
            },
        };
        let durable_runner = DurableSpecExecutor {
            runner,
            store: self.store.clone(),
            stage_run_id: stage.stage_run_id.clone(),
            harness: harness_name(harness).to_string(),
        };
        let pipeline = run_pipeline(
            &durable_runner,
            SpecPipelineRequest {
                stage_run_id: stage.stage_run_id.clone(),
                issue_context: issue_context(issue),
                seed,
                config: SpecPipelineConfig {
                    max_review_cycles: service.spec.max_review_cycles,
                    turn_timeout_ms: service.spec.turn_timeout_ms,
                    harness: harness_name(harness).to_string(),
                    draft_model,
                    review_model,
                    prompts: SpecPipelinePrompts {
                        draft: prompts.draft.clone(),
                        review: prompts.review.clone(),
                        revise: prompts.revise.clone(),
                    },
                },
            },
        )
        .await?;
        for turn in &pipeline.turns {
            self.record_event(
                Some(&stage.run_id),
                Some(&stage.stage_run_id),
                "spec_turn_completed",
                serde_json::json!({"turn_id":turn.turn_id,"kind":turn.kind.as_str(),"status":"completed"}),
            )?;
        }
        let bytes_len = serde_json::to_vec(&pipeline.artifact)
            .map_err(|error| SymphonyError::StorageError(error.to_string()))?
            .len() as u64;
        let artifact = self.store.store_spec_artifact(StoreSpecArtifactRequest {
            stage_run_id: stage.stage_run_id.clone(),
            issue_revision: issue_revision.to_string(),
            configuration_revision: configuration_revision.to_string(),
            artifact: pipeline.artifact,
            review_cycles: pipeline.review_cycles,
            unresolved_blocking_findings: pipeline.unresolved_blocking_findings,
            bytes_len,
            usage: pipeline.usage,
        })?;
        self.record_event(
            Some(&stage.run_id),
            Some(&stage.stage_run_id),
            "spec_completed",
            serde_json::json!({"artifact_id":artifact.artifact_id,"version":artifact.version,"status":"completed"}),
        )?;
        self.publish_artifact(
            issue,
            stage,
            &artifact,
            &service.spec.labels.approved,
            &service.spec.labels.revise,
            service.spec.max_intake_pages,
        )
        .await
    }

    async fn publish_artifact(
        &self,
        issue: &TriageIntakeIssue,
        stage: &crate::triage::domain::StageRunRecord,
        artifact: &SpecArtifactRecord,
        approved_label: &str,
        revise_label: &str,
        max_pages: u32,
    ) -> Result<()> {
        let intent = self.store.create_spec_publication_intent(
            &stage.run_id,
            Some(&artifact.artifact_id),
            SpecPublicationKind::Preview,
            &serde_json::json!({
                "issue_number":issue.issue_number,
                "version":artifact.version,
                "approved_label":approved_label,
                "revise_label":revise_label,
            }),
        )?;
        let body = render_spec_comment(
            &intent.intent_id,
            &stage.run_id,
            &stage.stage_run_id,
            stage.attempt,
            artifact.version,
            &artifact.artifact,
            &artifact.unresolved_blocking_findings,
            SpecCommentState::AwaitingDecision {
                approved_label,
                revise_label,
            },
        );
        self.upsert_owned_comment(issue.issue_number, &intent, &body, max_pages)
            .await?;
        if has_label(&issue.labels, revise_label) {
            self.routing
                .remove_issue_label(issue.issue_number, revise_label)
                .await?;
        }
        self.store
            .complete_spec_publication(&intent.intent_id, "spec_comment")?;
        self.record_event(
            Some(&stage.run_id),
            Some(&stage.stage_run_id),
            "spec_published",
            serde_json::json!({"intent_id":intent.intent_id,"artifact_id":artifact.artifact_id,"version":artifact.version,"status":"applied"}),
        )
    }

    async fn approve(
        &self,
        issue: &TriageIntakeIssue,
        run_id: &str,
        artifact: &SpecArtifactRecord,
        service: &ServiceConfig,
    ) -> Result<()> {
        let intent = self.store.create_spec_publication_intent(
            run_id,
            Some(&artifact.artifact_id),
            SpecPublicationKind::Approval,
            &serde_json::json!({
                "issue_number":issue.issue_number,
                "version":artifact.version,
                "intake_label":service.spec.intake_label,
                "approved_label":service.spec.labels.approved,
                "revise_label":service.spec.labels.revise,
                "route_label":service.spec.approval_route.label,
                "project_state":service.spec.approval_route.state,
            }),
        )?;
        self.store
            .set_pending_spec_approval(run_id, artifact.version)?;
        let pending = render_spec_comment(
            &intent.intent_id,
            run_id,
            &artifact.stage_run_id,
            0,
            artifact.version,
            &artifact.artifact,
            &artifact.unresolved_blocking_findings,
            SpecCommentState::ApprovalPending,
        );
        self.upsert_owned_comment(
            issue.issue_number,
            &intent,
            &pending,
            service.spec.max_intake_pages,
        )
        .await?;
        // Ordered, idempotent tracker effects. GitHub returns 404 when removing a
        // label the issue does not carry, so read current labels first and remove
        // only what is present. Removing unconditionally aborts the approval
        // mid-flight and leaves the run pinned at `pending_approval`.
        for label in [
            service.spec.intake_label.as_str(),
            service.spec.labels.approved.as_str(),
            service.spec.labels.revise.as_str(),
        ] {
            self.remove_label_if_present(issue.issue_number, label)
                .await?;
        }
        self.routing
            .add_issue_label(issue.issue_number, &service.spec.approval_route.label)
            .await?;
        if let Some(state) = service.spec.approval_route.state.as_deref() {
            self.routing
                .set_issue_project_state(issue.issue_number, state)
                .await?;
        }
        self.store
            .pin_spec_approval(run_id, &artifact.artifact_id)?;
        let approved = render_spec_comment(
            &intent.intent_id,
            run_id,
            &artifact.stage_run_id,
            0,
            artifact.version,
            &artifact.artifact,
            &artifact.unresolved_blocking_findings,
            SpecCommentState::Approved {
                route_label: &service.spec.approval_route.label,
                project_state: service.spec.approval_route.state.as_deref(),
            },
        );
        self.upsert_owned_comment(
            issue.issue_number,
            &intent,
            &approved,
            service.spec.max_intake_pages,
        )
        .await?;
        self.store
            .finalize_spec_approval(run_id, &intent.intent_id, "comment_final")?;
        self.record_event(
            Some(run_id),
            None,
            "spec_route_applied",
            serde_json::json!({"intent_id":intent.intent_id,"version":artifact.version,"status":"applied"}),
        )?;
        self.record_event(
            Some(run_id),
            None,
            "spec_approved",
            serde_json::json!({"intent_id":intent.intent_id,"artifact_id":artifact.artifact_id,"version":artifact.version,"status":"approved"}),
        )
    }

    /// GitHub returns 404 when removing a label the issue does not carry, so an
    /// unconditional removal aborts a partially applied publication on retry.
    /// Reading current labels first keeps every label step idempotent.
    async fn remove_label_if_present(&self, issue_number: u64, label: &str) -> Result<()> {
        let current = self.routing.list_issue_labels(issue_number).await?;
        if current
            .iter()
            .any(|present| present.eq_ignore_ascii_case(label))
        {
            self.routing.remove_issue_label(issue_number, label).await?;
        }
        Ok(())
    }

    async fn publish_diagnostic(
        &self,
        issue: &TriageIntakeIssue,
        run_id: &str,
        artifact: &SpecArtifactRecord,
        message: &str,
        max_pages: u32,
    ) -> Result<()> {
        // Reuse the run's diagnostic intent so a human who leaves the triggering
        // condition in place does not accumulate one intent per poll.
        let existing = self
            .store
            .find_spec_publication_by_kind(run_id, SpecPublicationKind::Diagnostic)?;
        let unchanged = existing.as_ref().is_some_and(|intent| {
            intent.status == crate::triage::domain::PublicationStatus::Applied
                && intent
                    .desired_effects
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    == Some(message)
        });
        if unchanged {
            return Ok(());
        }
        let intent = match existing {
            Some(intent) => self.store.update_spec_diagnostic_message(
                &intent.intent_id,
                &serde_json::json!({"issue_number":issue.issue_number,"message":message}),
            )?,
            None => self.store.create_spec_publication_intent(
                run_id,
                Some(&artifact.artifact_id),
                SpecPublicationKind::Diagnostic,
                &serde_json::json!({"issue_number":issue.issue_number,"message":message}),
            )?,
        };
        let body = render_spec_comment(
            &intent.intent_id,
            run_id,
            &artifact.stage_run_id,
            0,
            artifact.version,
            &artifact.artifact,
            &artifact.unresolved_blocking_findings,
            SpecCommentState::Diagnostic(message),
        );
        self.upsert_owned_comment(issue.issue_number, &intent, &body, max_pages)
            .await?;
        self.store
            .complete_spec_publication(&intent.intent_id, "diagnostic_comment")
    }

    async fn handle_ineligible(
        &self,
        issue: &TriageIntakeIssue,
        service: &ServiceConfig,
    ) -> Result<()> {
        let mut store = self.store.clone();
        let run = store.upsert_factory_run(UpsertFactoryRunRequest {
            forge_host: self.config.forge_host.clone(),
            repository: self.config.repository.clone(),
            issue_id: issue.issue_id.clone(),
            issue_identifier: issue.identifier.clone(),
            issue_revision: None,
            status: FactoryRunStatus::Ineligible,
            current_stage: Some(SPEC_STAGE_NAME.to_string()),
        })?;
        // Reuse the run's existing diagnostic intent. Creating a new one each poll
        // would grow the intent table and re-emit `spec_ineligible` forever while an
        // issue stays off-project, against the "one idempotent diagnostic" contract.
        let existing = self
            .store
            .find_spec_publication_by_kind(&run.run_id, SpecPublicationKind::Diagnostic)?;
        let already_applied = existing.as_ref().is_some_and(|intent| {
            intent.status == crate::triage::domain::PublicationStatus::Applied
        });
        let intent = match existing {
            Some(intent) => intent,
            None => self.store.create_spec_publication_intent(
                &run.run_id,
                None,
                SpecPublicationKind::Diagnostic,
                &serde_json::json!({
                    "issue_number":issue.issue_number,
                    "kind":"off_project",
                    "intake_label":service.spec.intake_label,
                }),
            )?,
        };
        if already_applied {
            return Ok(());
        }
        let body = format!(
            "<!-- symphony:spec:{} -->\n## Symphony specification\n\nThis issue is not a member of the configured GitHub Project. Add it to the project and leave `{}` applied.",
            intent.intent_id, service.spec.intake_label
        );
        self.upsert_owned_comment(
            issue.issue_number,
            &intent,
            &body,
            service.spec.max_intake_pages,
        )
        .await?;
        self.store
            .complete_spec_publication(&intent.intent_id, "diagnostic_comment")?;
        self.record_event(
            Some(&run.run_id),
            None,
            "spec_ineligible",
            serde_json::json!({"status":"ineligible","error_code":"off_project"}),
        )
    }

    async fn upsert_owned_comment(
        &self,
        issue_number: u64,
        intent: &SpecPublicationIntent,
        body: &str,
        max_pages: u32,
    ) -> Result<()> {
        let login = self.comments.authenticated_login().await?;
        if let Some(id) = intent
            .comment_id
            .as_deref()
            .and_then(|id| id.parse::<u64>().ok())
        {
            let existing = self.comments.get_comment(id).await?;
            if comment_author(&existing).eq_ignore_ascii_case(&login) {
                self.comments.update_comment(id, body).await?;
                return Ok(());
            }
            return Err(SymphonyError::TriageError(
                "stored spec comment is no longer owned by the publisher".to_string(),
            ));
        }
        let comments = self.comments.list_comments(issue_number, max_pages).await?;
        let exact_marker = format!("{SPEC_COMMENT_MARKER_PREFIX}{} -->", intent.intent_id);
        let owned = comments
            .iter()
            .find(|comment| {
                comment_author(comment).eq_ignore_ascii_case(&login)
                    && comment
                        .body
                        .as_deref()
                        .unwrap_or_default()
                        .contains(&exact_marker)
            })
            .or_else(|| {
                comments.iter().find(|comment| {
                    comment_author(comment).eq_ignore_ascii_case(&login)
                        && comment
                            .body
                            .as_deref()
                            .unwrap_or_default()
                            .contains(SPEC_COMMENT_MARKER_PREFIX)
                })
            });
        let record = if let Some(existing) = owned {
            self.comments.update_comment(existing.id, body).await?
        } else {
            self.comments.create_comment(issue_number, body).await?
        };
        if !comment_author(&record).eq_ignore_ascii_case(&login) {
            return Err(SymphonyError::TriageError(
                "spec comment author did not match authenticated publisher".to_string(),
            ));
        }
        self.store
            .set_spec_publication_comment(&intent.intent_id, &record.id.to_string(), &login)
    }

    fn record_event(
        &self,
        run_id: Option<&str>,
        stage_run_id: Option<&str>,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        let mut store = self.store.clone();
        store.record_event(FactoryEventRecord {
            event_id: Uuid::now_v7().to_string(),
            run_id: run_id.map(str::to_string),
            stage_run_id: stage_run_id.map(str::to_string),
            event_type: event_type.to_string(),
            timestamp: Utc::now(),
            payload: payload.clone(),
        })?;
        if let Some(events) = &self.events {
            events.emit_triage_event(event_type, None, run_id, stage_run_id, payload);
        }
        Ok(())
    }
}

struct DurableSpecExecutor {
    runner: IsolatedSpecRunnerConfig,
    store: SharedFactoryStore,
    stage_run_id: String,
    harness: String,
}

#[async_trait]
impl SpecTurnExecutor for DurableSpecExecutor {
    async fn execute(&self, request: SpecTurnRequest) -> Result<SpecTurnOutcome> {
        let started_at = Utc::now();
        let outcome = run_isolated_spec_turn(&self.runner, request.clone()).await;
        match &outcome {
            Ok(completed) => {
                let output_json = match &completed.output {
                    SpecTurnOutput::Spec(spec) => serde_json::to_value(spec),
                    SpecTurnOutput::Findings(findings) => serde_json::to_value(findings),
                }
                .map_err(|error| SymphonyError::StorageError(error.to_string()))?;
                self.store.store_spec_turn(StoreSpecTurnRequest {
                    turn_id: request.turn_id,
                    stage_run_id: self.stage_run_id.clone(),
                    ordinal: request.ordinal,
                    kind: request.kind,
                    status: SpecTurnStatus::Completed,
                    harness: self.harness.clone(),
                    model: request.model,
                    usage: completed.usage.clone(),
                    output_json: Some(output_json),
                    error: None,
                    started_at: completed.started_at,
                    completed_at: Some(completed.completed_at),
                })?;
            }
            Err(error) => {
                let factory_error = FactoryError::new(
                    "spec_turn_failed",
                    "spec_runner",
                    error.to_string(),
                    true,
                    None,
                );
                // Preserve the failed turn for restart diagnosis. If recording
                // itself fails, return that durability failure instead.
                self.store.store_spec_turn(StoreSpecTurnRequest {
                    turn_id: request.turn_id,
                    stage_run_id: self.stage_run_id.clone(),
                    ordinal: request.ordinal,
                    kind: request.kind,
                    status: SpecTurnStatus::Failed,
                    harness: self.harness.clone(),
                    model: request.model,
                    usage: Default::default(),
                    output_json: None,
                    error: Some(factory_error),
                    started_at,
                    completed_at: Some(Utc::now()),
                })?;
            }
        }
        // The validated turn output is durable now; remove the disposable
        // clone/input/home tree and clear its restart-recovery identity.
        if let Ok(completed) = &outcome {
            let workspace = PathBuf::from(&completed.workspace_identity);
            if let Some(attempt_root) = workspace.parent() {
                let _ = std::fs::remove_dir_all(attempt_root);
            }
        }
        let mut store = self.store.clone();
        store.clear_attempt_process(&self.stage_run_id)?;
        outcome
    }
}

#[derive(Clone)]
struct LoadedPrompts {
    draft: String,
    review: String,
    revise: String,
}

fn load_prompts(workflow_dir: &Path, service: &ServiceConfig) -> Result<LoadedPrompts> {
    let load = |relative: &str| {
        let path = workflow_dir.join(relative);
        std::fs::read_to_string(&path).map_err(|error| {
            SymphonyError::InvalidWorkflowConfig(format!(
                "could not read spec prompt {}: {error}",
                path.display()
            ))
        })
    };
    Ok(LoadedPrompts {
        draft: load(&service.spec.prompts.draft)?,
        review: load(&service.spec.prompts.review)?,
        revise: load(&service.spec.prompts.revise)?,
    })
}

fn spec_configuration_revision(service: &ServiceConfig, prompts: &LoadedPrompts) -> String {
    hash_json(&serde_json::json!({
        "schema_version": 1,
        "prompts": {"draft":prompts.draft,"review":prompts.review,"revise":prompts.revise},
        "model": service.spec.model,
        "review_model": service.spec.review_model,
        "turn_timeout_ms": service.spec.turn_timeout_ms,
        "max_review_cycles": service.spec.max_review_cycles,
        "max_attempts": service.spec.max_attempts,
        "max_revision_requests": service.spec.max_revision_requests,
        "labels": service.spec.labels,
        "approval_route": service.spec.approval_route,
    }))
}

fn spec_issue_revision(
    issue: &TriageIntakeIssue,
    service: &ServiceConfig,
    publisher_login: &str,
) -> String {
    let managed = service
        .spec
        .managed_labels()
        .into_iter()
        .chain(service.triage.routes.managed_labels())
        .chain(std::iter::once(service.triage.intake_label.clone()))
        .map(|label| label.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let labels: Vec<&str> = issue
        .labels
        .iter()
        .filter(|label| !managed.contains(&label.to_ascii_lowercase()))
        .map(String::as_str)
        .collect();
    let comments: Vec<_> = issue
        .comments
        .iter()
        .filter(|comment| {
            !(comment.author_login.eq_ignore_ascii_case(publisher_login)
                && comment.body.contains("<!-- symphony:"))
        })
        .map(|comment| serde_json::json!({
            "id":comment.id,"body":comment.body,"created_at":comment.created_at,"updated_at":comment.updated_at
        }))
        .collect();
    // Issue-level `updated_at` is deliberately excluded, matching A1's canonical
    // fingerprint: GitHub bumps it when Symphony publishes its own spec comment,
    // so including it would make every publication look like new human content and
    // trigger an endless attempt/version loop.
    hash_json(&serde_json::json!({
        "title":issue.title,"body":issue.body,"labels":labels,"assignees":issue.assignees,
        "milestone":issue.milestone.as_ref().map(|m| (&m.number,&m.title)),
        "comments":comments,
    }))
}

fn issue_context(issue: &TriageIntakeIssue) -> serde_json::Value {
    serde_json::json!({
        "id":issue.issue_id,"identifier":issue.identifier,"title":issue.title,"body":issue.body,
        "labels":issue.non_managed_labels,"assignees":issue.assignees,"milestone":issue.milestone.as_ref().map(|m| &m.title),
        "comments":issue.comments.iter().map(|c| serde_json::json!({"author":c.author_login,"body":c.body,"created_at":c.created_at,"updated_at":c.updated_at})).collect::<Vec<_>>(),
        "repository":issue.repository,"forge_host":issue.forge_host,
    })
}

fn hash_json(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).expect("JSON value serializes");
    hex::encode(Sha256::digest(bytes))
}

fn has_label(labels: &[String], wanted: &str) -> bool {
    labels
        .iter()
        .any(|label| label.eq_ignore_ascii_case(wanted))
}

fn harness_name(harness: TriageHarness) -> &'static str {
    match harness {
        TriageHarness::Pi => "pi",
        TriageHarness::Codex => "codex",
    }
}

fn agent_command(service: &ServiceConfig) -> Result<Vec<String>> {
    let command = match service.agent_backend {
        AgentBackend::Codex => &service.codex.command,
        AgentBackend::KataCli => &service.pi_agent.command,
    };
    if command.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "agent command is required for the spec stage".to_string(),
        ));
    }
    Ok(command.clone())
}

/// Resolve a configured workspace path the same way the triage stage does:
/// relative values expand against the process working directory.
fn resolve_path(value: &str, field: &str) -> Result<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(format!(
            "{field} cannot be empty for the spec stage"
        )));
    }
    crate::path_safety::canonicalize(Path::new(trimmed)).map_err(|error| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "failed to resolve {field} '{trimmed}' for the spec stage: {error}"
        ))
    })
}

fn comment_author(comment: &crate::github::client::GithubIssueComment) -> &str {
    comment
        .user
        .as_ref()
        .map(|user| user.login.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ServiceConfig;
    use crate::github::client::{GithubIssueComment, GithubUser};
    use crate::spec::domain::{SpecArtifact, SpecTurnKind};
    use crate::triage::domain::StageUsage;
    use crate::triage::intake::{IntakeComment, TriageIntakeIssue};
    use crate::triage::runtime::SharedFactoryStore;
    use crate::triage::store::{
        ClaimAttemptRequest, StoreSpecArtifactRequest, StoreSpecTurnRequest,
    };
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{Arc, Mutex};

    fn issue() -> TriageIntakeIssue {
        TriageIntakeIssue {
            issue_id: "1".to_string(),
            issue_number: 1,
            identifier: "#1".to_string(),
            title: "Add a section".to_string(),
            body: "Body".to_string(),
            labels: vec!["ready-to-spec".to_string()],
            non_managed_labels: Vec::new(),
            assignees: Vec::new(),
            milestone: None,
            comments: Vec::new(),
            created_at: Utc.with_ymd_and_hms(2026, 7, 26, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 7, 26, 0, 0, 0).unwrap(),
            forge_host: "github.com".to_string(),
            repository: "owner/repo".to_string(),
            in_project: true,
        }
    }

    /// Publishing the spec comment bumps the GitHub issue's `updated_at` and adds a
    /// publisher-owned comment. Neither is human content, so the revision must be
    /// unchanged. If it changed, the next poll would treat its own publication as
    /// new input and publish an unbounded series of versions with no human decision.
    #[test]
    fn publishing_the_spec_comment_does_not_change_the_issue_revision() {
        let service = ServiceConfig::default();
        let before = spec_issue_revision(&issue(), &service, "symphony-bot");

        let mut after_publication = issue();
        after_publication.updated_at = Utc.with_ymd_and_hms(2026, 7, 26, 1, 0, 0).unwrap();
        after_publication.comments.push(IntakeComment {
            id: 10,
            author_login: "symphony-bot".to_string(),
            body: "<!-- symphony:spec:abc -->\n## Symphony specification".to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 7, 26, 1, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 7, 26, 1, 0, 0).unwrap(),
        });

        assert_eq!(
            before,
            spec_issue_revision(&after_publication, &service, "symphony-bot"),
            "Symphony's own publication must not create a new revision pair"
        );
    }

    /// Human feedback is what a `spec-revise` request keys on, so it must still
    /// produce a new revision pair.
    #[test]
    fn human_feedback_comment_changes_the_issue_revision() {
        let service = ServiceConfig::default();
        let before = spec_issue_revision(&issue(), &service, "symphony-bot");

        let mut with_feedback = issue();
        with_feedback.comments.push(IntakeComment {
            id: 11,
            author_login: "maintainer".to_string(),
            body: "Please cover the error path too.".to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 7, 26, 2, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 7, 26, 2, 0, 0).unwrap(),
        });

        assert_ne!(
            before,
            spec_issue_revision(&with_feedback, &service, "symphony-bot")
        );
    }

    /// `workspace.root` and `workspace.repo` are documented as relative to the
    /// process working directory and every shipped starter workflow uses relative
    /// values. Rejecting them would make the spec stage unusable on a default
    /// `symphony init` workspace, so spec must resolve them like triage does.
    #[test]
    fn relative_workspace_paths_resolve_instead_of_failing() {
        let resolved = resolve_path(".symphony/workspaces", "workspace.root")
            .expect("relative workspace roots are the documented default");

        assert!(resolved.is_absolute());
        assert!(resolved.ends_with(std::path::Path::new(".symphony/workspaces")));
    }

    #[test]
    fn empty_workspace_path_is_rejected_with_the_field_name() {
        let error = resolve_path("   ", "workspace.repo").expect_err("empty paths are invalid");

        assert!(error.to_string().contains("workspace.repo"));
    }

    #[test]
    fn pure_helpers_cover_label_matching_and_agent_command() {
        assert!(has_label(&["Ready-To-Spec".to_string()], "ready-to-spec"));
        assert!(!has_label(&["other".to_string()], "ready-to-spec"));
        assert_eq!(harness_name(TriageHarness::Pi), "pi");
        assert_eq!(harness_name(TriageHarness::Codex), "codex");

        let mut service = ServiceConfig::default();
        service.pi_agent.command = vec!["pi".into(), "--mode".into(), "rpc".into()];
        assert_eq!(agent_command(&service).unwrap()[0], "pi");
        service.pi_agent.command.clear();
        assert!(agent_command(&service).is_err());

        let comment = GithubIssueComment {
            id: 1,
            user: Some(GithubUser {
                login: "bot".into(),
            }),
            body: None,
            html_url: None,
            created_at: None,
            updated_at: None,
        };
        assert_eq!(comment_author(&comment), "bot");
        assert_eq!(
            comment_author(&GithubIssueComment {
                user: None,
                ..comment
            }),
            ""
        );
        let _ = issue_context(&issue());
        let _ = hash_json(&serde_json::json!({"a": 1}));
    }

    struct FakeIntake {
        issues: Mutex<Vec<TriageIntakeIssue>>,
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
            _issue_number: u64,
            _max_pages: u32,
        ) -> Result<Vec<IntakeComment>> {
            Ok(Vec::new())
        }

        async fn authenticated_login(&self) -> Result<String> {
            Ok("symphony-bot".into())
        }
    }

    #[derive(Default)]
    struct FakeComments {
        login: String,
        comments: Mutex<HashMap<u64, GithubIssueComment>>,
        next_id: Mutex<u64>,
    }

    impl FakeComments {
        fn new(login: &str) -> Arc<Self> {
            Arc::new(Self {
                login: login.into(),
                comments: Mutex::new(HashMap::new()),
                next_id: Mutex::new(900),
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
                    message: "missing".into(),
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
                        message: "missing".into(),
                    })?;
            comment.body = Some(body.to_string());
            Ok(comment.clone())
        }
    }

    struct FakeRouting {
        labels: Mutex<Vec<String>>,
        project_state: Mutex<Option<String>>,
    }

    impl FakeRouting {
        fn new(labels: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                labels: Mutex::new(labels.iter().map(|label| (*label).to_string()).collect()),
                project_state: Mutex::new(None),
            })
        }
    }

    #[async_trait]
    impl TriageRoutingPort for Arc<FakeRouting> {
        async fn list_issue_labels(&self, _issue_number: u64) -> Result<Vec<String>> {
            Ok(self.labels.lock().unwrap().clone())
        }

        async fn issue_project_state(&self, _issue_number: u64) -> Result<Option<String>> {
            Ok(self.project_state.lock().unwrap().clone())
        }

        async fn add_issue_label(&self, _issue_number: u64, label: &str) -> Result<()> {
            let mut labels = self.labels.lock().unwrap();
            if !labels
                .iter()
                .any(|present| present.eq_ignore_ascii_case(label))
            {
                labels.push(label.to_string());
            }
            Ok(())
        }

        async fn remove_issue_label(&self, _issue_number: u64, label: &str) -> Result<()> {
            self.labels
                .lock()
                .unwrap()
                .retain(|present| !present.eq_ignore_ascii_case(label));
            Ok(())
        }

        async fn set_issue_project_state(&self, _issue_number: u64, state: &str) -> Result<()> {
            *self.project_state.lock().unwrap() = Some(state.to_string());
            Ok(())
        }
    }

    fn write_prompts(dir: &Path) {
        for name in ["draft.md", "review.md", "revise.md"] {
            fs::write(dir.join(name), format!("# {name}\n")).unwrap();
        }
    }

    fn enabled_service(workflow_dir: &Path) -> ServiceConfig {
        let mut service = ServiceConfig::default();
        service.spec.enabled = true;
        service.triage.enabled = true;
        service.triage.intake_label = "needs-triage".into();
        service.spec.prompts.draft = "draft.md".into();
        service.spec.prompts.review = "review.md".into();
        service.spec.prompts.revise = "revise.md".into();
        service.workspace.root = workflow_dir.join("workspaces").display().to_string();
        service.workspace.repo = Some(workflow_dir.join("repo").display().to_string());
        service.pi_agent.command = vec!["pi".into()];
        fs::create_dir_all(workflow_dir.join("workspaces")).unwrap();
        fs::create_dir_all(workflow_dir.join("repo")).unwrap();
        write_prompts(workflow_dir);
        service
    }

    fn open_store(path: &Path) -> SharedFactoryStore {
        SharedFactoryStore::open(path, 5_000).unwrap()
    }

    fn seed_published_spec(
        store: &SharedFactoryStore,
        service: &ServiceConfig,
        issue: &TriageIntakeIssue,
    ) -> SpecArtifactRecord {
        let workflow_dir = Path::new(&service.workspace.root).parent().unwrap();
        let prompts = load_prompts(workflow_dir, service).unwrap();
        let configuration_revision = spec_configuration_revision(service, &prompts);
        let issue_revision = spec_issue_revision(issue, service, "symphony-bot");
        let stage = store
            .claim_spec_attempt(ClaimAttemptRequest {
                forge_host: "github.com".into(),
                repository: "owner/repo".into(),
                issue_id: issue.issue_id.clone(),
                issue_identifier: issue.identifier.clone(),
                issue_revision: issue_revision.clone(),
                configuration_revision: configuration_revision.clone(),
                owner_instance: "test".into(),
                harness: "pi".into(),
                model: None,
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })
            .unwrap();
        store
            .store_spec_turn(StoreSpecTurnRequest {
                turn_id: format!("seed-turn-{}", issue.issue_id),
                stage_run_id: stage.stage_run_id.clone(),
                ordinal: 1,
                kind: SpecTurnKind::Draft,
                status: SpecTurnStatus::Completed,
                harness: "pi".into(),
                model: None,
                usage: StageUsage::default(),
                output_json: None,
                error: None,
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
            })
            .unwrap();
        let artifact = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: stage.stage_run_id,
                issue_revision,
                configuration_revision,
                artifact: SpecArtifact {
                    schema_version: 1,
                    product_behavior: "behavior".into(),
                    technical_approach: "approach".into(),
                    acceptance_criteria: vec!["done".into()],
                    open_decisions: vec![],
                },
                review_cycles: 1,
                unresolved_blocking_findings: vec![],
                bytes_len: 64,
                usage: StageUsage::default(),
            })
            .unwrap();
        let intent = store
            .create_spec_publication_intent(
                &stage.run_id,
                Some(&artifact.artifact_id),
                SpecPublicationKind::Preview,
                &serde_json::json!({"issue_number": issue.issue_number}),
            )
            .unwrap();
        // Leave comment_id unset so the first poll creates the owned comment through
        // the fake comment port. Completing the intent marks the version published.
        store
            .complete_spec_publication(&intent.intent_id, "spec_comment")
            .unwrap();
        let _ = intent;
        artifact
    }

    #[tokio::test]
    async fn poll_handles_disabled_dual_label_and_off_project_paths() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = temp.path().to_path_buf();
        let service = enabled_service(&workflow_dir);
        let store = open_store(&temp.path().join("state.db"));

        let mut dual = issue();
        dual.issue_id = "2".into();
        dual.issue_number = 2;
        dual.identifier = "#2".into();
        dual.labels = vec!["needs-triage".into(), "ready-to-spec".into()];

        let mut off = issue();
        off.issue_id = "3".into();
        off.issue_number = 3;
        off.identifier = "#3".into();
        off.in_project = false;

        let intake = Arc::new(FakeIntake {
            issues: Mutex::new(vec![dual, off]),
        });
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(&["ready-to-spec"]);
        let mut coordinator = SpecCoordinator::new(
            store.clone(),
            intake,
            comments.clone(),
            routing,
            SpecCoordinatorConfig {
                forge_host: "github.com".into(),
                repository: "owner/repo".into(),
                owner_instance: "test".into(),
                workflow_dir: workflow_dir.clone(),
            },
        );

        let mut disabled = service.clone();
        disabled.spec.enabled = false;
        let summary = coordinator.poll_once(&disabled).await.unwrap();
        assert!(!summary.spec_enabled);

        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.issues_seen, 2);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.ineligible, 1);
        assert_eq!(comments.comments.lock().unwrap().len(), 1);

        // Second poll must not grow dual-label events or off-project intents.
        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.ineligible, 1);
        assert_eq!(comments.comments.lock().unwrap().len(), 1);
        assert_eq!(
            store.list_pending_spec_publications().unwrap().len(),
            0,
            "diagnostic intent is applied, not pending"
        );
    }

    #[tokio::test]
    async fn poll_approves_current_version_and_diagnoses_missing_feedback() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_dir = temp.path().to_path_buf();
        let service = enabled_service(&workflow_dir);
        let store = open_store(&temp.path().join("state.db"));

        // Missing-feedback path.
        let mut revise_issue = issue();
        revise_issue.issue_id = "10".into();
        revise_issue.issue_number = 10;
        revise_issue.identifier = "#10".into();
        revise_issue.labels = vec!["ready-to-spec".into(), "spec-revise".into()];
        seed_published_spec(&store, &service, &revise_issue);

        // Approval path.
        let mut approve_issue = issue();
        approve_issue.issue_id = "11".into();
        approve_issue.issue_number = 11;
        approve_issue.identifier = "#11".into();
        approve_issue.labels = vec!["ready-to-spec".into(), "spec-approved".into()];
        seed_published_spec(&store, &service, &approve_issue);

        let intake = Arc::new(FakeIntake {
            issues: Mutex::new(vec![revise_issue.clone(), approve_issue.clone()]),
        });
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(&["ready-to-spec", "spec-approved", "spec-revise"]);
        let mut coordinator = SpecCoordinator::new(
            store.clone(),
            intake.clone(),
            comments.clone(),
            routing.clone(),
            SpecCoordinatorConfig {
                forge_host: "github.com".into(),
                repository: "owner/repo".into(),
                owner_instance: "test".into(),
                workflow_dir,
            },
        );

        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(
            summary.skipped, 1,
            "missing feedback is diagnosed and skipped"
        );
        assert_eq!(summary.approved, 1);

        use crate::triage::store::FactoryRunStore;
        let approve_run = store
            .get_run_by_issue("github.com", "owner/repo", "11")
            .unwrap()
            .unwrap();
        let state = store.get_spec_state(&approve_run.run_id).unwrap().unwrap();
        assert_eq!(state.approved_version, Some(1));
        assert!(routing
            .labels
            .lock()
            .unwrap()
            .iter()
            .any(|label| label == "ready-for-agent"));
        assert_eq!(
            routing.project_state.lock().unwrap().as_deref(),
            Some("Todo")
        );

        // Conflict path on a third issue with a published version.
        let mut conflict = issue();
        conflict.issue_id = "12".into();
        conflict.issue_number = 12;
        conflict.identifier = "#12".into();
        conflict.labels = vec![
            "ready-to-spec".into(),
            "spec-approved".into(),
            "spec-revise".into(),
        ];
        seed_published_spec(&store, &service, &conflict);
        *intake.issues.lock().unwrap() = vec![conflict];
        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.skipped, 1);
    }
}
