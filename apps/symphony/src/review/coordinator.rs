//! A4 eligibility, read-only dispatch, manifest validation, and preview.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AgentBackend, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::github::client::GithubClient;
use crate::github::projects_v2::ProjectsV2Client;
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
use crate::triage::runtime::SharedFactoryStore;
use crate::triage::store::{
    A4EligibleReviewRun, ClaimAttemptRequest, FactoryRunStore, StoreReviewArtifactRequest,
    StoreReviewAttemptRequest,
};
use crate::triage::runner::TriageIssueIdentity;

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
        let candidates = self.store.list_a4_eligible_review_runs()?;
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
                    && item
                        .repository
                        .as_deref()
                        .map_or(true, |repository| {
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
            match self.process_candidate(service, &candidate, issue_number).await {
                Ok(ProcessOutcome::Completed) => {
                    summary.attempts_started += 1;
                    summary.attempts_completed += 1;
                    summary.preview_published += 1;
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
            return Ok(ProcessOutcome::Waiting);
        }
        let files = self
            .github
            .list_pull_request_files(candidate.pr_number, self.config.max_pages)
            .await?;
        let changed = reviewed_files(&files);
        let diff = files
            .iter()
            .map(|file| {
                format!(
                    "diff --git a/{0} b/{0}\n--- a/{0}\n+++ b/{0}\n{1}",
                    file.filename,
                    file.patch.as_deref().unwrap_or("(binary or patch unavailable)\n")
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
        let stage = self.store.claim_review_attempt(
            ClaimAttemptRequest {
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
            },
        )?;
        let attempt_id = Uuid::now_v7().to_string();
        self.store.store_review_attempt_inputs(StoreReviewAttemptRequest {
            attempt_id: attempt_id.clone(),
            stage_run_id: stage.stage_run_id.clone(),
            draft_pr_artifact_id: candidate.draft_pr_artifact_id.clone(),
            implementation_artifact_id: candidate.implementation_artifact_id.clone(),
            spec_artifact_id: candidate.approved_artifact_id.clone(),
            pr_number: candidate.pr_number,
            reviewed_head_sha: pull.head.sha.clone(),
            base_sha: pull.base.sha.clone(),
        })?;
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
        let workspace_root = PathBuf::from(&service.workspace.root);
        let command = command_for_review(service)?;
        let mut worker_request = ReviewWorkerRequest {
            attempt_id: attempt_id.clone(),
            workspace_root,
            repo_path: PathBuf::from(repo_path),
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
            diff,
            pull_request_body: pull.body.unwrap_or_default(),
            approved_spec: spec.artifact.clone(),
            implementation_manifest: implementation.manifest.clone(),
        };

        let mut last_error = None;
        let mut final_output = None;
        let mut total_usage = StageUsage::default();
        for reprompt in 0..=service.review.max_reprompts {
            if reprompt > 0 {
                worker_request.prompt = format!(
                    "{}\n\nPrevious validation feedback (fix the JSON output only): {}",
                    worker_request.prompt,
                    last_error.as_deref().unwrap_or("manifest validation failed")
                );
            }
            let result = self.worker.run(&worker_request).await?;
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
                        self.store.update_review_attempt(
                            &attempt_id,
                            "running",
                            reprompt,
                            None,
                            None,
                            Some(&serde_json::json!({"accepted":false,"error":error.to_string()})),
                            None,
                        )?;
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
            self.store.update_review_attempt(
                &attempt_id,
                "blocked",
                service.review.max_reprompts,
                None,
                None,
                Some(&serde_json::json!({"accepted":false,"error":message})),
                Some(&factory_error),
            )?;
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
            attempt_id,
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
        let intent = self.store.create_review_publication_intent(
            &candidate.run_id,
            &artifact.artifact_id,
            "preview",
            &serde_json::json!({
                "issue_number": issue_number,
                "pr_number": candidate.pr_number,
                "reviewed_head_sha": pull.head.sha,
                "base_sha": pull.base.sha,
            }),
        )?;
        let publisher = ReviewPublisher::new(self.comments.clone());
        publisher
            .publish_preview(
                &self.store,
                &intent,
                &artifact,
                issue_number,
                self.config.max_pages,
            )
            .await?;
        self.record_event(
            Some(&candidate.run_id),
            Some(&stage.stage_run_id),
            "review_published",
            serde_json::json!({"status":"applied","intent_id":intent.intent_id,"artifact_id":artifact.artifact_id}),
        )?;
        Ok(ProcessOutcome::Completed)
    }

    async fn reconcile_pending_publications(&self, service: &ServiceConfig) -> Result<()> {
        let publisher = ReviewPublisher::new(self.comments.clone());
        for intent in self.store.list_pending_review_publications()? {
            let Some(artifact) = self.store.get_review_artifact(&intent.artifact_id)? else {
                continue;
            };
            let issue_number = intent
                .desired_effects
                .get("issue_number")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    SymphonyError::StorageError(format!(
                        "review intent {} is missing issue_number",
                        intent.intent_id
                    ))
                })?;
            if let Err(error) = publisher
                .publish_preview(
                    &self.store,
                    &intent,
                    &artifact,
                    issue_number,
                    service.triage.max_intake_pages.max(1),
                )
                .await
            {
                self.store.set_review_publication_error(
                    &intent.intent_id,
                    crate::triage::domain::PublicationStatus::Pending,
                    FactoryError::new(
                        "review_publication_retryable",
                        "review_publisher",
                        error.to_string(),
                        true,
                        None,
                    ),
                )?;
            }
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
            events.emit_triage_event(event_type, None, payload);
        }
        Ok(())
    }
}

enum ProcessOutcome {
    Completed,
    Waiting,
    Blocked,
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
