//! A3 eligibility, attempts, repair loop, and reconciliation.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AgentBackend, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::github::auth::resolve_github_token;
use crate::github::client::GithubClient;
use crate::implementation::artifact::{
    expand_spec_file_path, is_spec_gap, parse_and_validate_manifest, render_approved_spec,
};
use crate::implementation::automatic::{
    resolve_publication_remote, AutomaticImplementationPublisher, AutomaticPublishRequest,
};
use crate::implementation::branch::branch_name_for_issue;
use crate::implementation::bundle::{
    artifacts_dir, cleanup_incomplete_temps, create_result_bundle, store_blob_atomic, BlobSource,
};
use crate::implementation::domain::{
    BlockerKind, ExecutionProfile, ImplementationAttemptInputs, ImplementationBlocker,
    ImplementationDecision, ImplementationMode, ImplementationPublicationKind,
    ImplementationTurnKind, ImplementationTurnStatus, ManifestStatus, ValidationCommandResult,
    IMPLEMENTATION_STAGE_NAME,
};
use crate::implementation::publisher::{
    DiagnosticPublishRequest, ImplementationPublisher, PreviewPublishRequest, COMMENT_FINAL_STEP,
};
use crate::implementation::runner::{
    assess_postconditions, blob_sha_of_file, resolve_base_commit, ImplementationHarness,
    ImplementationRunner, ImplementationTurnRequest, LiveImplementationHarness,
};
use crate::implementation::runtime::implementation_configuration_revision;
use crate::implementation::validation::ValidationExecutor;
use crate::spec::domain::SpecArtifactRecord;
use crate::triage::coordinator::EventEmitter;
use crate::triage::domain::{FactoryError, FactoryEventRecord, PublicationStatus, StageUsage};
use crate::triage::intake::{IntakeComment, IntakeMilestone, TriageIntakeIssue};
use crate::triage::publisher::{TriageCommentPort, TriageRoutingPort};
use crate::triage::runner::{effective_pi_model, TriageHarness, TriageIssueIdentity};
use crate::triage::runtime::SharedFactoryStore;
use crate::triage::store::{
    A3EligibleApprovedRun, ClaimAttemptRequest, FactoryRunStore, RecordAttemptProcessRequest,
    StoreBundleArtifactRequest, StoreImplementationArtifactRequest, StoreImplementationTurnRequest,
    StoreValidationCycleRequest, UpsertImplementationStateRequest,
};

/// Reconcile attempts an automatic publication intent may consume before it is
/// terminalized as `Blocked`. Every poll retries each pending intent, so an
/// unreachable or misconfigured publication target would otherwise issue forge
/// calls and append event rows forever without ever surfacing to an operator.
const MAX_AUTOMATIC_PUBLICATION_RETRIES: u32 = 8;

/// First backoff interval between automatic publication reconcile attempts.
const AUTOMATIC_PUBLICATION_BACKOFF_BASE_SECS: i64 = 30;

/// Ceiling for a single backoff interval. With the base and ceiling above, the
/// retry budget spans roughly an hour, so transient forge outages recover
/// without exhausting it.
const AUTOMATIC_PUBLICATION_BACKOFF_MAX_SECS: i64 = 1_800;

