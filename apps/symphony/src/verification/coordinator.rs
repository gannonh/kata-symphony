//! A5 verification coordinator: exact-head command execution, evidence
//! capture, read-only verifier invocation, deterministic gate, and the
//! single owned preview comment.

use std::path::{Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AgentBackend, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::github::auth::resolve_github_token;
use crate::github::client::GithubClient;
use crate::github::projects_v2::ProjectsV2Client;
use crate::implementation::bundle::artifacts_dir;
use crate::triage::domain::{FactoryError, StageUsage};
use crate::triage::publisher::TriageCommentPort;
use crate::triage::runtime::SharedFactoryStore;
use crate::triage::store::{ClaimAttemptRequest, FactoryRunStore};
use crate::verification::domain::{
    VerificationCommandConfig, VerificationConfig, VerificationGateRecord,
};
use crate::verification::evidence::collect_evidence;
use crate::verification::executor::{
    cleanup_stopped_verification_containers, command_sha256, execute_command, CommandExecutionRequest,
    CommandRunFailure, LaunchIdentity,
};
use crate::verification::gate::{compute_gate, GateIdentity};
use crate::verification::publisher::publish_preview_comment;
use crate::verification::worker::{
    command_for_verification, harness_for_service, model_for_verification, VerificationWorker,
    VerificationWorkerRequest,
};
use crate::verification::workspace::{
    attempt_root_for_cleanup, fetch_pull_head_verified, prepare_verification_workspace,
    verify_workspace_unchanged, VerificationWorkspace,
};

#[derive(Debug, Clone)]
pub struct VerificationCoordinatorConfig {
    pub forge_host: String,
    pub repository: String,
    pub owner_instance: String,
    pub workflow_dir: PathBuf,
    pub project_owner: String,
    pub project_number: u64,
    pub max_pages: u32,
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerificationPollSummary {
    pub verification_enabled: bool,
    pub candidates_seen: u32,
    pub attempts_started: u32,
    pub attempts_completed: u32,
    pub attempts_failed: u32,
    pub waiting: u32,
    pub recovered: u32,
    pub preview_published: u32,
}

pub struct VerificationCoordinator<C, W> {
    store: SharedFactoryStore,
    comments: C,
    github: GithubClient,
    projects: ProjectsV2Client,
    worker: W,
    config: VerificationCoordinatorConfig,
    events: Option<std::sync::Arc<dyn crate::triage::coordinator::EventEmitter>>,
}

impl<C, W> VerificationCoordinator<C, W>
where
    C: TriageCommentPort + Clone,
    W: VerificationWorker,
{
    pub fn new(
        store: SharedFactoryStore,
        comments: C,
        github: GithubClient,
        projects: ProjectsV2Client,
        worker: W,
        config: VerificationCoordinatorConfig,
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

    pub fn with_events(mut self, events: std::sync::Arc<dyn crate::triage::coordinator::EventEmitter>) -> Self {
        self.events = Some(events);
        self
    }

    pub async fn poll_once(&mut self, service: &ServiceConfig) -> Result<VerificationPollSummary> {
        let mut summary = VerificationPollSummary {
            verification_enabled: service.verification.enabled,
            ..Default::default()
        };
        // Recovery runs before eligibility and before any dispatch: owned
        // processes, containers, and workspaces from a crashed run must be
        // safely terminal before new claims.
        summary.recovered = self.recover_interrupted_attempts().await?;
        if !service.verification.enabled {
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
        let trigger = service.verification.trigger_state.trim();
        let candidates = self.store.list_a5_eligible_verification_runs()?;
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
                    summary.preview_published += 1;
                }
                Ok(ProcessOutcome::Waiting) => summary.waiting += 1,
                Err(error) => {
                    summary.attempts_failed += 1;
                    tracing::warn!(
                        event = "verification_candidate_failed",
                        run_id = %candidate.run_id,
                        error = %error,
                        "verification candidate processing failed"
                    );
                }
            }
        }
        Ok(summary)
    }

    /// Restart recovery: terminate and reap persisted process groups or stop
    /// and remove persisted containers, remove label-discoverable stopped
    /// orphans, mark every interrupted command and attempt, and clean up the
    /// disposable workspaces.
    async fn recover_interrupted_attempts(&mut self) -> Result<u32> {
        let mut recovered = 0u32;
        let _ = cleanup_stopped_verification_containers().await;
        let attempts = self.store.list_running_verification_attempts()?;
        for attempt in attempts {
            recovered += 1;
            let command_runs = self.store.list_verification_command_runs(&attempt.attempt_id)?;
            for run in command_runs {
                if run.status != "launching" && run.status != "running" {
                    continue;
                }
                if let Some(container_id) = run.container_id.as_deref() {
                    let _ = crate::verification::executor::stop_persisted_container(container_id).await;
                } else if let Some(identity) = local_identity_from_record(&run) {
                    let _ = crate::triage::process_identity::terminate_process_group(&identity).await;
                }
                let _ = self.store.complete_verification_command(
                    crate::triage::store::CompleteVerificationCommandRequest {
                        command_run_id: &run.command_run_id,
                        status: "interrupted",
                        exit_code: None,
                        termination_reason: Some("restart_recovery"),
                        passed: Some(false),
                        output_tail: None,
                        output_sha256: None,
                        started_at: Utc::now(),
                        completed_at: Utc::now(),
                        duration_ms: 0,
                    },
                );
            }
            let error = FactoryError::new(
                "verification_attempt_interrupted",
                "verification_coordinator",
                "attempt was running when the process stopped; interrupted by restart recovery"
                    .to_string(),
                true,
                None,
            );
            let _ = self.store.interrupt_verification_stage_run(&attempt.stage_run_id, &error);
            let _ = self.store.update_verification_attempt(
                crate::triage::store::UpdateVerificationAttemptRequest {
                    attempt_id: &attempt.attempt_id,
                    status: Some("interrupted"),
                    workspace_path: None,
                    evidence_dir: None,
                    error: Some(&error),
                },
            );
            if let Some(workspace_path) = attempt.workspace_path.as_deref() {
                self.cleanup_attempt_workspace(workspace_path, &attempt.attempt_id);
            }
        }
        Ok(recovered)
    }

    fn cleanup_attempt_workspace(&self, workspace_path: &str, attempt_id: &str) {
        if let Some(attempt_root) = attempt_root_for_cleanup(
            Path::new(workspace_path),
            &self.config.workspace_root,
            attempt_id,
        ) {
            let _ = std::fs::remove_dir_all(&attempt_root);
        }
    }

    async fn process_candidate(
        &mut self,
        service: &ServiceConfig,
        candidate: &crate::triage::store::A5EligibleVerificationRun,
        issue_number: u64,
    ) -> Result<ProcessOutcome> {
        let pull = self.github.get_pull_request(candidate.pr_number).await?;
        if !pull.state.eq_ignore_ascii_case("open") {
            return Ok(ProcessOutcome::Waiting);
        }
        if pull.head.sha != candidate.reviewed_head_sha
            || pull.base.sha != candidate.reviewed_base_sha
        {
            self.record_event(
                Some(&candidate.run_id),
                None,
                "verification_stale_revision",
                serde_json::json!({
                    "status": "waiting",
                    "reason": "head_or_base_changed_since_review",
                    "reviewed_head_sha": candidate.reviewed_head_sha,
                    "reviewed_base_sha": candidate.reviewed_base_sha,
                    "live_head_sha": pull.head.sha,
                    "live_base_sha": pull.base.sha,
                }),
            )?;
            return Ok(ProcessOutcome::Waiting);
        }
        if candidate
            .route_state
            .as_deref()
            .map(|state| state.trim() != service.verification.trigger_state.trim())
            .unwrap_or(true)
        {
            return Ok(ProcessOutcome::Waiting);
        }

        let review_artifact = self
            .store
            .get_review_artifact(&candidate.review_artifact_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "review artifact {} is missing",
                    candidate.review_artifact_id
                ))
            })?;
        let spec = self
            .store
            .get_spec_artifact(&candidate.spec_artifact_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "approved spec artifact {} is missing",
                    candidate.spec_artifact_id
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

        let configuration_revision = verification_configuration_revision(&service.verification);
        let stage = self.store.claim_verification_attempt(ClaimAttemptRequest {
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
            model: service.verification.model.clone(),
            workspace_path: None,
            output_path: None,
            pid: None,
            process_group_id: None,
            process_start_token: None,
            executable_identity: None,
        })?;
        let attempt_id = Uuid::now_v7().to_string();
        let execution_profile = if service.workspace.docker.is_some() {
            "docker"
        } else {
            "local"
        };
        if let Err(error) = self.store.store_verification_attempt_inputs(
            crate::triage::store::StoreVerificationAttemptRequest {
                attempt_id: attempt_id.clone(),
                stage_run_id: stage.stage_run_id.clone(),
                pr_number: candidate.pr_number,
                reviewed_head_sha: pull.head.sha.clone(),
                base_sha: pull.base.sha.clone(),
                spec_artifact_id: candidate.spec_artifact_id.clone(),
                implementation_artifact_id: candidate.implementation_artifact_id.clone(),
                review_artifact_id: candidate.review_artifact_id.clone(),
                configuration_revision: configuration_revision.clone(),
                execution_profile: execution_profile.to_string(),
            },
        ) {
            let factory_error = FactoryError::new(
                "verification_attempt_input_persist_failed",
                "verification_coordinator",
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
                "verification_started",
                serde_json::json!({
                    "status": "running",
                    "pr_number": candidate.pr_number,
                    "reviewed_head_sha": pull.head.sha,
                    "base_sha": pull.base.sha,
                    "attempt_id": attempt_id,
                }),
            )?;

            let prompt = load_verification_prompt(&self.config.workflow_dir, service)?;
            let repo_path = resolve_repo_path(service)?;
            let workspace_root = resolve_workspace_root(service)?;
            let github_token = resolve_github_token(&service.tracker)
                .map(|token| token.token)
                .filter(|token| !token.trim().is_empty());
            let storage_path = service.storage.path.clone().ok_or_else(|| {
                SymphonyError::InvalidWorkflowConfig(
                    "storage.path is required when verification is enabled".to_string(),
                )
            })?;

            // Exact-head workspace: fetch the pull ref with subprocess-scoped
            // auth and verify it equals the A4 reviewed head.
            let pull_ref = format!("symphony-verification/{attempt_id}");
            fetch_pull_head_verified(
                &repo_path,
                candidate.pr_number,
                &pull.head.sha,
                github_token.as_deref(),
                &pull_ref,
            )
            .await?;
            let workspace = prepare_verification_workspace(
                &repo_path,
                &workspace_root,
                &attempt_id,
                &pull.head.sha,
            )
            .await?;
            self.store.update_verification_attempt(
                crate::triage::store::UpdateVerificationAttemptRequest {
                    attempt_id: &attempt_id,
                    status: None,
                    workspace_path: Some(&workspace.workspace_path.display().to_string()),
                    evidence_dir: Some(&workspace.evidence_dir.display().to_string()),
                    error: None,
                },
            )?;

            let docker = service.workspace.docker.clone();
            let commands = service.verification.commands.clone();
            let command_outcome = self
                .run_commands(
                    service,
                    &attempt_id,
                    &stage.stage_run_id,
                    &candidate.run_id,
                    &workspace,
                    &commands,
                    &configuration_revision,
                    docker,
                )
                .await;
            let (command_runs, command_failed) = match command_outcome {
                Ok((runs, failed)) => (runs, failed),
                Err(error) => {
                    let factory_error = FactoryError::new(
                        "verification_command_execution_failed",
                        "verification_coordinator",
                        error.to_string(),
                        false,
                        None,
                    );
                    let _ = self.store.fail_attempt(&stage.stage_run_id, factory_error.clone());
                    let _ = self.store.update_verification_attempt(
                        crate::triage::store::UpdateVerificationAttemptRequest {
                            attempt_id: &attempt_id,
                            status: Some("failed"),
                            workspace_path: None,
                            evidence_dir: None,
                            error: Some(&factory_error),
                        },
                    );
                    self.cleanup_attempt_workspace(
                        &workspace.workspace_path.display().to_string(),
                        &attempt_id,
                    );
                    return Err(error);
                }
            };
            if command_failed {
                // A failed command is expected product evidence: complete the
                // attempt in Verification with a failed gate below.
                tracing::info!(
                    event = "verification_commands_failed",
                    run_id = %candidate.run_id,
                    attempt_id = %attempt_id,
                    "commands did not all pass; gate will fail"
                );
            }

            // The pinned commit, tree, and tracked files must be unchanged.
            if let Err(error) = verify_workspace_unchanged(&workspace.workspace_path, &pull.head.sha)
            {
                let factory_error = FactoryError::new(
                    "verification_workspace_modified",
                    "verification_coordinator",
                    error.to_string(),
                    false,
                    None,
                );
                let _ = self.store.fail_attempt(&stage.stage_run_id, factory_error.clone());
                let _ = self.store.update_verification_attempt(
                    crate::triage::store::UpdateVerificationAttemptRequest {
                        attempt_id: &attempt_id,
                        status: Some("failed"),
                        workspace_path: None,
                        evidence_dir: None,
                        error: Some(&factory_error),
                    },
                );
                self.cleanup_attempt_workspace(
                    &workspace.workspace_path.display().to_string(),
                    &attempt_id,
                );
                return Err(error);
            }

            // Re-read head/base before evidence and verifier work.
            if self.revision_changed(candidate, issue_number).await? {
                return self
                    .supersede(
                        &stage.stage_run_id,
                        &attempt_id,
                        &workspace,
                        "head_or_base_changed_before_verifier",
                    )
                    .await;
            }

            // Evidence collection: bounded regular files, stored by digest.
            let artifacts = artifacts_dir(Path::new(&storage_path));
            let evidence_records = collect_evidence(
                &workspace.evidence_dir,
                &artifacts,
                &candidate.run_id,
                &attempt_id,
                service.verification.max_evidence_files,
                service.verification.max_evidence_bytes,
            )?;
            self.store
                .store_verification_evidence(&evidence_records)?;

            // Read-only verifier.
            let verifier_command = command_for_verification(service)?;
            let mut worker_request = VerificationWorkerRequest {
                attempt_id: attempt_id.clone(),
                workspace_root,
                repo_path,
                command: verifier_command,
                prompt,
                config: service.verification.clone(),
                model: model_for_verification(service),
                harness: harness_for_service(service),
                issue: crate::triage::runner::TriageIssueIdentity {
                    id: candidate.issue_id.clone(),
                    identifier: candidate.issue_identifier.clone(),
                    title: pull.title.clone(),
                },
                codex: (service.agent_backend == AgentBackend::Codex)
                    .then(|| service.codex.clone()),
                approved_spec: spec.artifact.clone(),
                implementation_manifest: implementation.manifest.clone(),
                review_artifact: review_artifact.clone(),
                command_runs: command_runs.clone(),
                evidence: evidence_records.clone(),
            };

            let mut last_error = None;
            let mut output_bytes = None;
            let mut total_usage = StageUsage::default();
            let base_prompt = worker_request.prompt.clone();
            for reprompt in 0..=service.verification.max_reprompts {
                worker_request.prompt = if reprompt == 0 {
                    base_prompt.clone()
                } else {
                    format!(
                        "{base_prompt}\n\nPrevious verifier output was rejected: {}. Return ONLY a corrected strict JSON manifest.",
                        last_error.as_deref().unwrap_or("invalid manifest")
                    )
                };
                match self.worker.run(&worker_request).await {
                    Ok(result) => {
                        total_usage.input_tokens += result.usage.input_tokens;
                        total_usage.output_tokens += result.usage.output_tokens;
                        total_usage.total_tokens += result.usage.total_tokens;
                        output_bytes = Some(result.output_bytes);
                        break;
                    }
                    Err(error) => {
                        last_error = Some(error.to_string());
                    }
                }
            }
            let Some(output_bytes) = output_bytes else {
                let factory_error = FactoryError::new(
                    "verification_verifier_failed",
                    "verification_coordinator",
                    format!(
                        "verifier exhausted {} attempts: {}",
                        service.verification.max_attempts,
                        last_error.unwrap_or_else(|| "unknown verifier failure".to_string())
                    ),
                    true,
                    None,
                );
                let _ = self.store.fail_attempt(&stage.stage_run_id, factory_error.clone());
                let _ = self.store.update_verification_attempt(
                    crate::triage::store::UpdateVerificationAttemptRequest {
                        attempt_id: &attempt_id,
                        status: Some("failed"),
                        workspace_path: None,
                        evidence_dir: None,
                        error: Some(&factory_error),
                    },
                );
                self.cleanup_attempt_workspace(
                    &workspace.workspace_path.display().to_string(),
                    &attempt_id,
                );
                return Err(SymphonyError::TriageError(factory_error.remediation.clone()));
            };

            // Strict manifest validation + deterministic gate.
            let manifest: crate::verification::domain::VerifierManifest =
                serde_json::from_slice(&output_bytes).map_err(|error| {
                    fail_verification_attempt(
                        &self.store,
                        &stage.stage_run_id,
                        &attempt_id,
                        "verification_verifier_manifest_invalid",
                        format!("verifier manifest is not valid JSON: {error}"),
                        &workspace,
                        &self.config.workspace_root,
                    );
                    SymphonyError::TriageError(format!(
                        "verifier manifest is not valid JSON: {error}"
                    ))
                })?;
            let criterion_count = spec.artifact.acceptance_criteria.len();
            let gate_identity = GateIdentity {
                spec_artifact_id: &candidate.spec_artifact_id,
                implementation_artifact_id: &candidate.implementation_artifact_id,
                review_artifact_id: &candidate.review_artifact_id,
                reviewed_head_sha: &pull.head.sha,
                base_sha: &pull.base.sha,
                criterion_count,
            };
            let verdict = compute_gate(&manifest, &gate_identity, &command_runs, &evidence_records)
                .map_err(|error| {
                    fail_verification_attempt(
                        &self.store,
                        &stage.stage_run_id,
                        &attempt_id,
                        "verification_verifier_manifest_rejected",
                        error.to_string(),
                        &workspace,
                        &self.config.workspace_root,
                    );
                    error
                })?;

            // Re-read head/base before gate persistence and publication.
            if self.revision_changed(candidate, issue_number).await? {
                return self
                    .supersede(
                        &stage.stage_run_id,
                        &attempt_id,
                        &workspace,
                        "head_or_base_changed_after_verifier",
                    )
                    .await;
            }

            let gate_status = if verdict.passed { "passed" } else { "failed" };
            let gate_id = Uuid::now_v7().to_string();
            let gate = VerificationGateRecord {
                gate_id: gate_id.clone(),
                run_id: candidate.run_id.clone(),
                attempt_id: attempt_id.clone(),
                status: gate_status.to_string(),
                verifier_manifest: Some(manifest),
                command_summary: Some(serde_json::json!({
                    "reasons": verdict.reasons,
                    "criteria": verdict.criteria.iter().map(|criterion| {
                        serde_json::json!({
                            "index": criterion.index,
                            "status": criterion.status.as_str(),
                            "rationale": criterion.rationale,
                            "evidence": criterion.evidence,
                        })
                    }).collect::<Vec<_>>(),
                })),
                computed_at: Some(Utc::now()),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.store.store_verification_gate(&gate)?;
            self.store.complete_verification_stage_run(&stage.stage_run_id, total_usage)?;
            self.store.update_verification_attempt(
                crate::triage::store::UpdateVerificationAttemptRequest {
                    attempt_id: &attempt_id,
                    status: Some("completed"),
                    workspace_path: None,
                    evidence_dir: None,
                    error: None,
                },
            )?;

            // Preview publication: one owned comment; nothing else mutates.
            let intent = self
                .store
                .create_verification_publication_intent(&candidate.run_id, &attempt_id, "preview")?;
            let comment_id = publish_preview_comment(
                &self.comments,
                &self.store,
                &intent,
                &candidate.run_id,
                &attempt_id,
                candidate.pr_number,
                &pull.head.sha,
                &gate,
                &command_runs,
                &evidence_records,
                self.config.max_pages,
            )
            .await?;
            self.record_event(
                Some(&candidate.run_id),
                Some(&stage.stage_run_id),
                "verification_completed",
                serde_json::json!({
                    "status": gate_status,
                    "gate_id": gate_id,
                    "attempt_id": attempt_id,
                    "comment_id": comment_id,
                    "commands_failed": command_failed,
                }),
            )?;
            self.cleanup_attempt_workspace(
                &workspace.workspace_path.display().to_string(),
                &attempt_id,
            );
            Ok(ProcessOutcome::Completed)
        }
        .await;

        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                // Infrastructure failures after a successful claim must leave
                // the stage run terminal so the run can be reclaimed.
                let message = error.to_string();
                if self
                    .store
                    .get_stage_run(&stage.stage_run_id)
                    .map(|stage| {
                        stage
                            .map(|stage| {
                                stage.status == crate::triage::domain::StageStatus::Running
                            })
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
                {
                    let factory_error = FactoryError::new(
                        "verification_attempt_failed",
                        "verification_coordinator",
                        message,
                        true,
                        None,
                    );
                    let _ = self.store.fail_attempt(&stage.stage_run_id, factory_error.clone());
                    let _ = self.store.update_verification_attempt(
                        crate::triage::store::UpdateVerificationAttemptRequest {
                            attempt_id: &attempt_id,
                            status: Some("failed"),
                            workspace_path: None,
                            evidence_dir: None,
                            error: Some(&factory_error),
                        },
                    );
                }
                Err(error)
            }
        }
    }

    /// Run configured commands in order against the exact reviewed head,
    /// stopping at the first non-passing result. Returns the durable command
    /// records and whether any command failed.
    async fn run_commands(
        &self,
        service: &ServiceConfig,
        attempt_id: &str,
        _stage_run_id: &str,
        run_id: &str,
        workspace: &VerificationWorkspace,
        commands: &[VerificationCommandConfig],
        configuration_revision: &str,
        docker: Option<crate::domain::DockerConfig>,
    ) -> Result<(Vec<crate::verification::domain::VerificationCommandRunRecord>, bool)> {
        let profile = if docker.is_some() {
            crate::implementation::domain::ExecutionProfile::Docker
        } else {
            crate::implementation::domain::ExecutionProfile::Local
        };
        let mut records = Vec::new();
        let mut failed = false;
        let mut interrupted_index: Option<usize> = None;
        for (index, command) in commands.iter().enumerate() {
            let ordinal = (index + 1) as u32;
            let nonce = Uuid::now_v7().to_string();
            let sha256 = command_sha256(&command.command);
            let command_run_id = self.store.record_verification_command_launch(
                run_id,
                attempt_id,
                ordinal,
                &command.name,
                command.kind,
                configuration_revision,
                &sha256,
                profile.as_str(),
                &nonce,
            )?;
            if interrupted_index.is_some() {
                self.store.mark_verification_command_not_run(&command_run_id)?;
                continue;
            }
            let request = CommandExecutionRequest {
                attempt_id: attempt_id.to_string(),
                command_name: command.name.clone(),
                workspace_path: workspace.workspace_path.clone(),
                evidence_dir: workspace.evidence_dir.clone(),
                home_dir: workspace.home_dir.clone(),
                command: command.command.clone(),
                timeout_ms: command.timeout_ms,
                execution_profile: profile,
                docker: docker.clone(),
            };
            let store = self.store.clone();
            let launch_nonce = nonce.clone();
            let launch_command_run_id = command_run_id.clone();
            let on_launch = move |identity: LaunchIdentity| match identity {
                LaunchIdentity::Process(identity) => store.cas_verification_launch_identity(
                    &launch_command_run_id,
                    &launch_nonce,
                    &identity,
                ),
                LaunchIdentity::Container { container_id } => store
                    .cas_verification_container(&launch_command_run_id, &launch_nonce, &container_id),
            };
            match execute_command(&request, on_launch).await {
                Ok(result) => {
                    let status = if result.passed { "completed" } else { "failed" };
                    self.store.complete_verification_command(
                        crate::triage::store::CompleteVerificationCommandRequest {
                            command_run_id: &command_run_id,
                            status,
                            exit_code: result.exit_code,
                            termination_reason: result.termination_reason.as_deref(),
                            passed: Some(result.passed),
                            output_tail: Some(&result.output.stdout_tail),
                            output_sha256: Some(&result.output.output_sha256),
                            started_at: result.started_at,
                            completed_at: result.completed_at,
                            duration_ms: result.duration_ms,
                        },
                    )?;
                    records.push(
                        self.store
                            .list_verification_command_runs(attempt_id)?
                            .into_iter()
                            .find(|record| record.command_run_id == command_run_id)
                            .ok_or_else(|| {
                                SymphonyError::StorageError(format!(
                                    "command run {command_run_id} missing after completion"
                                ))
                            })?,
                    );
                    if !result.passed {
                        failed = true;
                        interrupted_index = Some(index + 1);
                    }
                }
                Err(CommandRunFailure::TimedOut(output)) => {
                    self.store.complete_verification_command(
                        crate::triage::store::CompleteVerificationCommandRequest {
                            command_run_id: &command_run_id,
                            status: "interrupted",
                            exit_code: None,
                            termination_reason: Some("timeout"),
                            passed: Some(false),
                            output_tail: Some(&output.stdout_tail),
                            output_sha256: Some(&output.output_sha256),
                            started_at: Utc::now(),
                            completed_at: Utc::now(),
                            duration_ms: 0,
                        },
                    )?;
                    failed = true;
                    interrupted_index = Some(index + 1);
                }
                Err(CommandRunFailure::SpawnError(message))
                | Err(CommandRunFailure::NotSignalable(message))
                | Err(CommandRunFailure::StillRunning(message)) => {
                    self.store.complete_verification_command(
                        crate::triage::store::CompleteVerificationCommandRequest {
                            command_run_id: &command_run_id,
                            status: "interrupted",
                            exit_code: None,
                            termination_reason: Some("launch_or_termination_failure"),
                            passed: Some(false),
                            output_tail: None,
                            output_sha256: None,
                            started_at: Utc::now(),
                            completed_at: Utc::now(),
                            duration_ms: 0,
                        },
                    )?;
                    failed = true;
                    interrupted_index = Some(index + 1);
                    tracing::warn!(
                        event = "verification_command_launch_failure",
                        command = %command.name,
                        error = %message,
                        "verification command could not run"
                    );
                }
            }
        }
        let records = self.store.list_verification_command_runs(attempt_id)?;
        let _ = service;
        Ok((records, failed))
    }

    /// Re-read the live pull and detect head/base drift since the last check.
    async fn revision_changed(
        &self,
        candidate: &crate::triage::store::A5EligibleVerificationRun,
        _issue_number: u64,
    ) -> Result<bool> {
        let pull = self.github.get_pull_request(candidate.pr_number).await?;
        Ok(pull.head.sha != candidate.reviewed_head_sha
            || pull.base.sha != candidate.reviewed_base_sha)
    }

    /// Supersede without publishing: mark the attempt superseded, interrupt
    /// the stage run so a fresh cycle can claim, and clean the workspace.
    async fn supersede(
        &self,
        stage_run_id: &str,
        attempt_id: &str,
        workspace: &VerificationWorkspace,
        reason: &str,
    ) -> Result<ProcessOutcome> {
        let error = FactoryError::new(
            "verification_superseded",
            "verification_coordinator",
            format!("attempt superseded because {reason}"),
            false,
            None,
        );
        let _ = self.store.interrupt_verification_stage_run(stage_run_id, &error);
        let _ = self.store.update_verification_attempt(
            crate::triage::store::UpdateVerificationAttemptRequest {
                attempt_id,
                status: Some("superseded"),
                workspace_path: None,
                evidence_dir: None,
                error: Some(&error),
            },
        );
        self.cleanup_attempt_workspace(&workspace.workspace_path.display().to_string(), attempt_id);
        Ok(ProcessOutcome::Waiting)
    }

    fn record_event(
        &self,
        run_id: Option<&str>,
        stage_run_id: Option<&str>,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        let mut store = self.store.clone();
        store.record_event(crate::triage::domain::FactoryEventRecord {
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

/// Fail a verification attempt with a durable error and clean its workspace.
fn fail_verification_attempt(
    store: &SharedFactoryStore,
    stage_run_id: &str,
    attempt_id: &str,
    code: &str,
    message: String,
    workspace: &VerificationWorkspace,
    workspace_root: &Path,
) {
    let mut store = store.clone();
    let factory_error = FactoryError::new(code, "verification_coordinator", message, false, None);
    let _ = store.fail_attempt(stage_run_id, factory_error.clone());
    let _ = store.update_verification_attempt(
        crate::triage::store::UpdateVerificationAttemptRequest {
            attempt_id,
            status: Some("failed"),
            workspace_path: None,
            evidence_dir: None,
            error: Some(&factory_error),
        },
    );
    if let Some(attempt_root) =
        attempt_root_for_cleanup(&workspace.workspace_path, workspace_root, attempt_id)
    {
        let _ = std::fs::remove_dir_all(&attempt_root);
    }
}

fn local_identity_from_record(
    record: &crate::verification::domain::VerificationCommandRunRecord,
) -> Option<crate::triage::process_identity::ProcessIdentity> {
    let pid = record.pid?;
    let process_group_id = record.process_group_id?;
    if pid <= 0 || process_group_id <= 0 {
        return None;
    }
    Some(crate::triage::process_identity::ProcessIdentity {
        pid,
        process_group_id,
        start_token: record.process_start_token.clone(),
        executable: record.executable_identity.clone(),
    })
}

fn load_verification_prompt(workflow_dir: &Path, service: &ServiceConfig) -> Result<String> {
    let path = workflow_dir.join(&service.verification.prompt);
    std::fs::read_to_string(&path).map_err(|error| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "verification prompt {} could not be read: {error}",
            path.display()
        ))
    })
}

fn verification_configuration_revision(config: &VerificationConfig) -> String {
    let encoded = serde_json::to_vec(config).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(encoded);
    hex::encode(digest.finalize())
}

fn resolve_repo_path(service: &ServiceConfig) -> Result<PathBuf> {
    let repo_path = service.workspace.repo.as_deref().ok_or_else(|| {
        SymphonyError::InvalidWorkflowConfig(
            "workspace.repo is required when verification is enabled".to_string(),
        )
    })?;
    let trimmed = repo_path.trim();
    if trimmed.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "workspace.repo cannot be empty when verification is enabled".to_string(),
        ));
    }
    canonicalize(Path::new(trimmed)).map_err(|error| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "failed to resolve workspace.repo '{trimmed}' for the verification stage: {error}"
        ))
    })
}

