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
use crate::triage::domain::{FactoryError, FactoryEventRecord, FactoryRunStatus};
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
                self.record_event(
                    None,
                    None,
                    "spec_ineligible",
                    serde_json::json!({
                        "issue": issue.identifier,
                        "status": "ineligible",
                        "error_code": "intake_label_conflict"
                    }),
                )?;
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
                    let publication = self.store.get_latest_spec_publication(&run.run_id)?;
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
                            )
                            .await?;
                            summary.skipped += 1;
                            continue;
                        }
                        DecisionAction::Revise { feedback } => {
                            self.store.increment_spec_revision_requests(
                                &run.run_id,
                                service.spec.max_revision_requests,
                            )?;
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
                    let body = render_spec_comment(
                        &intent.intent_id,
                        &intent.run_id,
                        &artifact.stage_run_id,
                        stage.attempt,
                        artifact.version,
                        &artifact.artifact,
                        &artifact.unresolved_blocking_findings,
                        SpecCommentState::AwaitingDecision,
                    );
                    self.upsert_owned_comment(
                        issue_number,
                        &intent,
                        &body,
                        service.spec.max_intake_pages,
                    )
                    .await?;
                    if let Some(revise_label) = intent
                        .desired_effects
                        .get("revise_label")
                        .and_then(serde_json::Value::as_str)
                    {
                        self.routing
                            .remove_issue_label(issue_number, revise_label)
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
                            &intent,
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
                            &intent,
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
                        self.routing.remove_issue_label(issue_number, label).await?;
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
                        &intent,
                        &body,
                        service.spec.max_intake_pages,
                    )
                    .await?;
                    self.store.finalize_spec_approval(
                        &intent.run_id,
                        &intent.intent_id,
                        "comment_final",
                    )?;
                }
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
        let store = self.store.clone();
        if let Some(run) = store.get_run_by_issue(
            &self.config.forge_host,
            &self.config.repository,
            &issue.issue_id,
        )? {
            let attempts = store.list_stage_attempts_for_revision(
                &run.run_id,
                issue_revision,
                configuration_revision,
            )?;
            let spec_attempts = attempts
                .iter()
                .filter(|attempt| attempt.stage == SPEC_STAGE_NAME)
                .count() as u32;
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

        let runner = IsolatedSpecRunnerConfig {
            workspace_root: resolve_path(&service.workspace.root, "workspace.root")?,
            repo_path: resolve_path(
                service.workspace.repo.as_deref().ok_or_else(|| {
                    SymphonyError::InvalidWorkflowConfig(
                        "workspace.repo is required for the spec stage".to_string(),
                    )
                })?,
                "workspace.repo",
            )?,
            command: agent_command(service)?,
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
        .await;
        let pipeline = match pipeline {
            Ok(outcome) => outcome,
            Err(error) => {
                let factory_error = FactoryError::new(
                    "spec_turn_failed",
                    "spec_runner",
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
                    serde_json::json!({"status":"failed","error_code":"spec_turn_failed"}),
                )?;
                return Err(error);
            }
        };
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
        self.publish_artifact(issue, &stage, &artifact, &service.spec.labels.revise)
            .await
    }

    async fn publish_artifact(
        &self,
        issue: &TriageIntakeIssue,
        stage: &crate::triage::domain::StageRunRecord,
        artifact: &SpecArtifactRecord,
        revise_label: &str,
    ) -> Result<()> {
        let intent = self.store.create_spec_publication_intent(
            &stage.run_id,
            Some(&artifact.artifact_id),
            SpecPublicationKind::Preview,
            &serde_json::json!({
                "issue_number":issue.issue_number,
                "version":artifact.version,
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
            SpecCommentState::AwaitingDecision,
        );
        self.upsert_owned_comment(issue.issue_number, &intent, &body, 100)
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
        // Ordered, idempotent tracker effects. Removing an absent label and
        // adding an existing label are safe through the GitHub adapter.
        for label in [
            service.spec.intake_label.as_str(),
            service.spec.labels.approved.as_str(),
            service.spec.labels.revise.as_str(),
        ] {
            self.routing
                .remove_issue_label(issue.issue_number, label)
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

    async fn publish_diagnostic(
        &self,
        issue: &TriageIntakeIssue,
        run_id: &str,
        artifact: &SpecArtifactRecord,
        message: &str,
    ) -> Result<()> {
        let intent = self.store.create_spec_publication_intent(
            run_id,
            Some(&artifact.artifact_id),
            SpecPublicationKind::Diagnostic,
            &serde_json::json!({"issue_number":issue.issue_number,"message":message}),
        )?;
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
        self.upsert_owned_comment(issue.issue_number, &intent, &body, 100)
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
        let intent = self.store.create_spec_publication_intent(
            &run.run_id,
            None,
            SpecPublicationKind::Diagnostic,
            &serde_json::json!({
                "issue_number":issue.issue_number,
                "kind":"off_project",
                "intake_label":service.spec.intake_label,
            }),
        )?;
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
            events.emit_triage_event(event_type, None, payload);
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
    hash_json(&serde_json::json!({
        "title":issue.title,"body":issue.body,"labels":labels,"assignees":issue.assignees,
        "milestone":issue.milestone.as_ref().map(|m| (&m.number,&m.title)),
        "comments":comments,"updated_at":issue.updated_at,
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

fn resolve_path(value: &str, field: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(SymphonyError::InvalidWorkflowConfig(format!(
            "{field} must be an absolute local path for the spec stage"
        )));
    }
    Ok(path)
}

fn comment_author(comment: &crate::github::client::GithubIssueComment) -> &str {
    comment
        .user
        .as_ref()
        .map(|user| user.login.as_str())
        .unwrap_or_default()
}