#[derive(Debug, Clone)]
pub struct ImplementationCoordinatorConfig {
    pub forge_host: String,
    pub repository: String,
    pub owner_instance: String,
    pub workflow_dir: PathBuf,
    pub storage_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImplementationPollSummary {
    pub implementation_enabled: bool,
    pub candidates_seen: u32,
    pub attempts_started: u32,
    pub attempts_completed: u32,
    pub attempts_failed: u32,
    pub stale_skipped: u32,
    pub preview_published: u32,
    pub automatic_published: u32,
    pub automatic_pending: u32,
    pub awaiting_human: u32,
}

/// Fetch a single issue for eligibility revision checks.
#[async_trait]
pub trait ImplementationIssuePort: Send + Sync {
    async fn fetch_issue(&self, issue_id: &str) -> Result<TriageIntakeIssue>;
}

#[async_trait]
impl ImplementationIssuePort for GithubClient {
    async fn fetch_issue(&self, issue_id: &str) -> Result<TriageIntakeIssue> {
        let number: u64 = issue_id.parse().map_err(|_| {
            SymphonyError::TriageError(format!("invalid GitHub issue id {issue_id}"))
        })?;
        let issue = self.get_issue(number).await?;
        let comments = self.list_comments_paginated(number, 10).await?;
        let labels: Vec<String> = issue
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect();
        let assignees: Vec<String> = if !issue.assignees.is_empty() {
            issue
                .assignees
                .iter()
                .map(|user| user.login.clone())
                .collect()
        } else {
            issue.assignee.into_iter().map(|user| user.login).collect()
        };
        Ok(TriageIntakeIssue {
            issue_id: issue.number.to_string(),
            issue_number: issue.number,
            identifier: format!("#{}", issue.number),
            title: issue.title,
            body: issue.body.unwrap_or_default(),
            labels: labels.clone(),
            non_managed_labels: labels,
            assignees,
            milestone: issue.milestone.map(|m| IntakeMilestone {
                number: m.number,
                title: m.title,
            }),
            comments: comments
                .into_iter()
                .map(|c| IntakeComment {
                    id: c.id,
                    author_login: c.user.as_ref().map(|u| u.login.clone()).unwrap_or_default(),
                    body: c.body.unwrap_or_default(),
                    created_at: c.created_at.unwrap_or_else(Utc::now),
                    updated_at: c.updated_at.unwrap_or_else(Utc::now),
                })
                .collect(),
            created_at: issue.created_at.unwrap_or_else(Utc::now),
            updated_at: issue.updated_at.unwrap_or_else(Utc::now),
            forge_host: "github.com".into(),
            repository: format!("{}/{}", self.repo_owner, self.repo_name),
            in_project: true,
        })
    }
}

pub struct ImplementationCoordinator<C, I, H> {
    store: SharedFactoryStore,
    comments: C,
    issues: I,
    harness: H,
    config: ImplementationCoordinatorConfig,
    events: Option<Arc<dyn EventEmitter>>,
    routing: Option<Arc<dyn TriageRoutingPort>>,
    pulls: Option<Arc<dyn crate::implementation::automatic::ImplementationPullRequestPort>>,
}

impl<C, I> ImplementationCoordinator<C, I, LiveImplementationHarness>
where
    C: TriageCommentPort + Clone,
    I: ImplementationIssuePort,
{
    pub fn new(
        store: SharedFactoryStore,
        comments: C,
        issues: I,
        config: ImplementationCoordinatorConfig,
    ) -> Self {
        Self {
            store,
            comments,
            issues,
            harness: LiveImplementationHarness,
            config,
            events: None,
            routing: None,
            pulls: None,
        }
    }
}

impl<C, I, H> ImplementationCoordinator<C, I, H>
where
    C: TriageCommentPort + Clone,
    I: ImplementationIssuePort,
    H: ImplementationHarness,
{
    pub fn with_harness(
        store: SharedFactoryStore,
        comments: C,
        issues: I,
        config: ImplementationCoordinatorConfig,
        harness: H,
    ) -> Self {
        Self {
            store,
            comments,
            issues,
            harness,
            config,
            events: None,
            routing: None,
            pulls: None,
        }
    }

    pub fn with_events(mut self, events: Arc<dyn EventEmitter>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn with_routing(mut self, routing: impl TriageRoutingPort + 'static) -> Self {
        self.routing = Some(Arc::new(routing));
        self
    }

    pub fn with_pulls(
        mut self,
        pulls: impl crate::implementation::automatic::ImplementationPullRequestPort + 'static,
    ) -> Self {
        self.pulls = Some(Arc::new(pulls));
        self
    }

    pub async fn poll_once(
        &mut self,
        service: &ServiceConfig,
    ) -> Result<ImplementationPollSummary> {
        let mut summary = ImplementationPollSummary {
            implementation_enabled: service.implementation.enabled,
            ..Default::default()
        };

        // Recover interrupted implementation attempts (lease-stale → interrupted).
        self.store.interrupt_stale_attempts()?;
        let _ = cleanup_incomplete_temps(&artifacts_dir(&self.config.storage_path));

        self.reconcile_pending_publications(service).await?;

        if !service.implementation.enabled {
            return Ok(summary);
        }

        let prompts = load_prompts(&self.config.workflow_dir, service)?;
        let configuration_revision = implementation_configuration_revision(
            &service.implementation,
            &prompts.implement,
            &prompts.repair,
        );
        let publisher_login = self.comments.authenticated_login().await?;

        let candidates = self
            .store
            .list_a3_eligible_approved_runs(&configuration_revision)?;
        summary.candidates_seen = candidates.len() as u32;

        for candidate in candidates {
            match self
                .process_candidate(
                    service,
                    &prompts,
                    &configuration_revision,
                    &publisher_login,
                    &candidate,
                )
                .await
            {
                Ok(CandidateOutcome::StartedPreview) => {
                    summary.attempts_started += 1;
                    summary.attempts_completed += 1;
                    summary.preview_published += 1;
                }
                Ok(CandidateOutcome::StartedAutomaticPublished) => {
                    summary.attempts_started += 1;
                    summary.attempts_completed += 1;
                    summary.automatic_published += 1;
                }
                Ok(CandidateOutcome::StartedAutomaticPending) => {
                    summary.attempts_started += 1;
                    summary.attempts_completed += 1;
                    summary.automatic_pending += 1;
                }
                Ok(CandidateOutcome::StartedAwaitingHuman) => {
                    summary.attempts_started += 1;
                    summary.attempts_completed += 1;
                    summary.awaiting_human += 1;
                }
                Ok(CandidateOutcome::StartedFailed) => {
                    summary.attempts_started += 1;
                    summary.attempts_failed += 1;
                }
                Ok(CandidateOutcome::Stale) => summary.stale_skipped += 1,
                Ok(CandidateOutcome::Skipped) => {}
                Err(error) => {
                    tracing::warn!(
                        event = "implementation_candidate_failed",
                        run_id = %candidate.run_id,
                        error = %error,
                        "implementation candidate processing failed"
                    );
                    summary.attempts_failed += 1;
                }
            }
        }

        Ok(summary)
    }

    async fn reconcile_pending_publications(&self, service: &ServiceConfig) -> Result<()> {
        let pending = self.store.list_pending_implementation_publications()?;
        let publisher = ImplementationPublisher::new(self.comments.clone());
        for intent in pending {
            match intent.kind {
                ImplementationPublicationKind::Automatic => {
                    if intent.retry_count >= MAX_AUTOMATIC_PUBLICATION_RETRIES {
                        let failure = FactoryError::new(
                            "publication_retry_exhausted",
                            "implementation_publication",
                            format!(
                                "automatic publication exhausted {} reconcile attempts; last error: {}",
                                MAX_AUTOMATIC_PUBLICATION_RETRIES,
                                intent
                                    .last_error
                                    .as_ref()
                                    .map(|last| last.remediation.as_str())
                                    .unwrap_or("unknown"),
                            ),
                            false,
                            intent.completed_steps.last().cloned(),
                        );
                        self.store.set_implementation_publication_error(
                            &intent.intent_id,
                            PublicationStatus::Blocked,
                            failure.clone(),
                        )?;
                        self.record_event(
                            Some(&intent.run_id),
                            None,
                            "implementation_publication_blocked",
                            serde_json::json!({
                                "intent_id": intent.intent_id,
                                "retry_count": intent.retry_count,
                                "error": failure,
                            }),
                        )?;
                        tracing::warn!(
                            event = "implementation_automatic_publication_retry_exhausted",
                            intent_id = %intent.intent_id,
                            retry_count = intent.retry_count,
                            "automatic publication blocked after exhausting retries"
                        );
                        continue;
                    }
                    if !automatic_retry_ready(intent.retry_count, intent.updated_at, Utc::now()) {
                        continue;
                    }
                    if let Err(error) = self.reconcile_automatic_publication(service, &intent).await
                    {
                        let error_message = error.to_string();
                        let latest = self
                            .store
                            .get_implementation_publication_intent(&intent.intent_id)?;
                        if let Some(latest) = latest.filter(|latest| {
                            latest.status == PublicationStatus::Pending
                                && !latest
                                    .last_error
                                    .as_ref()
                                    .is_some_and(|last| already_recorded(last, &error_message))
                        }) {
                            let failure = FactoryError::new(
                                "publication_retryable",
                                "implementation_publication",
                                error_message,
                                true,
                                latest.completed_steps.last().cloned(),
                            );
                            self.store.set_implementation_publication_error(
                                &intent.intent_id,
                                PublicationStatus::Pending,
                                failure.clone(),
                            )?;
                            self.record_event(
                                Some(&intent.run_id),
                                None,
                                "implementation_publication_blocked",
                                serde_json::json!({
                                    "intent_id": intent.intent_id,
                                    "error": failure,
                                }),
                            )?;
                        }
                        tracing::warn!(
                            event = "implementation_automatic_publication_failed",
                            intent_id = %intent.intent_id,
                            error = %error,
                            "automatic publication reconcile failed"
                        );
                    }
                }
                ImplementationPublicationKind::Preview
                | ImplementationPublicationKind::Diagnostic => {
                    // Re-publish when we have enough durable context; otherwise leave pending.
                    let Some(artifact_id) = intent.artifact_id.as_deref() else {
                        if intent.kind == ImplementationPublicationKind::Diagnostic {
                            // Diagnostic without artifact — message in desired_effects.
                            let message = intent
                                .desired_effects
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Implementation requires human attention.");
                            let issue_number = issue_number_from_run(&self.store, &intent.run_id)?;
                            let _ = publisher
                                .publish_diagnostic(
                                    &self.store,
                                    DiagnosticPublishRequest {
                                        intent: &intent,
                                        issue_number,
                                        run_id: &intent.run_id,
                                        message,
                                        max_pages: service
                                            .triage
                                            .max_intake_pages
                                            .max(service.spec.max_intake_pages),
                                    },
                                )
                                .await;
                        }
                        continue;
                    };
                    let Some(artifact) = self.store.get_implementation_artifact(artifact_id)?
                    else {
                        continue;
                    };
                    let issue_number = issue_number_from_run(&self.store, &intent.run_id)?;
                    if intent.kind == ImplementationPublicationKind::Preview {
                        let validation =
                            last_validation_results(&self.store, &artifact.stage_run_id)?;
                        let changed = intent
                            .desired_effects
                            .get("changed_paths")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(str::to_string))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let _ = publisher
                            .publish_preview(
                                &self.store,
                                PreviewPublishRequest {
                                    intent: &intent,
                                    issue_number,
                                    run_id: &artifact.run_id,
                                    stage_run_id: &artifact.stage_run_id,
                                    artifact: &artifact,
                                    manifest: &artifact.manifest,
                                    changed_paths: &changed,
                                    validation: &validation,
                                    max_pages: service
                                        .triage
                                        .max_intake_pages
                                        .max(service.spec.max_intake_pages),
                                },
                            )
                            .await;
                    }
                }
            }
        }
        Ok(())
    }

    async fn reconcile_automatic_publication(
        &self,
        service: &ServiceConfig,
        intent: &crate::implementation::domain::ImplementationPublicationIntent,
    ) -> Result<()> {
        let Some(routing) = self.routing.as_ref() else {
            return Err(SymphonyError::TriageError(
                "automatic publication requires TriageRoutingPort".into(),
            ));
        };
        let Some(artifact_id) = intent.artifact_id.as_deref() else {
            return Err(SymphonyError::TriageError(
                "automatic publication missing artifact_id".into(),
            ));
        };
        let Some(artifact) = self.store.get_implementation_artifact(artifact_id)? else {
            return Err(SymphonyError::TriageError(format!(
                "implementation artifact {artifact_id} missing"
            )));
        };
        let Some(state) = self.store.get_implementation_state(&intent.run_id)? else {
            return Err(SymphonyError::TriageError(
                "implementation run state missing".into(),
            ));
        };
        let Some(bundle_id) = state.bundle_artifact_id.as_deref() else {
            return Err(SymphonyError::TriageError(
                "automatic publication missing bundle artifact".into(),
            ));
        };
        let Some(bundle) = self.store.get_bundle_artifact(bundle_id)? else {
            return Err(SymphonyError::TriageError(format!(
                "bundle artifact {bundle_id} missing"
            )));
        };
        let issue_id = {
            let run = self.store.get_run_by_id(&intent.run_id)?.ok_or_else(|| {
                SymphonyError::StorageError(format!("run {} missing", intent.run_id))
            })?;
            run.issue_id
        };
        let issue = self.issues.fetch_issue(&issue_id).await?;
        let repo_owner = required_desired_effect(&intent.desired_effects, "repo_owner")?;
        let repo_name = required_desired_effect(&intent.desired_effects, "repo_name")?;
        let active_repo_owner = service
            .tracker
            .repo_owner
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| SymphonyError::TriageError("tracker.repo_owner required".into()))?;
        let active_repo_name = service
            .tracker
            .repo_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| SymphonyError::TriageError("tracker.repo_name required".into()))?;
        if active_repo_owner != repo_owner || active_repo_name != repo_name {
            return Err(SymphonyError::TriageError(format!(
                "automatic publication repository drift: pinned={repo_owner}/{repo_name} configured={active_repo_owner}/{active_repo_name}"
            )));
        }
        let publication_branch =
            required_desired_effect(&intent.desired_effects, "publication_branch")?;
        let base_branch = required_desired_effect(&intent.desired_effects, "base_branch")?;
        let approval_label =
            required_desired_effect(&intent.desired_effects, "approval_route_label")?;
        let completion_state =
            required_desired_effect(&intent.desired_effects, "completion_route_state")?;
        let remote_url = resolve_publication_remote(
            &intent.desired_effects,
            &self.config.forge_host,
            repo_owner,
            repo_name,
        )?;
        let github_token = resolve_github_token(&service.tracker)
            .ok_or(SymphonyError::MissingGithubApiToken)?
            .token;
        let validation = last_validation_results(&self.store, &artifact.stage_run_id)?;
        let publisher_login = self.comments.authenticated_login().await?;
        let current_revision = implementation_issue_revision(&issue, service, &publisher_login);

