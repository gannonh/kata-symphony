//! A4 eligibility, read-only dispatch, manifest validation, and preview.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AgentBackend, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::github::client::GithubClient;
use crate::github::projects_v2::{ProjectItem, ProjectsV2Client, StatusFieldInfo};
use crate::path_safety::canonicalize;
use crate::review::domain::ReviewConfig;
use crate::review::findings::reviewed_files;
use crate::review::manifest::parse_and_validate_review_manifest;
use crate::review::publisher::ReviewPublisher;
use crate::review::worker::{
    command_for_review, harness_for_service, model_for_review, ReviewWorker, ReviewWorkerRequest,
};
use crate::triage::coordinator::EventEmitter;
use crate::triage::domain::{FactoryError, FactoryEventRecord, StageUsage};
use crate::triage::publisher::TriageCommentPort;
use crate::triage::runner::TriageIssueIdentity;
use crate::triage::runtime::SharedFactoryStore;
use crate::triage::store::{
    A4EligibleReviewRun, ClaimAttemptRequest, FactoryRunStore, StoreReviewArtifactRequest,
    StoreReviewAttemptRequest, UpdateReviewAttemptRequest,
};

#[derive(Debug, Clone)]
pub struct ReviewCoordinatorConfig {
    pub forge_host: String,
    pub repository: String,
    pub owner_instance: String,
    pub workflow_dir: PathBuf,
    pub project_owner: String,
    pub project_number: u64,
    pub max_pages: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewPollSummary {
    pub review_enabled: bool,
    pub candidates_seen: u32,
    pub attempts_started: u32,
    pub attempts_completed: u32,
    pub attempts_failed: u32,
    pub waiting: u32,
    pub preview_published: u32,
    pub automatic_published: u32,
    pub blocked: u32,
}

pub struct ReviewCoordinator<C, W> {
    store: SharedFactoryStore,
    comments: C,
    github: GithubClient,
    projects: ProjectsV2Client,
    worker: W,
    config: ReviewCoordinatorConfig,
    events: Option<Arc<dyn EventEmitter>>,
}

impl<C, W> ReviewCoordinator<C, W>
where
    C: TriageCommentPort + Clone,
    W: ReviewWorker,
{
    pub fn new(
        store: SharedFactoryStore,
        comments: C,
        github: GithubClient,
        projects: ProjectsV2Client,
        worker: W,
        config: ReviewCoordinatorConfig,
    ) -> Self {
        Self {
            store,
            comments,
            github,
            projects,
            worker,
            config,
            events: None,
        }
    }

    pub fn with_events(mut self, events: Arc<dyn EventEmitter>) -> Self {
        self.events = Some(events);
        self
    }

    pub async fn poll_once(&mut self, service: &ServiceConfig) -> Result<ReviewPollSummary> {
        let mut summary = ReviewPollSummary {
            review_enabled: service.review.enabled,
            ..Default::default()
        };
        self.store.interrupt_stale_attempts()?;
        self.reconcile_pending_publications(service).await?;
        if !service.review.enabled {
            return Ok(summary);
        }

        let status_field = self
            .projects
            .resolve_status_field(&self.config.project_owner, self.config.project_number)
            .await?;
        let project_items = self
            .projects
            .query_all_items(&status_field.project_id, self.config.max_pages)
            .await?;
        let trigger = service.review.trigger_state.trim();
        let candidates = self
            .store
            .list_a4_eligible_review_runs(service.review.max_attempts)?;
        summary.candidates_seen = candidates.len() as u32;

        for candidate in candidates {
            let issue_number = match candidate.issue_id.parse::<u64>() {
                Ok(number) => number,
                Err(_) => {
                    summary.waiting += 1;
                    continue;
                }
            };
            let in_trigger_state = project_items.iter().any(|item| {
                item.issue_number == issue_number
                    && item.repository.as_deref().is_some_and(|repository| {
                        repository.eq_ignore_ascii_case(&candidate.repository)
                    })
                    && item
                        .status
                        .as_deref()
                        .is_some_and(|status| status.trim().eq_ignore_ascii_case(trigger))
            });
            if !in_trigger_state {
                summary.waiting += 1;
                continue;
            }
            match self
                .process_candidate(service, &candidate, issue_number)
                .await
            {
                Ok(ProcessOutcome::Completed) => {
                    summary.attempts_started += 1;
                    summary.attempts_completed += 1;
                    if service.review.mode == crate::review::domain::ReviewMode::Automatic {
                        summary.automatic_published += 1;
                    } else {
                        summary.preview_published += 1;
                    }
                }
                Ok(ProcessOutcome::Waiting) => summary.waiting += 1,
                Ok(ProcessOutcome::Blocked) => {
                    summary.attempts_started += 1;
                    summary.attempts_failed += 1;
                    summary.blocked += 1;
                }
                Err(error) => {
                    summary.attempts_failed += 1;
                    tracing::warn!(
                        event = "review_candidate_failed",
                        run_id = %candidate.run_id,
                        error = %error,
                        "review candidate processing failed"
                    );
                }
            }
        }
        Ok(summary)
    }

    async fn process_candidate(
        &mut self,
        service: &ServiceConfig,
        candidate: &A4EligibleReviewRun,
        issue_number: u64,
    ) -> Result<ProcessOutcome> {
        let pull = self.github.get_pull_request(candidate.pr_number).await?;
        if !pull.state.eq_ignore_ascii_case("open") {
            return Ok(ProcessOutcome::Waiting);
        }
        let failed_attempts = self
            .store
            .count_review_attempt_failures_for_head(&candidate.run_id, &pull.head.sha)?;
        if failed_attempts >= service.review.max_attempts.max(1) {
            return Ok(ProcessOutcome::Waiting);
        }
        if self
            .store
            .review_artifact_exists_for_head(&candidate.run_id, &pull.head.sha)?
        {
            if let Some(artifact) = self
                .store
                .get_orphaned_review_artifact_for_head(&candidate.run_id, &pull.head.sha)?
            {
                let spec = self
                    .store
                    .get_spec_artifact(&candidate.approved_artifact_id)?
                    .ok_or_else(|| {
                        SymphonyError::StorageError(format!(
                            "approved spec artifact {} is missing",
                            candidate.approved_artifact_id
                        ))
                    })?;
                let kind = service.review.mode.as_str();
                let route_state = review_route_state(&artifact, service);
                self.store.create_review_publication_intent(
                    &candidate.run_id,
                    &artifact.artifact_id,
                    kind,
                    &review_publication_effects(
                        issue_number,
                        candidate,
                        &pull,
                        service,
                        route_state,
                        spec.version,
                    ),
                )?;
                self.reconcile_pending_publications(service).await?;
                let intent = self
                    .store
                    .list_review_publications_for_run(&candidate.run_id)?
                    .into_iter()
                    .find(|intent| intent.artifact_id == artifact.artifact_id)
                    .ok_or_else(|| {
                        SymphonyError::StorageError(format!(
                            "recreated review intent for artifact {} is missing",
                            artifact.artifact_id
                        ))
                    })?;
                return Ok(
                    if intent.status == crate::triage::domain::PublicationStatus::Applied {
                        ProcessOutcome::Completed
                    } else {
                        ProcessOutcome::Waiting
                    },
                );
            }
            // An artifact with an intent is already complete or pending durable
            // publication. Reconciliation owns pending intents.
            return Ok(ProcessOutcome::Waiting);
        }
        if pull.head.sha != candidate.head_sha {
            self.record_event(
                Some(&candidate.run_id),
                None,
                "review_cycle_reopened",
                serde_json::json!({
                    "status": "waiting",
                    "reason": "head_sha_changed",
                    "expected_head_sha": candidate.head_sha,
                    "observed_head_sha": pull.head.sha,
                }),
            )?;
        }
        let files = self
            .github
            .list_pull_request_files(candidate.pr_number, self.config.max_pages)
            .await?;
        let refreshed_pull = self.github.get_pull_request(candidate.pr_number).await?;
        if pull_revision_changed(&pull, &refreshed_pull) {
            self.record_event(
                Some(&candidate.run_id),
                None,
                "review_cycle_reopened",
                serde_json::json!({
                    "status": "waiting",
                    "reason": "head_or_base_sha_changed_during_file_retrieval",
                    "expected_head_sha": pull.head.sha,
                    "observed_head_sha": refreshed_pull.head.sha,
                    "expected_base_sha": pull.base.sha,
                    "observed_base_sha": refreshed_pull.base.sha,
                }),
            )?;
            return Ok(ProcessOutcome::Waiting);
        }
        let changed = reviewed_files(&files);
        let diff = files
            .iter()
            .map(|file| {
                format!(
                    "diff --git a/{0} b/{0}\n--- a/{0}\n+++ b/{0}\n{1}",
                    file.filename,
                    file.patch
                        .as_deref()
                        .unwrap_or("(binary or patch unavailable)\n")
                )
            })
            .collect::<String>();
        let spec = self
            .store
            .get_spec_artifact(&candidate.approved_artifact_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "approved spec artifact {} is missing",
                    candidate.approved_artifact_id
                ))
            })?;
        let implementation = self
            .store
            .get_implementation_artifact(&candidate.implementation_artifact_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "implementation artifact {} is missing",
                    candidate.implementation_artifact_id
                ))
            })?;
        let configuration_revision = review_configuration_revision(&service.review);
        let stage = self.store.claim_review_attempt(ClaimAttemptRequest {
            forge_host: candidate.forge_host.clone(),
            repository: candidate.repository.clone(),
            issue_id: candidate.issue_id.clone(),
            issue_identifier: candidate.issue_identifier.clone(),
            issue_revision: pull.head.sha.clone(),
            configuration_revision: configuration_revision.clone(),
            owner_instance: self.config.owner_instance.clone(),
            harness: match service.agent_backend {
                AgentBackend::Codex => "codex".to_string(),
                AgentBackend::KataCli => "pi".to_string(),
            },
            model: service.review.model.clone(),
            workspace_path: None,
            output_path: None,
            pid: None,
            process_group_id: None,
            process_start_token: None,
            executable_identity: None,
        })?;
        let attempt_id = Uuid::now_v7().to_string();
        let attempt_inputs = StoreReviewAttemptRequest {
            attempt_id: attempt_id.clone(),
            stage_run_id: stage.stage_run_id.clone(),
            draft_pr_artifact_id: candidate.draft_pr_artifact_id.clone(),
            implementation_artifact_id: candidate.implementation_artifact_id.clone(),
            spec_artifact_id: candidate.approved_artifact_id.clone(),
            pr_number: candidate.pr_number,
            reviewed_head_sha: pull.head.sha.clone(),
            base_sha: pull.base.sha.clone(),
        };
        if let Err(error) = self.store.store_review_attempt_inputs(attempt_inputs) {
            let factory_error = FactoryError::new(
                "review_attempt_input_persist_failed",
                "review_coordinator",
                error.to_string(),
                true,
                None,
            );
            let _ = self.store.fail_attempt(&stage.stage_run_id, factory_error);
            return Err(error);
        }
        let result: Result<ProcessOutcome> = async {
            self.record_event(
            Some(&candidate.run_id),
            Some(&stage.stage_run_id),
            "review_started",
            serde_json::json!({
                "status": "running",
                "pr_number": candidate.pr_number,
                "reviewed_head_sha": pull.head.sha,
                "base_sha": pull.base.sha,
            }),
            )?;

        let prompt = load_review_prompt(&self.config.workflow_dir, service)?;
        let repo_path = service.workspace.repo.as_deref().ok_or_else(|| {
            SymphonyError::InvalidWorkflowConfig(
                "workspace.repo is required when review is enabled".to_string(),
            )
        })?;
        let repo_path = resolve_review_path(repo_path, "workspace.repo")?;
        let workspace_root = resolve_review_path(&service.workspace.root, "workspace.root")?;
        let command = command_for_review(service)?;
        let mut worker_request = ReviewWorkerRequest {
            attempt_id: attempt_id.clone(),
            workspace_root,
            repo_path,
            command,
            prompt,
            config: service.review.clone(),
            model: model_for_review(service),
            harness: harness_for_service(service),
            issue: TriageIssueIdentity {
                id: candidate.issue_id.clone(),
                identifier: candidate.issue_identifier.clone(),
                title: pull.title.clone(),
            },
            codex: (service.agent_backend == AgentBackend::Codex).then(|| service.codex.clone()),
            diff: diff.clone(),
            pull_request_body: pull.body.clone().unwrap_or_default(),
            approved_spec: spec.artifact.clone(),
            implementation_manifest: implementation.manifest.clone(),
        };

        let mut last_error = None;
        let mut final_output = None;
        let mut total_usage = StageUsage::default();
        let base_prompt = worker_request.prompt.clone();
        for reprompt in 0..=service.review.max_reprompts {
            if reprompt > 0 {
                worker_request.prompt = format!(
                    "{}\n\nPrevious validation feedback (fix the JSON output only): {}",
                    base_prompt,
                    last_error.as_deref().unwrap_or("manifest validation failed")
                );
            }
            let result = match self.worker.run(&worker_request).await {
                Ok(result) => result,
                Err(error) => {
                    last_error = Some(format!("worker invocation failed: {error}"));
                    self.store.update_review_attempt(UpdateReviewAttemptRequest {
                        attempt_id: &attempt_id,
                        status: "running",
                        reprompt_count: reprompt + 1,
                        worker_turn: None,
                        manifest: None,
                        validation_result: Some(&serde_json::json!({
                            "accepted": false,
                            "error": last_error.as_deref().unwrap_or_default()
                        })),
                        error: None,
                    })?;
                    continue;
                }
            };
            let completed_pull = self.github.get_pull_request(candidate.pr_number).await?;
            if pull_revision_changed(&pull, &completed_pull) {
                self.record_event(
                    Some(&candidate.run_id),
                    Some(&stage.stage_run_id),
                    "review_cycle_reopened",
                    serde_json::json!({
                        "status": "waiting",
                        "reason": "head_or_base_sha_changed_after_worker",
                        "expected_head_sha": pull.head.sha,
                        "observed_head_sha": completed_pull.head.sha,
                        "expected_base_sha": pull.base.sha,
                        "observed_base_sha": completed_pull.base.sha,
                    }),
                )?;
                let reopened_error = FactoryError::new(
                    "review_cycle_reopened",
                    "review_coordinator",
                    "review cycle reopened while worker was running",
                    true,
                    None,
                );
                self.store.update_review_attempt(UpdateReviewAttemptRequest {
                    attempt_id: &attempt_id,
                    status: "interrupted",
                    reprompt_count: reprompt,
                    worker_turn: None,
                    manifest: None,
                    validation_result: Some(&serde_json::json!({
                        "accepted": false,
                        "status": "waiting",
                        "reason": "head_or_base_sha_changed_after_worker",
                        "expected_head_sha": pull.head.sha,
                        "observed_head_sha": completed_pull.head.sha,
                        "expected_base_sha": pull.base.sha,
                        "observed_base_sha": completed_pull.base.sha,
                    })),
                    error: Some(&reopened_error),
                })?;
                let _ = self
                    .store
                    .interrupt_review_attempt(&stage.stage_run_id, &self.config.owner_instance)?;
                return Ok(ProcessOutcome::Waiting);
            }
            total_usage.input_tokens = total_usage
                .input_tokens
                .saturating_add(result.usage.input_tokens);
            total_usage.output_tokens = total_usage
                .output_tokens
                .saturating_add(result.usage.output_tokens);
            total_usage.total_tokens = total_usage
                .total_tokens
                .saturating_add(result.usage.total_tokens);
            match String::from_utf8(result.output_bytes) {
                Ok(raw) => match parse_and_validate_review_manifest(
                    &raw,
                    &pull.head.sha,
                    &pull.base.sha,
                    &changed,
                    service.review.max_findings,
                ) {
                    Ok(manifest) => {
                        final_output = Some(manifest);
                        break;
                    }
                    Err(error) => {
                        last_error = Some(error.to_string());
                        self.store.update_review_attempt(UpdateReviewAttemptRequest {
                            attempt_id: &attempt_id,
                            status: "running",
                            reprompt_count: reprompt,
                            worker_turn: None,
                            manifest: None,
                            validation_result: Some(
                                &serde_json::json!({"accepted":false,"error":error.to_string()}),
                            ),
                            error: None,
                        })?;
                    }
                },
                Err(error) => {
                    last_error = Some(format!("worker output was not UTF-8: {error}"));
                }
            }
        }
        let Some(manifest) = final_output else {
            let message = last_error.unwrap_or_else(|| "review manifest was not produced".to_string());
            let factory_error = FactoryError::new(
                "review_manifest_invalid",
                "review_coordinator",
                message.clone(),
                false,
                None,
            );
            self.store.update_review_attempt(UpdateReviewAttemptRequest {
                attempt_id: &attempt_id,
                status: "blocked",
                reprompt_count: service.review.max_reprompts,
                worker_turn: None,
                manifest: None,
                validation_result: Some(&serde_json::json!({"accepted":false,"error":message})),
                error: Some(&factory_error),
            })?;
            self.store.fail_attempt(&stage.stage_run_id, factory_error.clone())?;
            self.record_event(
                Some(&candidate.run_id),
                Some(&stage.stage_run_id),
                "review_blocked",
                serde_json::json!({"status":"blocked","error":factory_error}),
            )?;
            return Ok(ProcessOutcome::Blocked);
        };

        let artifact = self.store.store_review_artifact(StoreReviewArtifactRequest {
            stage_run_id: stage.stage_run_id.clone(),
            attempt_id: attempt_id.clone(),
            draft_pr_artifact_id: candidate.draft_pr_artifact_id.clone(),
            implementation_artifact_id: candidate.implementation_artifact_id.clone(),
            spec_artifact_id: candidate.approved_artifact_id.clone(),
            reviewed_head_sha: pull.head.sha.clone(),
            base_sha: pull.base.sha.clone(),
            bytes_len: serde_json::to_vec(&manifest)
                .map_err(|error| SymphonyError::StorageError(error.to_string()))?
                .len() as u64,
            manifest,
            usage: total_usage,
        })?;
        self.record_event(
            Some(&candidate.run_id),
            Some(&stage.stage_run_id),
            "review_findings_recorded",
            serde_json::json!({
                "status":"completed",
                "artifact_id": artifact.artifact_id,
                "finding_count": artifact.finding_count,
            }),
        )?;
        let kind = service.review.mode.as_str();
        let route_state = if artifact
            .manifest
            .findings
            .iter()
            .any(|finding| finding.severity <= service.review.blocking_severity)
        {
            service
                .review
                .changes_requested_route
                .as_ref()
                .map(|route| route.state.trim().to_string())
        } else {
            service
                .review
                .completion_route
                .as_ref()
                .map(|route| route.state.trim().to_string())
        };
        let intent = self.store.create_review_publication_intent(
            &candidate.run_id,
            &artifact.artifact_id,
            kind,
            &serde_json::json!({
                "issue_number": issue_number,
                "repository": candidate.repository,
                "pr_number": candidate.pr_number,
                "reviewed_head_sha": pull.head.sha,
                "base_sha": pull.base.sha,
                "trigger_state": service.review.trigger_state,
                "route_state": route_state,
                "approved_spec_version": spec.version,
            }),
        )?;
        let publisher = ReviewPublisher::new(self.comments.clone());
        if service.review.mode == crate::review::domain::ReviewMode::Preview {
            if let Err(error) = publisher
                .publish_preview(
                    &self.store,
                    &intent,
                    &artifact,
                    issue_number,
                    self.config.max_pages,
                )
                .await
            {
                let classified = classify_review_publication_error(&error);
                let status = self.record_publication_failure(
                    &intent,
                    classified.clone(),
                    service.review.max_attempts,
                )?;
                if matches!(
                    status,
                    crate::triage::domain::PublicationStatus::Conflict
                        | crate::triage::domain::PublicationStatus::Blocked
                ) {
                    self.record_event(
                        Some(&candidate.run_id),
                        Some(&stage.stage_run_id),
                        "review_blocked",
                        serde_json::json!({
                            "status": status.as_str(),
                            "classification": status.as_str(),
                            "intent_id": intent.intent_id,
                            "artifact_id": artifact.artifact_id,
                            "error": classified,
                        }),
                    )?;
                    return Ok(ProcessOutcome::Blocked);
                }
                return Err(error);
            }
        } else {
            if let Err(error) = publisher
                .publish_formal(
                    &self.store,
                    &self.github,
                    &intent,
                    &artifact,
                    candidate.pr_number,
                    self.config.max_pages,
                )
                .await
            {
                let classified = classify_review_publication_error(&error);
                let status = self.record_publication_failure(
                    &intent,
                    classified.clone(),
                    service.review.max_attempts,
                )?;
                if matches!(
                    status,
                    crate::triage::domain::PublicationStatus::Conflict
                        | crate::triage::domain::PublicationStatus::Blocked
                ) {
                    self.record_event(
                        Some(&candidate.run_id),
                        Some(&stage.stage_run_id),
                        "review_blocked",
                        serde_json::json!({
                            "status": status.as_str(),
                            "classification": status.as_str(),
                            "intent_id": intent.intent_id,
                            "artifact_id": artifact.artifact_id,
                            "error": classified,
                        }),
                    )?;
                    return Ok(ProcessOutcome::Blocked);
                }
                return Err(error);
            }
            let pending = self
                .store
                .get_review_publication_intent(&intent.intent_id)?
                .ok_or_else(|| {
                    SymphonyError::StorageError(format!(
                        "review intent {} disappeared after publication",
                        intent.intent_id
                    ))
                })?;
            self.apply_automatic_route(service, &pending, issue_number).await?;
            let routed = self
                .store
                .get_review_publication_intent(&intent.intent_id)?
                .ok_or_else(|| {
                    SymphonyError::StorageError(format!(
                        "review intent {} disappeared after routing",
                        intent.intent_id
                    ))
                })?;
            if !routed.completed_steps.iter().any(|step| step == "route_applied") {
                return Ok(ProcessOutcome::Waiting);
            }
            self.store.record_review_publication_step(
                &routed.intent_id,
                "comment_final",
                crate::triage::domain::PublicationStatus::Pending,
                &serde_json::json!({
                    "review_id": routed.review_id,
                    "review_url": routed.review_url,
                    "route_state": routed.route_state,
                }),
            )?;
        }
        let final_intent = self
            .store
            .get_review_publication_intent(&intent.intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "review intent {} disappeared before completion",
                    intent.intent_id
                ))
            })?;
        if final_intent.status == crate::triage::domain::PublicationStatus::Applied {
            self.record_event(
                Some(&candidate.run_id),
                Some(&stage.stage_run_id),
                "review_published",
                serde_json::json!({"status":"applied","intent_id":intent.intent_id,"artifact_id":artifact.artifact_id}),
            )?;
            Ok(ProcessOutcome::Completed)
        } else {
            Ok(ProcessOutcome::Waiting)
        }
        }
        .await;

        if let Err(error) = &result {
            let cycle_reopened = error
                .to_string()
                .contains("review cycle reopened while worker was running");
            let factory_error = FactoryError::new(
                if cycle_reopened {
                    "review_cycle_reopened"
                } else {
                    "review_attempt_failed"
                },
                "review_coordinator",
                error.to_string(),
                true,
                None,
            );
            if let Err(cleanup_error) =
                self.store
                    .update_review_attempt(UpdateReviewAttemptRequest {
                        attempt_id: &attempt_id,
                        status: "failed",
                        reprompt_count: service.review.max_reprompts,
                        worker_turn: None,
                        manifest: None,
                        validation_result: Some(&serde_json::json!({
                            "accepted": false,
                            "error": error.to_string()
                        })),
                        error: Some(&factory_error),
                    })
            {
                tracing::error!(
                    event = "review_attempt_cleanup_failed",
                    attempt_id = %attempt_id,
                    error = %cleanup_error,
                    "could not persist failed review attempt"
                );
            }
            if let Err(cleanup_error) = self.store.fail_attempt(&stage.stage_run_id, factory_error)
            {
                tracing::error!(
                    event = "review_stage_cleanup_failed",
                    stage_run_id = %stage.stage_run_id,
                    error = %cleanup_error,
                    "could not terminate failed review stage"
                );
            }
        }
        result
    }

    async fn reconcile_pending_publications(&self, service: &ServiceConfig) -> Result<()> {
        let publisher = ReviewPublisher::new(self.comments.clone());
        let pending = self.store.list_pending_review_publications()?;
        let has_automatic = pending.iter().any(|intent| intent.kind == "automatic");
        let project_data = if has_automatic {
            let field = self
                .projects
                .resolve_status_field(&self.config.project_owner, self.config.project_number)
                .await?;
            let items = self
                .projects
                .query_all_items(&field.project_id, self.config.max_pages)
                .await?;
            Some((field, items))
        } else {
            None
        };

        for intent in pending {
            let Some(artifact) = self.store.get_review_artifact(&intent.artifact_id)? else {
                let error = FactoryError::new(
                    "review_publication_missing_artifact",
                    "review_publisher",
                    format!("review artifact {} is missing", intent.artifact_id),
                    false,
                    None,
                );
                self.record_publication_failure(
                    &intent,
                    error.clone(),
                    service.review.max_attempts,
                )?;
                self.record_reconciliation_blocked(&intent, &error)?;
                continue;
            };
            let Some(issue_number) = intent
                .desired_effects
                .get("issue_number")
                .and_then(serde_json::Value::as_u64)
            else {
                let error = FactoryError::new(
                    "review_publication_missing_issue_number",
                    "review_publisher",
                    format!("review intent {} is missing issue_number", intent.intent_id),
                    false,
                    None,
                );
                self.record_publication_failure(
                    &intent,
                    error.clone(),
                    service.review.max_attempts,
                )?;
                self.record_reconciliation_blocked(&intent, &error)?;
                continue;
            };

            let result = if intent.kind == "automatic" {
                let publisher_result = async {
                    let pr_number = intent
                        .desired_effects
                        .get("pr_number")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(issue_number);
                    let expected_head = intent
                        .desired_effects
                        .get("reviewed_head_sha")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    let pull = self.github.get_pull_request(pr_number).await?;
                    if pull.head.sha != expected_head {
                        self.record_event(
                            Some(&intent.run_id),
                            None,
                            "review_cycle_reopened",
                            serde_json::json!({
                                "status": "waiting",
                                "reason": "publication_head_sha_changed",
                                "intent_id": intent.intent_id,
                                "expected_head_sha": expected_head,
                                "observed_head_sha": pull.head.sha,
                            }),
                        )?;
                        return Ok(ReviewPublicationResult::Waiting);
                    }
                    publisher
                        .publish_formal(
                            &self.store,
                            &self.github,
                            &intent,
                            &artifact,
                            pr_number,
                            self.config.max_pages,
                        )
                        .await
                        .map(|()| ReviewPublicationResult::Published)
                }
                .await;
                match publisher_result {
                    Ok(ReviewPublicationResult::Waiting) => Ok(ReviewPublicationResult::Waiting),
                    Err(error) => Err(error),
                    Ok(ReviewPublicationResult::Published) => self
                        .apply_automatic_route_with_data(
                            service,
                            &intent,
                            issue_number,
                            project_data.as_ref().expect("automatic project data"),
                        )
                        .await
                        .and_then(|applied| {
                            if !applied {
                                return Ok(ReviewPublicationResult::Published);
                            }
                            let latest = self
                                .store
                                .get_review_publication_intent(&intent.intent_id)?
                                .ok_or_else(|| {
                                    SymphonyError::StorageError(format!(
                                        "review intent {} disappeared during reconciliation",
                                        intent.intent_id
                                    ))
                                })?;
                            if latest
                                .completed_steps
                                .iter()
                                .any(|step| step == "comment_final")
                            {
                                return Ok(ReviewPublicationResult::Published);
                            }
                            self.store.record_review_publication_step(
                                &intent.intent_id,
                                "comment_final",
                                crate::triage::domain::PublicationStatus::Pending,
                                &serde_json::json!({
                                    "review_id": latest.review_id,
                                    "review_url": latest.review_url,
                                    "route_state": latest.route_state,
                                }),
                            )?;
                            Ok(ReviewPublicationResult::Published)
                        }),
                }
            } else {
                publisher
                    .publish_preview(
                        &self.store,
                        &intent,
                        &artifact,
                        issue_number,
                        self.config.max_pages,
                    )
                    .await
                    .map(|()| ReviewPublicationResult::Published)
            };
            match result {
                Ok(ReviewPublicationResult::Waiting) => {}
                Ok(ReviewPublicationResult::Published) => {
                    let latest = self
                        .store
                        .get_review_publication_intent(&intent.intent_id)?
                        .ok_or_else(|| {
                            SymphonyError::StorageError(format!(
                                "review intent {} disappeared during reconciliation",
                                intent.intent_id
                            ))
                        })?;
                    if latest.status == crate::triage::domain::PublicationStatus::Applied {
                        self.record_event(
                            Some(&intent.run_id),
                            None,
                            "review_published",
                            serde_json::json!({
                                "status": "applied",
                                "intent_id": intent.intent_id,
                                "artifact_id": intent.artifact_id,
                                "reconciled": true,
                            }),
                        )?;
                    }
                }
                Err(error) => {
                    let classified = classify_review_publication_error(&error);
                    self.record_publication_failure(
                        &intent,
                        classified.clone(),
                        service.review.max_attempts,
                    )?;
                    let latest = self
                        .store
                        .get_review_publication_intent(&intent.intent_id)?
                        .ok_or_else(|| {
                            SymphonyError::StorageError(format!(
                                "review intent {} disappeared after publication failure",
                                intent.intent_id
                            ))
                        })?;
                    if matches!(
                        latest.status,
                        crate::triage::domain::PublicationStatus::Conflict
                            | crate::triage::domain::PublicationStatus::Blocked
                    ) {
                        self.record_reconciliation_blocked(&intent, &classified)?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn apply_automatic_route(
        &self,
        service: &ServiceConfig,
        intent: &crate::review::domain::ReviewPublicationIntent,
        issue_number: u64,
    ) -> Result<bool> {
        let field = self
            .projects
            .resolve_status_field(&self.config.project_owner, self.config.project_number)
            .await?;
        let items = self
            .projects
            .query_all_items(&field.project_id, self.config.max_pages)
            .await?;
        self.apply_automatic_route_with_data(service, intent, issue_number, &(field, items))
            .await
    }

    async fn apply_automatic_route_with_data(
        &self,
        _service: &ServiceConfig,
        intent: &crate::review::domain::ReviewPublicationIntent,
        issue_number: u64,
        project_data: &(StatusFieldInfo, Vec<ProjectItem>),
    ) -> Result<bool> {
        if intent
            .completed_steps
            .iter()
            .any(|step| step == "route_applied")
        {
            return Ok(true);
        }
        let route_state = intent
            .desired_effects
            .get("route_state")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|state| !state.is_empty())
            .ok_or_else(|| {
                SymphonyError::InvalidWorkflowConfig(
                    "automatic review intent has no route_state".to_string(),
                )
            })?;
        let field = &project_data.0;
        let target = field
            .options
            .iter()
            .find(|option| option.name.trim().eq_ignore_ascii_case(route_state))
            .ok_or_else(|| {
                SymphonyError::GithubProjectsV2Error(format!(
                    "review route state '{route_state}' is not a Projects v2 option"
                ))
            })?;
        let repository = intent
            .desired_effects
            .get("repository")
            .and_then(serde_json::Value::as_str);
        let item = project_data.1.iter().find(|item| {
            item.issue_number == issue_number
                && item.repository.as_deref().is_some_and(|repo| {
                    repository.is_some_and(|expected| repo.eq_ignore_ascii_case(expected))
                })
        });
        let Some(item) = item else {
            return Ok(false);
        };
        let current = item.status.as_deref().map(str::trim).unwrap_or("");
        if current.eq_ignore_ascii_case(route_state) {
            self.store
                .set_review_publication_route_state(&intent.intent_id, route_state)?;
            self.store.record_review_publication_step(
                &intent.intent_id,
                "route_applied",
                crate::triage::domain::PublicationStatus::Pending,
                &serde_json::json!({"route_state": route_state, "already_applied": true}),
            )?;
            return Ok(true);
        }
        let trigger = intent
            .desired_effects
            .get("trigger_state")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if !current.eq_ignore_ascii_case(trigger) {
            // A human moved the item. Preserve the pending intent and retry after it returns
            // to the trigger state instead of overwriting the unexpected state.
            return Ok(false);
        }
        self.projects
            .update_item_status(
                &field.project_id,
                &item.item_id,
                &field.field_id,
                &target.id,
            )
            .await?;
        let updated_items = self
            .projects
            .query_all_items(&field.project_id, self.config.max_pages)
            .await?;
        let updated = updated_items.iter().find(|updated_item| {
            updated_item.issue_number == issue_number
                && updated_item.repository.as_deref().is_some_and(|repo| {
                    repository.is_some_and(|expected| repo.eq_ignore_ascii_case(expected))
                })
        });
        let verified = updated
            .and_then(|updated_item| updated_item.status.as_deref())
            .is_some_and(|status| status.trim().eq_ignore_ascii_case(route_state));
        if !verified {
            return Err(SymphonyError::GithubProjectsV2Error(format!(
                "Projects v2 route update for issue {issue_number} did not reach '{route_state}'"
            )));
        }
        self.store
            .set_review_publication_route_state(&intent.intent_id, route_state)?;
        self.store.record_review_publication_step(
            &intent.intent_id,
            "route_applied",
            crate::triage::domain::PublicationStatus::Pending,
            &serde_json::json!({"route_state": route_state, "option_id": target.id}),
        )?;
        Ok(true)
    }

    fn record_publication_failure(
        &self,
        intent: &crate::review::domain::ReviewPublicationIntent,
        error: FactoryError,
        max_attempts: u32,
    ) -> Result<crate::triage::domain::PublicationStatus> {
        let next = intent.retry_count.saturating_add(1);
        let status = if error.code == "review_publication_conflict" {
            crate::triage::domain::PublicationStatus::Conflict
        } else if !error.retryable || next >= max_attempts.max(1) {
            crate::triage::domain::PublicationStatus::Blocked
        } else {
            crate::triage::domain::PublicationStatus::Pending
        };
        self.store
            .set_review_publication_error(&intent.intent_id, status, error)?;
        Ok(status)
    }

    fn record_reconciliation_blocked(
        &self,
        intent: &crate::review::domain::ReviewPublicationIntent,
        error: &FactoryError,
    ) -> Result<()> {
        let latest = self
            .store
            .get_review_publication_intent(&intent.intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "review intent {} disappeared while recording blocked outcome",
                    intent.intent_id
                ))
            })?;
        if matches!(
            latest.status,
            crate::triage::domain::PublicationStatus::Conflict
                | crate::triage::domain::PublicationStatus::Blocked
        ) {
            self.record_event(
                Some(&intent.run_id),
                None,
                "review_blocked",
                serde_json::json!({
                    "status": latest.status.as_str(),
                    "classification": latest.status.as_str(),
                    "intent_id": intent.intent_id,
                    "artifact_id": intent.artifact_id,
                    "error": error,
                    "reconciled": true,
                }),
            )?;
        }
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
            events.emit_triage_event(event_type, None, run_id, stage_run_id, payload);
        }
        Ok(())
    }
}