pub fn resolve_workspace_root(service: &ServiceConfig) -> Result<PathBuf> {
    let root = service.workspace.root.trim();
    if root.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "workspace.root cannot be empty when verification is enabled".to_string(),
        ));
    }
    canonicalize(Path::new(root)).map_err(|error| {
        SymphonyError::InvalidWorkflowConfig(format!(
            "failed to resolve workspace.root '{root}' for the verification stage: {error}"
        ))
    })
}

fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

enum ProcessOutcome {
    Completed,
    Waiting,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_revision_changes_with_commands() {
        let mut config = VerificationConfig::default();
        config.commands = vec![VerificationCommandConfig {
            name: "unit".to_string(),
            kind: crate::verification::domain::VerificationCommandKind::Test,
            command: "cargo test".to_string(),
            timeout_ms: 60_000,
        }];
        let first = verification_configuration_revision(&config);
        config.commands[0].command = "cargo test --all".to_string();
        let second = verification_configuration_revision(&config);
        assert_ne!(first, second);
    }

    #[test]
    fn command_failure_records_carry_the_reviewed_identity_fields() {
        let record = crate::verification::domain::VerificationCommandRunRecord {
            command_run_id: "c1".to_string(),
            run_id: "run".to_string(),
            attempt_id: "attempt".to_string(),
            ordinal: 1,
            name: "unit".to_string(),
            kind: crate::verification::domain::VerificationCommandKind::Test,
            configuration_revision: "cfg".to_string(),
            command_sha256: "sha".to_string(),
            status: "running".to_string(),
            launch_nonce: None,
            pid: Some(12),
            process_group_id: Some(12),
            process_start_token: Some("token".to_string()),
            executable_identity: Some("/bin/sh".to_string()),
            container_id: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            exit_code: None,
            termination_reason: None,
            passed: None,
            output_tail: None,
            output_sha256: None,
            execution_profile: "local".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let identity = local_identity_from_record(&record).unwrap();
        assert_eq!(identity.pid, 12);
        assert_eq!(identity.process_group_id, 12);
        assert_eq!(identity.start_token.as_deref(), Some("token"));
        assert!(crate::verification::workspace::attempt_root_for_cleanup(
            Path::new("/srv/verification-att-1/workspace"),
            Path::new("/srv"),
            "att-1",
        )
        .is_some());
    }
}