        let Some(pulls) = self.pulls.as_ref() else {
            return Err(SymphonyError::TriageError(
                "automatic publication requires ImplementationPullRequestPort".into(),
            ));
        };

        self.record_event(
            Some(&intent.run_id),
            Some(&artifact.stage_run_id),
            "implementation_publication_started",
            serde_json::json!({
                "intent_id": intent.intent_id,
                "artifact_id": artifact.artifact_id,
                "bundle_id": bundle.artifact_id,
            }),
        )?;

        let publisher = AutomaticImplementationPublisher::new(
            self.comments.clone(),
            routing.clone(),
            pulls.clone(),
        );
        let draft = publisher
            .publish_automatic(
                &self.store,
                AutomaticPublishRequest {
                    intent,
                    issue_number: issue.issue_number,
                    issue_title: &issue.title,
                    current_issue_revision: &current_revision,
                    run_id: &intent.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &artifact.manifest,
                    validation: &validation,
                    branch: publication_branch,
                    base_branch,
                    remote_url: &remote_url,
                    github_token: Some(&github_token),
                    repo_owner,
                    approval_route_label: approval_label,
                    completion_route_state: completion_state,
                    storage_path: &self.config.storage_path,
                    max_pages: service
                        .triage
                        .max_intake_pages
                        .max(service.spec.max_intake_pages),
                },
            )
            .await?;