fn classify_review_publication_error(error: &SymphonyError) -> FactoryError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    let conflict = [
        "marker",
        "foreign",
        "owned by another",
        "head does not match",
        "drift",
        "conflict",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if conflict {
        FactoryError::new(
            "review_publication_conflict",
            "review_publisher",
            message,
            false,
            None,
        )
    } else {
        FactoryError::new(
            "review_publication_retryable",
            "review_publisher",
            message,
            true,
            None,
        )
    }
}

fn pull_revision_changed(
    before: &crate::github::client::GithubPullRequest,
    after: &crate::github::client::GithubPullRequest,
) -> bool {
    before.head.sha != after.head.sha || before.base.sha != after.base.sha
}

fn review_route_state(
    artifact: &crate::review::domain::ReviewFindingsArtifactRecord,
    service: &ServiceConfig,
) -> Option<String> {
    let route = if artifact
        .manifest
        .findings
        .iter()
        .any(|finding| finding.severity <= service.review.blocking_severity)
    {
        service.review.changes_requested_route.as_ref()
    } else {
        service.review.completion_route.as_ref()
    };
    route.map(|route| route.state.trim().to_string())
}

fn review_publication_effects(
    issue_number: u64,
    candidate: &A4EligibleReviewRun,
    pull: &crate::github::client::GithubPullRequest,
    service: &ServiceConfig,
    route_state: Option<String>,
    approved_spec_version: u32,
) -> serde_json::Value {
    serde_json::json!({
        "issue_number": issue_number,
        "repository": candidate.repository,
        "pr_number": candidate.pr_number,
        "reviewed_head_sha": pull.head.sha,
        "base_sha": pull.base.sha,
        "trigger_state": service.review.trigger_state,
        "route_state": route_state,
        "approved_spec_version": approved_spec_version,
    })
}