        self.record_event(
            Some(&intent.run_id),
            Some(&artifact.stage_run_id),
            "implementation_pr_created",
            serde_json::json!({
                "intent_id": intent.intent_id,
                "pr_number": draft.number,
                "pr_url": draft.url,
            }),
        )?;
        self.record_event(
            Some(&intent.run_id),
            Some(&artifact.stage_run_id),
            "implementation_route_applied",
            serde_json::json!({
                "intent_id": intent.intent_id,
                "completion_state": completion_state,
            }),
        )?;
        self.record_event(
            Some(&intent.run_id),
            Some(&artifact.stage_run_id),
            "implementation_completed",
            serde_json::json!({
                "intent_id": intent.intent_id,
                "pr_number": draft.number,
                "pr_url": draft.url,
            }),
        )?;
        self.store.set_implementation_decision(
            &intent.run_id,
            ImplementationDecision::Published,
            None,
        )?;
        self.store
            .complete_implementation_publication(&intent.intent_id, COMMENT_FINAL_STEP)?;
        Ok(())
    }

    async fn process_candidate(
        &mut self,
        service: &ServiceConfig,
        prompts: &LoadedPrompts,
        configuration_revision: &str,
        publisher_login: &str,
        candidate: &A3EligibleApprovedRun,
    ) -> Result<CandidateOutcome> {
        let approved = self
            .store
            .get_spec_artifact(&candidate.approved_artifact_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "approved artifact {} missing",
                    candidate.approved_artifact_id
                ))
            })?;

        let issue = self.issues.fetch_issue(&candidate.issue_id).await?;
        let current_revision = implementation_issue_revision(&issue, service, publisher_login);
        if current_revision != approved.issue_revision {
            self.record_event(
                Some(&candidate.run_id),
                None,
                "approved_spec_stale",
                serde_json::json!({
                    "issue": candidate.issue_identifier,
                    "approved_issue_revision": approved.issue_revision,
                    "current_issue_revision": current_revision,
                }),
            )?;
            return Ok(CandidateOutcome::Stale);
        }

        // Bound attempts before claiming.
        let prior = self.store.list_stage_attempts_for_revision(
            IMPLEMENTATION_STAGE_NAME,
            &candidate.run_id,
            &approved.issue_revision,
            configuration_revision,
        )?;
        if prior.len() as u32 >= service.implementation.max_attempts {
            return Ok(CandidateOutcome::Skipped);
        }

        let workspace_root = resolve_path(&service.workspace.root, "workspace.root")?;
        let repo_path = resolve_path(
            service.workspace.repo.as_deref().ok_or_else(|| {
                SymphonyError::InvalidWorkflowConfig(
                    "workspace.repo is required for the implementation stage".to_string(),
                )
            })?,
            "workspace.repo",
        )?;
        let base_branch = service
            .workspace
            .base_branch
            .clone()
            .unwrap_or_else(|| "main".to_string());
        let base_commit = resolve_base_commit(&repo_path, &base_branch)?;
        let execution_profile = if service.workspace.docker.is_some() {
            ExecutionProfile::Docker
        } else {
            ExecutionProfile::Local
        };

        let harness = match service.agent_backend {
            AgentBackend::Codex => TriageHarness::Codex,
            AgentBackend::KataCli => TriageHarness::Pi,
        };
        let model = match harness {
            TriageHarness::Pi => effective_pi_model(
                service.implementation.model.as_deref(),
                service.pi_agent.model.as_deref(),
            ),
            TriageHarness::Codex => None,
        };
        let command = agent_command(service)?;

        let stage = self
            .store
            .claim_implementation_attempt(ClaimAttemptRequest {
                forge_host: self.config.forge_host.clone(),
                repository: self.config.repository.clone(),
                issue_id: candidate.issue_id.clone(),
                issue_identifier: candidate.issue_identifier.clone(),
                issue_revision: approved.issue_revision.clone(),
                configuration_revision: configuration_revision.to_string(),
                owner_instance: self.config.owner_instance.clone(),
                harness: harness_name(harness).to_string(),
                model: model.clone(),
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })?;

        // Stick dispatch guards immediately so a mid-attempt crash still blocks
        // duplicate claim eligibility (retry only after decision=failed).
        self.store
            .upsert_implementation_state(UpsertImplementationStateRequest {
                run_id: stage.run_id.clone(),
                approved_artifact_id: approved.artifact_id.clone(),
                approved_version: approved.version,
                configuration_revision: configuration_revision.to_string(),
                decision: ImplementationDecision::Pending,
                successful_artifact_id: None,
                bundle_artifact_id: None,
                blocker: None,
            })?;

        self.store.store_implementation_attempt_inputs(
            &stage.stage_run_id,
            &ImplementationAttemptInputs {
                approved_artifact_id: approved.artifact_id.clone(),
                approved_version: approved.version,
                approval_issue_revision: approved.issue_revision.clone(),
                base_branch: base_branch.clone(),
                base_commit: base_commit.clone(),
                execution_profile,
                configuration_revision: configuration_revision.to_string(),
            },
        )?;

        if execution_profile == ExecutionProfile::Docker {
            // PR1: fail closed — host execution is refused when workspace.docker is set.
            let error = FactoryError::new(
                "docker_execution_required",
                "implementation",
                "workspace.docker is configured; host (local) implementation execution is refused until Docker orchestration is available in this build",
                false,
                None,
            );
            self.store
                .fail_attempt(&stage.stage_run_id, error.clone())?;
            // Leave decision=pending so eligibility does not retry forever.
            self.record_event(
                Some(&stage.run_id),
                Some(&stage.stage_run_id),
                "implementation_failed",
                serde_json::json!({
                    "error": error.remediation,
                    "code": error.code,
                }),
            )?;
            return Ok(CandidateOutcome::StartedFailed);
        }

        self.record_event(
            Some(&stage.run_id),
            Some(&stage.stage_run_id),
            "implementation_started",
            serde_json::json!({
                "status": "running",
                "attempt": stage.attempt,
                "approved_artifact_id": approved.artifact_id,
                "execution_profile": execution_profile.as_str(),
            }),
        )?;

        match self
            .run_attempt_pipeline(
                service,
                prompts,
                &stage.stage_run_id,
                &stage.run_id,
                &approved,
                &issue,
                &workspace_root,
                &repo_path,
                &base_commit,
                execution_profile,
                harness,
                model,
                &command,
                configuration_revision,
            )
            .await
        {
            Ok(AttemptResult::Previewed) => Ok(CandidateOutcome::StartedPreview),
            Ok(AttemptResult::AutomaticPublished) => {
                Ok(CandidateOutcome::StartedAutomaticPublished)
            }
            Ok(AttemptResult::AutomaticPending) => Ok(CandidateOutcome::StartedAutomaticPending),
            Ok(AttemptResult::AwaitingHuman) => Ok(CandidateOutcome::StartedAwaitingHuman),
            Ok(AttemptResult::Failed) => Ok(CandidateOutcome::StartedFailed),
            Err(error) => {
                let _ = self.store.fail_attempt(
                    &stage.stage_run_id,
                    FactoryError::new(
                        "implementation_failed",
                        "implementation",
                        error.to_string(),
                        true,
                        None,
                    ),
                );
                let _ = self.store.set_implementation_decision(
                    &stage.run_id,
                    ImplementationDecision::Failed,
                    None,
                );
                self.record_event(
                    Some(&stage.run_id),
                    Some(&stage.stage_run_id),
                    "implementation_failed",
                    serde_json::json!({"error": error.to_string()}),
                )?;
                Ok(CandidateOutcome::StartedFailed)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_attempt_pipeline(
        &mut self,
        service: &ServiceConfig,
        prompts: &LoadedPrompts,
        stage_run_id: &str,
        run_id: &str,
        approved: &SpecArtifactRecord,
        issue: &TriageIntakeIssue,
        workspace_root: &Path,
        repo_path: &Path,
        base_commit: &str,
        execution_profile: ExecutionProfile,
        harness: TriageHarness,
        model: Option<String>,
        command: &[String],
        configuration_revision: &str,
    ) -> Result<AttemptResult> {
        let approved_at = approved.received_at.to_rfc3339();
        let spec_markdown = render_approved_spec(
            run_id,
            &issue.identifier,
            &approved.artifact_id,
            approved.version,
            &approved_at,
            &approved.artifact,
        );
        let spec_path = expand_spec_file_path(
            &service.implementation.spec_file,
            &issue.identifier,
            run_id,
            &approved.artifact_id,
            approved.version,
        )
        .map_err(|error| SymphonyError::InvalidWorkflowConfig(error.to_string()))?;

        let stage_inputs = serde_json::json!({
            "issue": {
                "id": issue.issue_id,
                "identifier": issue.identifier,
                "title": issue.title,
                "body": issue.body,
            },
            "approved_spec": spec_markdown,
            "approved_spec_path": spec_path,
            "base_commit": base_commit,
        });

        let spawned = {
            let store = self.store.clone();
            let stage_run_id = stage_run_id.to_string();
            let owner_instance = self.config.owner_instance.clone();
            Some(
                std::sync::Arc::new(move |spawn: crate::triage::runner::TriageSpawnInfo| {
                    let mut durable = store.clone();
                    if let Err(error) = FactoryRunStore::record_attempt_process(
                        &mut durable,
                        RecordAttemptProcessRequest {
                            stage_run_id: stage_run_id.clone(),
                            owner_instance: owner_instance.clone(),
                            identity: spawn.identity,
                            workspace_path: Some(
                                spawn.workspace_path.to_string_lossy().to_string(),
                            ),
                            output_path: Some(spawn.output_path.to_string_lossy().to_string()),
                        },
                    ) {
                        tracing::error!(
                            %error,
                            "failed to persist spawned implementation process identity"
                        );
                    }
                }) as crate::triage::runner::TriageSpawnSink,
            )
        };

        let turn_request = ImplementationTurnRequest {
            attempt_id: stage_run_id.to_string(),
            workspace_root: workspace_root.to_path_buf(),
            repo_path: repo_path.to_path_buf(),
            base_commit: base_commit.to_string(),
            command: command.to_vec(),
            prompt: prompts.implement.clone(),
            timeout_ms: service.implementation.invocation_timeout_ms,
            model: model.clone(),
            harness,
            issue: TriageIssueIdentity {
                id: issue.issue_id.clone(),
                identifier: issue.identifier.clone(),
                title: issue.title.clone(),
            },
            codex: Some(service.codex.clone()),
            approved_spec_markdown: spec_markdown.clone(),
            approved_spec_path: spec_path.clone(),
            stage_inputs: stage_inputs.clone(),
            spawned,
            execution_profile,
            existing_workspace: None,
        };

        let runner = ImplementationRunner;
        let mut turn = runner.run_local_turn(&turn_request, &self.harness).await?;

        self.store_turn(
            stage_run_id,
            1,
            ImplementationTurnKind::Implement,
            harness,
            model.as_deref(),
            execution_profile,
            &turn.usage,
            Some(&turn.output_bytes),
            None,
        )?;

        let mut manifest = match parse_and_validate_manifest(&turn.output_bytes, &approved.artifact)
        {
            Ok(manifest) => manifest,
            Err(error) => {
                self.fail_implementation_attempt(
                    stage_run_id,
                    run_id,
                    FactoryError::new(
                        "invalid_manifest",
                        "implementation",
                        error.to_string(),
                        true,
                        None,
                    ),
                )?;
                return Ok(AttemptResult::Failed);
            }
        };

        if manifest.status == ManifestStatus::Blocked {
            let blocker = manifest.blocker.clone().unwrap_or(ImplementationBlocker {
                kind: BlockerKind::Repository,
                summary: "blocked".into(),
                evidence: "no blocker payload".into(),
            });
            if is_spec_gap(&blocker) {
                let artifact = self.store.store_implementation_artifact(
                    StoreImplementationArtifactRequest {
                        stage_run_id: stage_run_id.to_string(),
                        approved_artifact_id: approved.artifact_id.clone(),
                        approved_version: approved.version,
                        issue_revision: approved.issue_revision.clone(),
                        configuration_revision: configuration_revision.to_string(),
                        manifest: manifest.clone(),
                        base_commit: base_commit.to_string(),
                        head_commit: None,
                        approved_spec_path: spec_path.clone(),
                        validation_cycles: 0,
                        execution_profile,
                        bytes_len: turn.output_bytes.len() as u64,
                        usage: turn.usage.clone(),
                    },
                )?;
                let intent = self.store.create_implementation_publication_intent(
                    run_id,
                    Some(&artifact.artifact_id),
                    ImplementationPublicationKind::Diagnostic,
                    &serde_json::json!({
                        "kind": "spec_gap",
                        "message": blocker.summary,
                    }),
                )?;
                let publisher = ImplementationPublisher::new(self.comments.clone());
                let _ = publisher
                    .publish_diagnostic(
                        &self.store,
                        DiagnosticPublishRequest {
                            intent: &intent,
                            issue_number: issue.issue_number,
                            run_id,
                            message: &blocker.summary,
                            max_pages: 5,
                        },
                    )
                    .await;
                self.record_event(
                    Some(run_id),
                    Some(stage_run_id),
                    "implementation_spec_gap",
                    serde_json::json!({"blocker": blocker}),
                )?;
                return Ok(AttemptResult::AwaitingHuman);
            }
            self.fail_implementation_attempt(
                stage_run_id,
                run_id,
                FactoryError::new(
                    format!("blocked_{}", blocker.kind.as_str()),
                    "implementation",
                    blocker.summary,
                    true,
                    None,
                ),
            )?;
            return Ok(AttemptResult::Failed);
        }

        let expected_head = manifest
            .head_commit
            .clone()
            .ok_or_else(|| SymphonyError::TriageError("completed manifest missing head".into()))?;

        let mut assessment = match assess_postconditions(
            &turn.workspace_path,
            base_commit,
            &spec_path,
            spec_markdown.as_bytes(),
            &expected_head,
        ) {
            Ok(a) => a,
            Err(error) => {
                self.fail_implementation_attempt(
                    stage_run_id,
                    run_id,
                    FactoryError::new(
                        "postcondition_failed",
                        "implementation",
                        error.to_string(),
                        true,
                        None,
                    ),
                )?;
                return Ok(AttemptResult::Failed);
            }
        };

        let validator = ValidationExecutor::new(execution_profile);
        let max_cycles = service.implementation.max_validation_cycles.max(1);
        let mut validation_results: Vec<ValidationCommandResult> = Vec::new();
        let mut cycles_completed = 0u32;

        for cycle in 1..=max_cycles {
            let outcome = validator
                .run_cycle(
                    &turn.workspace_path,
                    &service.implementation.validation,
                    cycle,
                )
                .await?;
            cycles_completed = cycle;
            self.store
                .store_validation_cycle(StoreValidationCycleRequest {
                    cycle_id: Uuid::now_v7().to_string(),
                    stage_run_id: stage_run_id.to_string(),
                    cycle,
                    passed: outcome.passed,
                    commands: outcome.commands.clone(),
                    started_at: outcome
                        .commands
                        .first()
                        .map(|c| c.started_at)
                        .unwrap_or_else(Utc::now),
                    completed_at: outcome
                        .commands
                        .last()
                        .map(|c| c.completed_at)
                        .unwrap_or_else(Utc::now),
                })?;
            self.record_event(
                Some(run_id),
                Some(stage_run_id),
                "implementation_validation",
                serde_json::json!({
                    "cycle": cycle,
                    "passed": outcome.passed,
                    "commands": outcome.commands.iter().map(|c| {
                        serde_json::json!({"name": c.name, "passed": c.passed, "duration_ms": c.duration_ms})
                    }).collect::<Vec<_>>(),
                }),
            )?;
            validation_results = outcome.commands;
            if outcome.passed {
                break;
            }
            if cycle == max_cycles {
                self.record_event(
                    Some(run_id),
                    Some(stage_run_id),
                    "implementation_validation_exhausted",
                    serde_json::json!({"cycles": cycle}),
                )?;
                self.fail_implementation_attempt(
                    stage_run_id,
                    run_id,
                    FactoryError::new(
                        "validation_exhausted",
                        "implementation",
                        "validation cycles exhausted".to_string(),
                        true,
                        None,
                    ),
                )?;
                return Ok(AttemptResult::Failed);
            }

            // Repair turn in the same workspace. Ordinal 1 is implement; repairs are cycle+1.
            let repair_inputs = serde_json::json!({
                "issue": stage_inputs.get("issue"),
                "approved_spec": spec_markdown,
                "approved_spec_path": spec_path,
                "previous_manifest": manifest,
                "validation_failure": validation_results,
            });
            let repair_request = ImplementationTurnRequest {
                prompt: prompts.repair.clone(),
                stage_inputs: repair_inputs,
                existing_workspace: Some(turn.workspace_path.clone()),
                ..turn_request.clone()
            };
            turn = runner
                .run_local_turn(&repair_request, &self.harness)
                .await?;
            self.store_turn(
                stage_run_id,
                cycle + 1,
                ImplementationTurnKind::Repair,
                harness,
                model.as_deref(),
                execution_profile,
                &turn.usage,
                Some(&turn.output_bytes),
                None,
            )?;
            manifest = parse_and_validate_manifest(&turn.output_bytes, &approved.artifact)
                .map_err(|error| {
                    SymphonyError::TriageError(format!("repair manifest invalid: {error}"))
                })?;
            if manifest.status != ManifestStatus::Completed {
                self.fail_implementation_attempt(
                    stage_run_id,
                    run_id,
                    FactoryError::new(
                        "repair_not_completed",
                        "implementation",
                        "repair turn did not produce a completed manifest".to_string(),
                        true,
                        None,
                    ),
                )?;
                return Ok(AttemptResult::Failed);
            }
            let expected_head = manifest.head_commit.clone().ok_or_else(|| {
                SymphonyError::TriageError(
                    "repair completed manifest missing head_commit".to_string(),
                )
            })?;
            assessment = assess_postconditions(
                &turn.workspace_path,
                base_commit,
                &spec_path,
                spec_markdown.as_bytes(),
                &expected_head,
            )?;
        }

        // Export result bundle and store blob (blocking IO off the async runtime).
        let result_bundle = turn.attempt_root.join("result.bundle");
        let workspace_path = turn.workspace_path.clone();
        let base_commit_owned = base_commit.to_string();
        let head_commit = assessment.head_commit.clone();
        let arts = artifacts_dir(&self.config.storage_path);
        let max_bundle_bytes = service.implementation.max_bundle_bytes;
        let (sha256, bytes_len) = tokio::task::spawn_blocking(move || {
            create_result_bundle(
                &workspace_path,
                &base_commit_owned,
                &head_commit,
                &result_bundle,
            )?;
            store_blob_atomic(&arts, BlobSource::Path(&result_bundle), max_bundle_bytes)
        })
        .await
        .map_err(|error| {
            SymphonyError::TriageError(format!("bundle worker join failed: {error}"))
        })??;
        let approved_spec_blob_sha = blob_sha_of_file(&turn.workspace_path.join(&spec_path))?;

        let artifact =
            self.store
                .store_implementation_artifact(StoreImplementationArtifactRequest {
                    stage_run_id: stage_run_id.to_string(),
                    approved_artifact_id: approved.artifact_id.clone(),
                    approved_version: approved.version,
                    issue_revision: approved.issue_revision.clone(),
                    configuration_revision: configuration_revision.to_string(),
                    manifest: manifest.clone(),
                    base_commit: base_commit.to_string(),
                    head_commit: Some(assessment.head_commit.clone()),
                    approved_spec_path: spec_path.clone(),
                    validation_cycles: cycles_completed,
                    execution_profile,
                    bytes_len: turn.output_bytes.len() as u64,
                    usage: turn.usage.clone(),
                })?;

        let bundle = self
            .store
            .store_bundle_artifact(StoreBundleArtifactRequest {
                stage_run_id: stage_run_id.to_string(),
                implementation_artifact_id: artifact.artifact_id.clone(),
                base_commit: base_commit.to_string(),
                head_commit: assessment.head_commit.clone(),
                tree_sha: assessment.tree_sha.clone(),
                sha256,
                bytes_len,
                changed_file_count: assessment.changed_paths.len() as u32,
                changed_paths: assessment.changed_paths.clone(),
                approved_spec_path: spec_path.clone(),
                approved_spec_blob_sha,
                harness: harness_name(harness).to_string(),
                model: model.clone(),
                execution_profile,
                created_at: Utc::now(),
                verified_at: Utc::now(),
            })?;

        let _ = bundle;
        let decision = if service.implementation.mode == ImplementationMode::Automatic {
            ImplementationDecision::Pending
        } else {
            ImplementationDecision::Previewed
        };
        let _ = self
            .store
            .set_implementation_decision(run_id, decision, None);

        let kind = if service.implementation.mode == ImplementationMode::Automatic {
            ImplementationPublicationKind::Automatic
        } else {
            ImplementationPublicationKind::Preview
        };
        let mut desired_effects = serde_json::json!({
            "mode": service.implementation.mode.as_str(),
            "changed_paths": assessment.changed_paths,
        });
        if kind == ImplementationPublicationKind::Automatic {
            let repo_owner = service
                .tracker
                .repo_owner
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| SymphonyError::TriageError("tracker.repo_owner required".into()))?;
            let repo_name = service
                .tracker
                .repo_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| SymphonyError::TriageError("tracker.repo_name required".into()))?;
            let base_branch = service.workspace.base_branch.as_deref().unwrap_or("main");
            let publication_branch =
                branch_name_for_issue(&service.workspace.branch_prefix, &issue.identifier);
            let remote_url = intent_remote_url(&self.config.forge_host, repo_owner, repo_name)?;
            let completion_state = automatic_completion_state(service)?;
            desired_effects = serde_json::json!({
                "mode": service.implementation.mode.as_str(),
                "changed_paths": assessment.changed_paths,
                "approval_route_label": service.spec.approval_route.label,
                "completion_route_state": completion_state,
                "base_branch": base_branch,
                "publication_branch": publication_branch,
                "repo_owner": repo_owner,
                "repo_name": repo_name,
                "remote_url": remote_url,
            });
        }
        let intent = self.store.create_implementation_publication_intent(
            run_id,
            Some(&artifact.artifact_id),
            kind,
            &desired_effects,
        )?;

        if kind == ImplementationPublicationKind::Preview {
            let publisher = ImplementationPublisher::new(self.comments.clone());
            publisher
                .publish_preview(
                    &self.store,
                    PreviewPublishRequest {
                        intent: &intent,
                        issue_number: issue.issue_number,
                        run_id,
                        stage_run_id,
                        artifact: &artifact,
                        manifest: &manifest,
                        changed_paths: &assessment.changed_paths,
                        validation: &validation_results,
                        max_pages: 5,
                    },
                )
                .await?;

            self.record_event(
                Some(run_id),
                Some(stage_run_id),
                "implementation_preview_published",
                serde_json::json!({
                    "artifact_id": artifact.artifact_id,
                    "intent_id": intent.intent_id,
                    "head_commit": assessment.head_commit,
                }),
            )?;
            return Ok(AttemptResult::Previewed);
        }

        // Automatic: create intent and reconcile immediately (also recoverable via pending list).
        match self.reconcile_automatic_publication(service, &intent).await {
            Ok(()) => Ok(AttemptResult::AutomaticPublished),
            Err(error) => {
                tracing::warn!(
                    event = "implementation_automatic_publication_failed",
                    intent_id = %intent.intent_id,
                    error = %error,
                    "automatic publication will retry via pending reconcile"
                );
                Ok(AttemptResult::AutomaticPending)
            }
        }
    }

    fn fail_implementation_attempt(
        &self,
        stage_run_id: &str,
        run_id: &str,
        error: FactoryError,
    ) -> Result<()> {
        let mut store = self.store.clone();
        store.fail_attempt(stage_run_id, error)?;
        store.set_implementation_decision(run_id, ImplementationDecision::Failed, None)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn store_turn(
        &self,
        stage_run_id: &str,
        ordinal: u32,
        kind: ImplementationTurnKind,
        harness: TriageHarness,
        model: Option<&str>,
        execution_profile: ExecutionProfile,
        usage: &StageUsage,
        output_bytes: Option<&[u8]>,
        error: Option<FactoryError>,
    ) -> Result<()> {
        let output_json = output_bytes.and_then(|bytes| serde_json::from_slice(bytes).ok());
        self.store
            .store_implementation_turn(StoreImplementationTurnRequest {
                turn_id: Uuid::now_v7().to_string(),
                stage_run_id: stage_run_id.to_string(),
                ordinal,
                kind,
                status: if error.is_some() {
                    ImplementationTurnStatus::Failed
                } else {
                    ImplementationTurnStatus::Completed
                },
                harness: harness_name(harness).to_string(),
                model: model.map(str::to_string),
                execution_profile,
                usage: usage.clone(),
                output_json,
                error,
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
            })?;
        Ok(())
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

#[derive(Debug)]
enum CandidateOutcome {
    StartedPreview,
    StartedAutomaticPublished,
    StartedAutomaticPending,
    StartedAwaitingHuman,
    StartedFailed,
    Stale,
    Skipped,
}

#[derive(Debug)]
enum AttemptResult {
    Previewed,
    AutomaticPublished,
    AutomaticPending,
    AwaitingHuman,
    Failed,
}

struct LoadedPrompts {
    implement: String,
    repair: String,
}

fn load_prompts(workflow_dir: &Path, service: &ServiceConfig) -> Result<LoadedPrompts> {
    let implement = read_prompt(workflow_dir, &service.implementation.prompt)?;
    let repair = read_prompt(workflow_dir, &service.implementation.repair_prompt)?;
    Ok(LoadedPrompts { implement, repair })
}

fn read_prompt(workflow_dir: &Path, relative: &str) -> Result<String> {
    let path = if Path::new(relative).is_absolute() {
        PathBuf::from(relative)
    } else {
        workflow_dir.join(relative)
    };
    fs::read_to_string(&path).map_err(|error| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "failed reading implementation prompt {}: {error}",
            path.display()
        ))
    })
}

fn resolve_path(value: &str, field: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if !path.exists() {
        return Err(SymphonyError::InvalidWorkflowConfig(format!(
            "{field} does not exist: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn agent_command(service: &ServiceConfig) -> Result<Vec<String>> {
    let command = match service.agent_backend {
        AgentBackend::Codex => &service.codex.command,
        AgentBackend::KataCli => &service.pi_agent.command,
    };
    if command.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "agent command is required for the implementation stage".to_string(),
        ));
    }
    Ok(command.clone())
}

fn harness_name(harness: TriageHarness) -> &'static str {
    match harness {
        TriageHarness::Pi => "pi",
        TriageHarness::Codex => "codex",
    }
}

fn implementation_issue_revision(
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
        .map(|comment| {
            serde_json::json!({
                "id": comment.id,
                "body": comment.body,
                "created_at": comment.created_at,
                "updated_at": comment.updated_at
            })
        })
        .collect();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "title": issue.title,
        "body": issue.body,
        "labels": labels,
        "assignees": issue.assignees,
        "milestone": issue.milestone.as_ref().map(|m| (&m.number, &m.title)),
        "comments": comments,
    }))
    .expect("JSON serializes");
    hex::encode(Sha256::digest(bytes))
}

fn issue_number_from_run(store: &SharedFactoryStore, run_id: &str) -> Result<u64> {
    let run = store
        .get_run_by_id(run_id)?
        .ok_or_else(|| SymphonyError::StorageError(format!("run {run_id} missing")))?;
    run.issue_id.parse().map_err(|_| {
        SymphonyError::TriageError(format!("issue id {} is not numeric", run.issue_id))
    })
}