fn resolve_review_path(value: &str, field: &str) -> Result<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(format!(
            "{field} cannot be empty when review is enabled"
        )));
    }
    canonicalize(Path::new(trimmed)).map_err(|error| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "failed to resolve {field} '{trimmed}' for the review stage: {error}"
        ))
    })
}

enum ProcessOutcome {
    Completed,
    Waiting,
    Blocked,
}

enum ReviewPublicationResult {
    Published,
    Waiting,
}

#[cfg(test)]
mod tests {
    use super::{pull_revision_changed, resolve_review_path};
    use crate::github::client::{GithubPullRequest, GithubPullRequestRef};

    fn pull(head: &str, base: &str) -> GithubPullRequest {
        GithubPullRequest {
            number: 1,
            html_url: String::new(),
            draft: false,
            state: "open".to_string(),
            title: String::new(),
            head: GithubPullRequestRef {
                ref_name: "head".to_string(),
                sha: head.to_string(),
            },
            base: GithubPullRequestRef {
                ref_name: "base".to_string(),
                sha: base.to_string(),
            },
            user: None,
            body: None,
        }
    }

    #[test]
    fn review_relative_paths_are_resolved_before_worker_setup() {
        assert!(resolve_review_path(".", "workspace.repo")
            .unwrap()
            .is_absolute());
    }

    #[test]
    fn file_retrieval_revision_guard_catches_head_or_base_changes() {
        let initial = pull("head-1", "base-1");
        assert!(!pull_revision_changed(&initial, &pull("head-1", "base-1")));
        assert!(pull_revision_changed(&initial, &pull("head-2", "base-1")));
        assert!(pull_revision_changed(&initial, &pull("head-1", "base-2")));
    }

    #[test]
    fn live_head_after_candidate_reopen_is_a_valid_revision_baseline() {
        let candidate_head = "head-1";
        let live = pull("head-2", "base-1");
        assert_ne!(candidate_head, live.head.sha);
        assert!(!pull_revision_changed(&live, &pull("head-2", "base-1")));
    }
}

fn load_review_prompt(workflow_dir: &std::path::Path, service: &ServiceConfig) -> Result<String> {
    let path = workflow_dir.join(&service.review.prompt);
    fs::read_to_string(&path).map_err(|error| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "review prompt {} could not be read: {error}",
            path.display()
        ))
    })
}

fn review_configuration_revision(config: &ReviewConfig) -> String {
    let encoded = serde_json::to_vec(config).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(encoded);
    hex::encode(digest.finalize())
}