fn intent_remote_url(forge_host: &str, repo_owner: &str, repo_name: &str) -> Result<String> {
    resolve_publication_remote(&serde_json::Value::Null, forge_host, repo_owner, repo_name)
}

fn required_desired_effect<'a>(
    desired_effects: &'a serde_json::Value,
    key: &str,
) -> Result<&'a str> {
    desired_effects
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            SymphonyError::TriageError(format!(
                "automatic publication intent is missing pinned {key}"
            ))
        })
}

/// Whether the intent's stored error already describes this reconcile failure,
/// so the generic retryable wrapper neither charges the retry budget twice nor
/// clobbers the specific error code the inner path recorded.
///
/// The comparison is a suffix match because `SymphonyError` variants prefix
/// their payload on `Display` (`"triage error: {0}"`), while `remediation`
/// holds the unprefixed message.
fn already_recorded(last: &FactoryError, error_message: &str) -> bool {
    last.retryable
        && !last.remediation.is_empty()
        && error_message.ends_with(last.remediation.as_str())
}

/// Exponential backoff for the given reconcile attempt count, capped at
/// [`AUTOMATIC_PUBLICATION_BACKOFF_MAX_SECS`].
fn automatic_backoff_secs(retry_count: u32) -> i64 {
    let exponent = retry_count.saturating_sub(1).min(16);
    AUTOMATIC_PUBLICATION_BACKOFF_BASE_SECS
        .saturating_mul(1i64 << exponent)
        .min(AUTOMATIC_PUBLICATION_BACKOFF_MAX_SECS)
}

/// Whether a pending automatic intent has waited out its backoff. A never-tried
/// intent (`retry_count == 0`) is always ready, so first publication stays
/// immediate.
fn automatic_retry_ready(retry_count: u32, updated_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    if retry_count == 0 {
        return true;
    }
    (now - updated_at).num_seconds() >= automatic_backoff_secs(retry_count)
}

fn automatic_completion_state(service: &ServiceConfig) -> Result<&str> {
    service
        .implementation
        .completion_route
        .as_ref()
        .map(|route| route.state.trim())
        .filter(|state| !state.is_empty())
        .ok_or_else(|| {
            SymphonyError::TriageError(
                "implementation.completion_route.state required when mode is automatic".into(),
            )
        })
}

fn last_validation_results(
    store: &SharedFactoryStore,
    stage_run_id: &str,
) -> Result<Vec<ValidationCommandResult>> {
    let cycles = store.list_validation_cycles(stage_run_id)?;
    Ok(cycles
        .into_iter()
        .last()
        .map(|cycle| cycle.commands)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::{GithubIssueComment, GithubUser};
    use crate::implementation::domain::ImplementationValidationCommand;
    use crate::implementation::runner::ImplementationTurnResult;
    use crate::spec::domain::{SpecArtifact, SpecPublicationKind};
    use crate::triage::runner::AttemptLayout;
    use crate::triage::store::StoreSpecArtifactRequest;
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::Mutex;
    use tempfile::tempdir;

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
                next_id: Mutex::new(800),
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
            Ok(self.comments.lock().unwrap().values().cloned().collect())
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
            let comment = comments.get_mut(&comment_id).unwrap();
            comment.body = Some(body.to_string());
            Ok(comment.clone())
        }
    }

    struct FakeIssues {
        issue: Mutex<TriageIntakeIssue>,
    }

    #[async_trait]
    impl ImplementationIssuePort for Arc<FakeIssues> {
        async fn fetch_issue(&self, _issue_id: &str) -> Result<TriageIntakeIssue> {
            Ok(self.issue.lock().unwrap().clone())
        }
    }

    /// Commits approved spec + code file and writes a completed manifest.
    struct FakeHarness {
        repair_touch: bool,
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl ImplementationHarness for FakeHarness {
        async fn run_turn(
            &self,
            request: &ImplementationTurnRequest,
            layout: &AttemptLayout,
        ) -> Result<StageUsage> {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            let call = *calls;
            drop(calls);

            let spec = layout.workspace_path.join(&request.approved_spec_path);
            fs::create_dir_all(spec.parent().unwrap()).unwrap();
            fs::write(&spec, request.approved_spec_markdown.as_bytes()).unwrap();
            assert!(
                layout.stage_input_path.join("issue.json").is_file(),
                "stage-input/issue.json missing"
            );
            assert!(
                layout.stage_input_path.join("approved-spec.md").is_file(),
                "stage-input/approved-spec.md missing"
            );
            if call >= 2 {
                assert!(
                    layout
                        .stage_input_path
                        .join("previous-manifest.json")
                        .is_file(),
                    "stage-input/previous-manifest.json missing on repair"
                );
                assert!(
                    layout
                        .stage_input_path
                        .join("validation-failure.json")
                        .is_file(),
                    "stage-input/validation-failure.json missing on repair"
                );
            }
            let code = layout.workspace_path.join("src/feature.rs");
            fs::create_dir_all(code.parent().unwrap()).unwrap();
            fs::write(&code, format!("// call {call}\n")).unwrap();
            if self.repair_touch && call >= 2 {
                fs::write(layout.workspace_path.join("repaired"), b"ok\n").unwrap();
            }
            assert!(Command::new("git")
                .args(["add", "-A"])
                .current_dir(&layout.workspace_path)
                .status()
                .unwrap()
                .success());
            assert!(Command::new("git")
                .args([
                    "-c",
                    "user.email=t@t.com",
                    "-c",
                    "user.name=t",
                    "commit",
                    "-m",
                    "impl"
                ])
                .current_dir(&layout.workspace_path)
                .status()
                .unwrap()
                .success());
            let head = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&layout.workspace_path)
                .output()
                .unwrap();
            let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
            let manifest = serde_json::json!({
                "schema_version": 1,
                "status": "completed",
                "head_commit": head,
                "summary": "Implements the approved feature.",
                "acceptance_criteria": [{
                    "index": 1,
                    "status": "implemented",
                    "evidence": [{
                        "kind": "repository",
                        "reference": "src/feature.rs",
                        "summary": "Feature module added."
                    }]
                }],
                "known_limitations": []
            });
            fs::write(
                &layout.output_path,
                serde_json::to_vec_pretty(&manifest).unwrap(),
            )
            .unwrap();
            let _ = ImplementationTurnResult {
                workspace_path: layout.workspace_path.clone(),
                output_path: layout.output_path.clone(),
                output_bytes: vec![],
                usage: StageUsage::default(),
                attempt_root: layout.attempt_root.clone(),
            };
            Ok(StageUsage::default())
        }
    }

    fn init_repo(path: &Path) {
        for args in [
            ["init"].as_slice(),
            ["config", "user.email", "t@t.com"].as_slice(),
            ["config", "user.name", "T"].as_slice(),
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(path)
                .status()
                .unwrap()
                .success());
        }
        fs::write(path.join("README.md"), "base\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(path)
            .status()
            .unwrap()
            .success());
        // Ensure base_branch main exists.
        let _ = Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(path)
            .status();
    }

    fn base_issue() -> TriageIntakeIssue {
        TriageIntakeIssue {
            issue_id: "42".into(),
            issue_number: 42,
            identifier: "#42".into(),
            title: "Add feature".into(),
            body: "Please implement.".into(),
            labels: vec!["ready".into()],
            non_managed_labels: vec!["ready".into()],
            assignees: vec![],
            milestone: None,
            comments: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            forge_host: "github.com".into(),
            repository: "acme/repo".into(),
            in_project: true,
        }
    }

    async fn seed_approved(store: &SharedFactoryStore, issue_revision: &str) -> SpecArtifactRecord {
        let stage = store
            .claim_spec_attempt(ClaimAttemptRequest {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                issue_id: "42".into(),
                issue_identifier: "#42".into(),
                issue_revision: issue_revision.into(),
                configuration_revision: "spec-cfg".into(),
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
        let artifact = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: stage.stage_run_id,
                issue_revision: issue_revision.into(),
                configuration_revision: "spec-cfg".into(),
                artifact: SpecArtifact {
                    schema_version: 1,
                    product_behavior: "Do the thing".into(),
                    technical_approach: "Add a module".into(),
                    acceptance_criteria: vec!["Feature module exists".into()],
                    open_decisions: vec![],
                },
                review_cycles: 1,
                unresolved_blocking_findings: vec![],
                bytes_len: 64,
                usage: StageUsage::default(),
            })
            .unwrap();
        store
            .pin_spec_approval(&artifact.run_id, &artifact.artifact_id)
            .unwrap();
        let intent = store
            .create_spec_publication_intent(
                &artifact.run_id,
                Some(&artifact.artifact_id),
                SpecPublicationKind::Approval,
                &serde_json::json!({"intake_label":"ready"}),
            )
            .unwrap();
        store
            .finalize_spec_approval(&artifact.run_id, &intent.intent_id, "route_applied")
            .unwrap();
        artifact
    }

    fn service_config(repo: &Path, workspace: &Path, workflow: &Path) -> ServiceConfig {
        let mut service = ServiceConfig::default();
        service.implementation.enabled = true;
        service.implementation.mode = ImplementationMode::Preview;
        service.implementation.prompt = "prompts/implementation.md".into();
        service.implementation.repair_prompt = "prompts/implementation-repair.md".into();
        service.implementation.validation = vec![ImplementationValidationCommand {
            name: "true".into(),
            command: "true".into(),
            timeout_ms: 5_000,
        }];
        service.implementation.max_validation_cycles = 3;
        service.implementation.max_attempts = 3;
        service.workspace.root = workspace.display().to_string();
        service.workspace.repo = Some(repo.display().to_string());
        service.workspace.base_branch = Some("main".into());
        service.pi_agent.command = vec!["true".into()];
        service.agent_backend = AgentBackend::KataCli;
        let _ = workflow;
        service
    }

    fn write_prompts(workflow: &Path) {
        fs::create_dir_all(workflow.join("prompts")).unwrap();
        fs::write(
            workflow.join("prompts/implementation.md"),
            "implement please",
        )
        .unwrap();
        fs::write(
            workflow.join("prompts/implementation-repair.md"),
            "repair please",
        )
        .unwrap();
    }

    #[tokio::test]
    async fn coordinator_preview_path_with_fake_harness() {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        let workspace = root.path().join("workspaces");
        let workflow = root.path().join("workflow");
        let db = root.path().join("factory.db");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&workflow).unwrap();
        init_repo(&repo);
        write_prompts(&workflow);

        let store = SharedFactoryStore::open(&db, 5_000).unwrap();
        let issue = base_issue();
        let mut service = service_config(&repo, &workspace, &workflow);
        let rev = implementation_issue_revision(&issue, &service, "symphony-bot");
        let _approved = seed_approved(&store, &rev).await;

        let comments = FakeComments::new("symphony-bot");
        let issues = Arc::new(FakeIssues {
            issue: Mutex::new(issue),
        });
        let mut coordinator = ImplementationCoordinator::with_harness(
            store.clone(),
            comments.clone(),
            issues,
            ImplementationCoordinatorConfig {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                owner_instance: "test".into(),
                workflow_dir: workflow,
                storage_path: db,
            },
            FakeHarness {
                repair_touch: false,
                calls: Mutex::new(0),
            },
        );

        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.attempts_completed, 1);
        assert_eq!(summary.preview_published, 1);
        assert!(!comments.comments.lock().unwrap().is_empty());
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
        assert!(body.contains("implementation preview"));
        assert!(body.contains("No remote branch"));

        // Second poll should find no candidates (successful artifact exists).
        let summary2 = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary2.candidates_seen, 0);
        let _ = &mut service;
    }

    #[tokio::test]
    async fn coordinator_repairs_after_validation_failure() {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        let workspace = root.path().join("workspaces");
        let workflow = root.path().join("workflow");
        let db = root.path().join("factory.db");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&workflow).unwrap();
        init_repo(&repo);
        write_prompts(&workflow);

        let store = SharedFactoryStore::open(&db, 5_000).unwrap();
        let issue = base_issue();
        let mut service = service_config(&repo, &workspace, &workflow);
        // Fail until `repaired` exists (written by FakeHarness on repair turn).
        service.implementation.validation = vec![ImplementationValidationCommand {
            name: "needs-repair".into(),
            command: "test -f repaired".into(),
            timeout_ms: 5_000,
        }];
        let rev = implementation_issue_revision(&issue, &service, "symphony-bot");
        let _ = seed_approved(&store, &rev).await;

        let comments = FakeComments::new("symphony-bot");
        let issues = Arc::new(FakeIssues {
            issue: Mutex::new(issue),
        });
        let mut coordinator = ImplementationCoordinator::with_harness(
            store,
            comments.clone(),
            issues,
            ImplementationCoordinatorConfig {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                owner_instance: "test".into(),
                workflow_dir: workflow,
                storage_path: db,
            },
            FakeHarness {
                repair_touch: true,
                calls: Mutex::new(0),
            },
        );

        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.attempts_completed, 1);
        assert_eq!(summary.preview_published, 1);
        assert!(!comments.comments.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stale_issue_revision_skips_claim() {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        let workspace = root.path().join("workspaces");
        let workflow = root.path().join("workflow");
        let db = root.path().join("factory.db");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&workflow).unwrap();
        init_repo(&repo);
        write_prompts(&workflow);

        let store = SharedFactoryStore::open(&db, 5_000).unwrap();
        let issue = base_issue();
        let service = service_config(&repo, &workspace, &workflow);
        let rev = implementation_issue_revision(&issue, &service, "symphony-bot");
        let _ = seed_approved(&store, &rev).await;

        let mut stale = issue.clone();
        stale.body = "changed after approval".into();
        let comments = FakeComments::new("symphony-bot");
        let issues = Arc::new(FakeIssues {
            issue: Mutex::new(stale),
        });
        let mut coordinator = ImplementationCoordinator::with_harness(
            store,
            comments,
            issues,
            ImplementationCoordinatorConfig {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                owner_instance: "test".into(),
                workflow_dir: workflow,
                storage_path: db,
            },
            FakeHarness {
                repair_touch: false,
                calls: Mutex::new(0),
            },
        );
        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.stale_skipped, 1);
        assert_eq!(summary.attempts_started, 0);
    }

    #[tokio::test]
    async fn docker_profile_fails_closed_without_local_execution() {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        let workspace = root.path().join("workspaces");
        let workflow = root.path().join("workflow");
        let db = root.path().join("factory.db");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&workflow).unwrap();
        init_repo(&repo);
        write_prompts(&workflow);

        let store = SharedFactoryStore::open(&db, 5_000).unwrap();
        let issue = base_issue();
        let mut service = service_config(&repo, &workspace, &workflow);
        service.workspace.docker = Some(crate::domain::DockerConfig {
            image: "symphony-impl:test".into(),
            ..Default::default()
        });
        let rev = implementation_issue_revision(&issue, &service, "symphony-bot");
        let approved = seed_approved(&store, &rev).await;

        let comments = FakeComments::new("symphony-bot");
        let issues = Arc::new(FakeIssues {
            issue: Mutex::new(issue),
        });
        let mut coordinator = ImplementationCoordinator::with_harness(
            store.clone(),
            comments,
            issues,
            ImplementationCoordinatorConfig {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                owner_instance: "test".into(),
                workflow_dir: workflow,
                storage_path: db,
            },
            FakeHarness {
                repair_touch: false,
                calls: Mutex::new(0),
            },
        );
        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.attempts_started, 1);
        assert_eq!(summary.attempts_failed, 1);
        assert_eq!(summary.preview_published, 0);

        let impl_state = store
            .get_implementation_state(&approved.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(impl_state.decision, ImplementationDecision::Pending);
        // Pending excludes eligibility (no silent local retry).
        let configuration_revision = implementation_configuration_revision(
            &service.implementation,
            "implement please",
            "repair please",
        );
        assert!(store
            .list_a3_eligible_approved_runs(&configuration_revision)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn automatic_reconcile_persists_unhandled_retryable_error() {
        let root = tempdir().unwrap();
        let workflow = root.path().join("workflow");
        let db = root.path().join("factory.db");
        fs::create_dir_all(&workflow).unwrap();

        let store = SharedFactoryStore::open(&db, 5_000).unwrap();
        let approved = seed_approved(&store, "rev").await;
        let intent = store
            .create_implementation_publication_intent(
                &approved.run_id,
                None,
                ImplementationPublicationKind::Automatic,
                &serde_json::json!({}),
            )
            .unwrap();
        let comments = FakeComments::new("symphony-bot");
        let issues = Arc::new(FakeIssues {
            issue: Mutex::new(base_issue()),
        });
        let mut coordinator = ImplementationCoordinator::with_harness(
            store.clone(),
            comments,
            issues,
            ImplementationCoordinatorConfig {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                owner_instance: "test".into(),
                workflow_dir: workflow,
                storage_path: db,
            },
            FakeHarness {
                repair_touch: false,
                calls: Mutex::new(0),
            },
        );

        coordinator
            .poll_once(&ServiceConfig::default())
            .await
            .unwrap();

        let recovered = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.status, PublicationStatus::Pending);
        assert_eq!(recovered.retry_count, 1);
        assert_eq!(
            recovered
                .last_error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("publication_retryable")
        );
    }

    #[test]
    fn publication_remote_is_derived_from_pinned_forge_identity() {
        assert_eq!(
            intent_remote_url("github.com", "acme", "repo").unwrap(),
            "https://github.com/acme/repo.git"
        );
        assert_eq!(
            intent_remote_url("github.example.com", "acme", "repo.git").unwrap(),
            "https://github.example.com/acme/repo.git"
        );
    }

    #[test]
    fn automatic_reconciliation_requires_pinned_desired_effects() {
        let desired = serde_json::json!({
            "publication_branch": "symphony/KATA-42",
            "repo_owner": "acme",
        });
        assert_eq!(
            required_desired_effect(&desired, "publication_branch").unwrap(),
            "symphony/KATA-42"
        );
        let err = required_desired_effect(&desired, "repo_name").unwrap_err();
        assert!(err.to_string().contains("pinned repo_name"));
    }

    #[test]
    fn already_recorded_matches_through_the_display_prefix() {
        let drift = FactoryError::new(
            "issue_revision_drift",
            "implementation_publication",
            "issue revision drifted: approved=a current=b",
            true,
            None,
        );
        // The inner path stores the unprefixed remediation; the reconcile loop
        // compares against the Display form, which adds "triage error: ".
        let displayed =
            SymphonyError::TriageError("issue revision drifted: approved=a current=b".into())
                .to_string();
        assert!(already_recorded(&drift, &displayed));
        assert!(!already_recorded(&drift, "triage error: something else"));

        // A non-retryable or empty remediation never suppresses recording.
        let terminal = FactoryError::new(
            "pr_drift",
            "implementation_publication",
            "observed drift",
            false,
            None,
        );
        assert!(!already_recorded(&terminal, "triage error: observed drift"));
        let empty = FactoryError::new("x", "implementation_publication", "", true, None);
        assert!(!already_recorded(&empty, "triage error: anything"));
    }

    #[test]
    fn automatic_backoff_grows_then_saturates() {
        assert_eq!(automatic_backoff_secs(1), 30);
        assert_eq!(automatic_backoff_secs(2), 60);
        assert_eq!(automatic_backoff_secs(3), 120);
        assert_eq!(
            automatic_backoff_secs(7),
            AUTOMATIC_PUBLICATION_BACKOFF_MAX_SECS
        );
        assert_eq!(
            automatic_backoff_secs(u32::MAX),
            AUTOMATIC_PUBLICATION_BACKOFF_MAX_SECS
        );
    }

    #[test]
    fn automatic_retry_waits_out_backoff() {
        let now = Utc::now();
        // A never-tried intent publishes immediately.
        assert!(automatic_retry_ready(0, now, now));
        // A just-failed intent waits.
        assert!(!automatic_retry_ready(1, now, now));
        assert!(!automatic_retry_ready(
            1,
            now - chrono::Duration::seconds(29),
            now
        ));
        assert!(automatic_retry_ready(
            1,
            now - chrono::Duration::seconds(30),
            now
        ));
        // Later attempts wait proportionally longer.
        assert!(!automatic_retry_ready(
            3,
            now - chrono::Duration::seconds(60),
            now
        ));
        assert!(automatic_retry_ready(
            3,
            now - chrono::Duration::seconds(120),
            now
        ));
    }

    #[tokio::test]
    async fn automatic_publication_blocks_after_exhausting_retries() {
        let root = tempdir().unwrap();
        let workflow = root.path().join("workflow");
        let db = root.path().join("factory.db");
        fs::create_dir_all(&workflow).unwrap();

        let store = SharedFactoryStore::open(&db, 5_000).unwrap();
        let approved = seed_approved(&store, "rev").await;
        let intent = store
            .create_implementation_publication_intent(
                &approved.run_id,
                None,
                ImplementationPublicationKind::Automatic,
                &serde_json::json!({}),
            )
            .unwrap();

        // Burn the retry budget the way repeated reconcile failures would.
        for _ in 0..MAX_AUTOMATIC_PUBLICATION_RETRIES {
            store
                .set_implementation_publication_error(
                    &intent.intent_id,
                    PublicationStatus::Pending,
                    FactoryError::new(
                        "publication_retryable",
                        "implementation_publication",
                        "forge unreachable",
                        true,
                        None,
                    ),
                )
                .unwrap();
        }
        let exhausted = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(exhausted.retry_count, MAX_AUTOMATIC_PUBLICATION_RETRIES);
        assert_eq!(exhausted.status, PublicationStatus::Pending);

        let comments = FakeComments::new("symphony-bot");
        let issues = Arc::new(FakeIssues {
            issue: Mutex::new(base_issue()),
        });
        let mut coordinator = ImplementationCoordinator::with_harness(
            store.clone(),
            comments,
            issues,
            ImplementationCoordinatorConfig {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                owner_instance: "test".into(),
                workflow_dir: workflow,
                storage_path: db,
            },
            FakeHarness {
                repair_touch: false,
                calls: Mutex::new(0),
            },
        );

        coordinator
            .poll_once(&ServiceConfig::default())
            .await
            .unwrap();

        let recovered = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.status, PublicationStatus::Blocked);
        let last_error = recovered.last_error.expect("blocking error recorded");
        assert_eq!(last_error.code, "publication_retry_exhausted");
        assert!(!last_error.retryable);
        assert!(last_error.remediation.contains("forge unreachable"));

        // A blocked intent leaves the pending poll set for good.
        assert!(store
            .list_pending_implementation_publications()
            .unwrap()
            .iter()
            .all(|pending| pending.intent_id != intent.intent_id));

        // ...until an operator recovers it, which is the only way back.
        let blocked = store.list_blocked_implementation_publications().unwrap();
        assert!(blocked
            .iter()
            .any(|entry| entry.intent_id == intent.intent_id));

        let reset = store
            .reset_blocked_implementation_publication(&intent.intent_id, "operator")
            .unwrap();
        assert_eq!(reset.status, PublicationStatus::Pending);
        assert_eq!(reset.retry_count, 0);
        assert!(reset.last_error.is_none());
        // Durable progress survives the reset so publication resumes, not restarts.
        assert_eq!(reset.completed_steps, recovered.completed_steps);

        // The audit event commits with the reset, not after it, and carries the
        // error that was cleared so the timeline explains the intervention.
        let audit = rusqlite::Connection::open(root.path().join("factory.db")).unwrap();
        let (count, payload): (u64, String) = audit
            .query_row(
                "SELECT COUNT(*), COALESCE(MAX(payload_json), '')
                 FROM factory_events
                 WHERE event_type = 'implementation_publication_reset'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(payload.contains("\"operator\":\"operator\""));
        assert!(payload.contains("publication_retry_exhausted"));

        assert!(store
            .list_pending_implementation_publications()
            .unwrap()
            .iter()
            .any(|pending| pending.intent_id == intent.intent_id));
        assert!(store
            .list_blocked_implementation_publications()
            .unwrap()
            .is_empty());

        // Resetting something that is not blocked is refused, not silently applied.
        let err = store
            .reset_blocked_implementation_publication(&intent.intent_id, "operator")
            .unwrap_err();
        assert!(err.to_string().contains("not blocked"));
    }

    #[test]
    fn automatic_completion_route_is_required_before_intent_creation() {
        let service = ServiceConfig::default();
        let err = automatic_completion_state(&service).unwrap_err();
        assert!(err
            .to_string()
            .contains("implementation.completion_route.state required"));
    }
}
