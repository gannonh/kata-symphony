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
    cleanup_stopped_verification_containers, command_sha256, execute_command,
    CommandExecutionRequest, CommandRunFailure, LaunchIdentity,
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

    pub fn with_events(
        mut self,
        events: std::sync::Arc<dyn crate::triage::coordinator::EventEmitter>,
    ) -> Self {
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
        let candidates = self
            .store
            .list_a5_eligible_verification_runs(service.verification.max_attempts.max(1))?;
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
            let command_runs = self
                .store
                .list_verification_command_runs(&attempt.attempt_id)?;
            let mut termination_verified = true;
            for run in command_runs {
                if run.status != "launching" && run.status != "running" {
                    continue;
                }
                let verified = if let Some(container_id) = run.container_id.as_deref() {
                    crate::verification::executor::stop_persisted_container(container_id)
                        .await
                        .is_ok()
                } else if let Some(identity) = local_identity_from_record(&run) {
                    matches!(
                        crate::triage::process_identity::terminate_process_group(&identity).await,
                        crate::triage::process_identity::TerminationOutcome::Terminated
                    )
                } else {
                    // A launching record with no identity never released its
                    // payload; nothing is left to terminate.
                    true
                };
                // Accumulate: a termination failure recorded for an earlier
                // command run must not be erased by a later run with no
                // persisted identity.
                termination_verified = termination_verified && verified;
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
            // The read-only verifier may also still be running.
            if let Some(identity) = local_identity_from_attempt(&attempt) {
                termination_verified = termination_verified
                    && matches!(
                        crate::triage::process_identity::terminate_process_group(&identity).await,
                        crate::triage::process_identity::TerminationOutcome::Terminated
                    );
            }
            if !termination_verified {
                // The attempt stays RUNNING: the stage pin keeps the run out
                // of the eligibility queue, no new dispatch can start while
                // the surviving process group or container is still owned,
                // and the next poll retries termination until it is verified.
                tracing::error!(
                    event = "verification_recovery_termination_failed",
                    attempt_id = %attempt.attempt_id,
                    "persisted process group or container survived recovery; attempt retained for retry"
                );
                continue;
            }
            let error = FactoryError::new(
                "verification_attempt_interrupted",
                "verification_coordinator",
                "attempt was running when the process stopped; interrupted by restart recovery"
                    .to_string(),
                true,
                None,
            );
            let _ = self
                .store
                .interrupt_verification_stage_run(&attempt.stage_run_id, &error);
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
                    &attempt_id,
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
                    // Workspace is retained: termination could not be
                    // verified, so cleanup must wait for recovery.
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
                fail_verification_attempt(
                    &self.store,
                    &stage.stage_run_id,
                    &attempt_id,
                    "verification_workspace_modified",
                    error.to_string(),
                    &workspace,
                    &self.config.workspace_root,
                    true,
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
                &crate::verification::evidence::EvidenceLimits {
                    run_id: candidate.run_id.clone(),
                    attempt_id: attempt_id.clone(),
                    max_files: service.verification.max_evidence_files,
                    max_bytes: service.verification.max_evidence_bytes,
                },
            )
            .inspect_err(|error| {
                fail_verification_attempt(
                    &self.store,
                    &stage.stage_run_id,
                    &attempt_id,
                    "verification_evidence_storage_failed",
                    error.to_string(),
                    &workspace,
                    &self.config.workspace_root,
                    true,
                );
            })?;
            self.store
                .store_verification_evidence(&evidence_records)
                .inspect_err(|error| {
                    fail_verification_attempt(
                        &self.store,
                        &stage.stage_run_id,
                        &attempt_id,
                        "verification_evidence_storage_failed",
                        error.to_string(),
                        &workspace,
                        &self.config.workspace_root,
                        true,
                    );
                })?;

            // Read-only verifier.
            let verifier_command = command_for_verification(service).inspect_err(|error| {
                fail_verification_attempt(
                    &self.store,
                    &stage.stage_run_id,
                    &attempt_id,
                    "verification_verifier_command_missing",
                    error.to_string(),
                    &workspace,
                    &self.config.workspace_root,
                    true,
                );
            })?;
            // The verifier works from the reviewed-head bundle clone (no
            // authenticated remote), not from the controller's checkout.
            let verifier_repo_path = workspace.workspace_path.clone();
            let store_sink = self.store.clone();
            let attempt_sink = attempt_id.clone();
            let identity_error: std::sync::Arc<std::sync::Mutex<Option<String>>> =
                std::sync::Arc::new(std::sync::Mutex::new(None));
            let identity_error_sink = identity_error.clone();
            let spawned: Option<crate::triage::runner::TriageSpawnSink> =
                Some(std::sync::Arc::new(move |info| {
                    if let Err(error) =
                        store_sink.record_verifier_identity(&attempt_sink, &info.identity)
                    {
                        *identity_error_sink
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                            Some(error.to_string());
                    }
                }));
            let mut worker_request = VerificationWorkerRequest {
                attempt_id: attempt_id.clone(),
                workspace_root,
                repo_path: verifier_repo_path,
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
                spec_artifact_id: candidate.spec_artifact_id.clone(),
                implementation_artifact_id: candidate.implementation_artifact_id.clone(),
                spawned,
            };

            let mut last_error: Option<String> = None;
            let mut manifest: Option<crate::verification::domain::VerifierManifest> = None;
            let mut total_usage = StageUsage::default();
            let base_prompt = worker_request.prompt.clone();
            let criterion_count = spec.artifact.acceptance_criteria.len();
            let gate_identity = GateIdentity {
                spec_artifact_id: &candidate.spec_artifact_id,
                implementation_artifact_id: &candidate.implementation_artifact_id,
                review_artifact_id: &candidate.review_artifact_id,
                reviewed_head_sha: &pull.head.sha,
                base_sha: &pull.base.sha,
                criterion_count,
            };
            for reprompt in 0..=service.verification.max_reprompts {
                worker_request.prompt = if reprompt == 0 {
                    base_prompt.clone()
                } else {
                    format!(
                        "{base_prompt}\n\nPrevious verifier output was rejected: {}. Return ONLY a corrected strict JSON manifest.",
                        last_error.as_deref().unwrap_or("invalid manifest")
                    )
                };
                let result = match self.worker.run(&worker_request).await {
                    Ok(result) => result,
                    Err(error) => {
                        last_error = Some(format!("worker invocation failed: {error}"));
                        continue;
                    }
                };
                total_usage.input_tokens += result.usage.input_tokens;
                total_usage.output_tokens += result.usage.output_tokens;
                total_usage.total_tokens += result.usage.total_tokens;
                if let Some(error) = identity_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    last_error = Some(format!(
                        "verifier process identity could not be durably recorded: {error}"
                    ));
                    continue;
                }
                let parsed: crate::verification::domain::VerifierManifest =
                    match serde_json::from_slice(&result.output_bytes) {
                        Ok(manifest) => manifest,
                        Err(error) => {
                            last_error = Some(format!("manifest is not valid JSON: {error}"));
                            continue;
                        }
                    };
                // Strict validation runs inside the retry loop so an invalid
                // manifest costs a reprompt, not a failed attempt.
                if let Err(error) = crate::verification::gate::validate_manifest(
                    &parsed,
                    &gate_identity,
                    &evidence_records,
                ) {
                    last_error = Some(error.to_string());
                    continue;
                }
                manifest = Some(parsed);
                break;
            }
            let Some(manifest) = manifest else {
                fail_verification_attempt(
                    &self.store,
                    &stage.stage_run_id,
                    &attempt_id,
                    "verification_verifier_failed",
                    format!(
                        "verifier exhausted {} invocations: {}",
                        service.verification.max_reprompts.saturating_add(1),
                        last_error.unwrap_or_else(|| "unknown verifier failure".to_string())
                    ),
                    &workspace,
                    &self.config.workspace_root,
                    true,
                );
                return Err(SymphonyError::TriageError(
                    "verifier could not produce a valid manifest; attempt blocked".to_string(),
                ));
            };

            // Deterministic gate from the validated manifest.
            let verdict = compute_gate(&manifest, &gate_identity, &command_runs, &evidence_records)
                .inspect_err(|error| {
                    fail_verification_attempt(
                        &self.store,
                        &stage.stage_run_id,
                        &attempt_id,
                        "verification_verifier_manifest_rejected",
                        error.to_string(),
                        &workspace,
                        &self.config.workspace_root,
                        true,
                    );
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

            // Final revision fence immediately before the comment side
            // effect: a drifted head/base supersedes without leaving any
            // durable publication intent behind.
            if self.revision_changed(candidate, issue_number).await? {
                self.record_event(
                    Some(&candidate.run_id),
                    Some(&stage.stage_run_id),
                    "verification_superseded",
                    serde_json::json!({
                        "status": "superseded",
                        "reason": "head_or_base_changed_before_publication",
                        "attempt_id": attempt_id,
                    }),
                )?;
                return self
                    .supersede(
                        &stage.stage_run_id,
                        &attempt_id,
                        &workspace,
                        "head_or_base_changed_before_publication",
                    )
                    .await;
            }

            // Preview publication: one owned comment; nothing else mutates.
            // The intent is reused across attempts for the same run and kind,
            // so the marker stays stable: one owned comment per run and head.
            let intent = self
                .store
                .create_verification_publication_intent(&candidate.run_id, &attempt_id, "preview")?;
            let comment_id = publish_preview_comment(
                &self.comments,
                &self.store,
                &intent,
                &crate::verification::publisher::PreviewCommentContext {
                    run_id: &candidate.run_id,
                    attempt_id: &attempt_id,
                    pr_number: candidate.pr_number,
                    reviewed_head_sha: &pull.head.sha,
                    base_sha: &pull.base.sha,
                    spec_artifact_id: &candidate.spec_artifact_id,
                    implementation_artifact_id: &candidate.implementation_artifact_id,
                    review_artifact_id: &candidate.review_artifact_id,
                    gate: &gate,
                    commands: &command_runs,
                    evidence: &evidence_records,
                },
                self.config.max_pages,
            )
            .await?;

            // The stage completes only after the owned preview comment is
            // durable: a publication failure leaves the stage run non-terminal
            // so the next poll can retry the attempt instead of stranding a
            // completed stage with a pending intent and no comment.
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
                    let _ = self
                        .store
                        .fail_attempt(&stage.stage_run_id, factory_error.clone());
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
        attempt_id: &str,
        run_id: &str,
        workspace: &VerificationWorkspace,
        commands: &[VerificationCommandConfig],
        configuration_revision: &str,
        docker: Option<crate::domain::DockerConfig>,
    ) -> Result<(
        Vec<crate::verification::domain::VerificationCommandRunRecord>,
        bool,
    )> {
        let profile = if docker.is_some() {
            crate::implementation::domain::ExecutionProfile::Docker
        } else {
            crate::implementation::domain::ExecutionProfile::Local
        };
        let mut failed = false;
        let mut interrupted_index: Option<usize> = None;
        for (index, command) in commands.iter().enumerate() {
            let ordinal = (index + 1) as u32;
            let nonce = Uuid::now_v7().to_string();
            let sha256 = command_sha256(&command.command);
            let command_run_id = self.store.record_verification_command_launch(
                crate::triage::store::RecordVerificationCommandLaunchRequest {
                    run_id,
                    attempt_id,
                    ordinal,
                    name: &command.name,
                    kind: command.kind,
                    configuration_revision,
                    command_sha256: &sha256,
                    execution_profile: profile.as_str(),
                    launch_nonce: &nonce,
                },
            )?;
            if interrupted_index.is_some() {
                self.store
                    .mark_verification_command_not_run(&command_run_id)?;
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
                LaunchIdentity::Container { container_id } => store.cas_verification_container(
                    &launch_command_run_id,
                    &launch_nonce,
                    &container_id,
                ),
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
                Err(CommandRunFailure::SpawnError(message)) => {
                    self.store.complete_verification_command(
                        crate::triage::store::CompleteVerificationCommandRequest {
                            command_run_id: &command_run_id,
                            status: "interrupted",
                            exit_code: None,
                            termination_reason: Some("launch_failure"),
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
                Err(CommandRunFailure::NotSignalable(message))
                | Err(CommandRunFailure::StillRunning(message)) => {
                    // The persisted process group/container could not be
                    // terminated and verified: this is an infrastructure
                    // failure, not product evidence. Abort the pipeline and
                    // retain the workspace for diagnosis.
                    self.store.complete_verification_command(
                        crate::triage::store::CompleteVerificationCommandRequest {
                            command_run_id: &command_run_id,
                            status: "interrupted",
                            exit_code: None,
                            termination_reason: Some("termination_failed"),
                            passed: Some(false),
                            output_tail: None,
                            output_sha256: None,
                            started_at: Utc::now(),
                            completed_at: Utc::now(),
                            duration_ms: 0,
                        },
                    )?;
                    return Err(SymphonyError::TriageError(format!(
                        "verification command '{}' could not be terminated and verified: {message}",
                        command.name
                    )));
                }
            }
        }
        let records = self.store.list_verification_command_runs(attempt_id)?;
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
        let _ = self
            .store
            .interrupt_verification_stage_run(stage_run_id, &error);
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
#[allow(clippy::too_many_arguments)]
fn fail_verification_attempt(
    store: &SharedFactoryStore,
    stage_run_id: &str,
    attempt_id: &str,
    code: &str,
    message: String,
    workspace: &VerificationWorkspace,
    workspace_root: &Path,
    blocked: bool,
) {
    let mut store = store.clone();
    let factory_error = FactoryError::new(code, "verification_coordinator", message, false, None);
    let _ = store.fail_attempt(stage_run_id, factory_error.clone());
    let _ =
        store.update_verification_attempt(crate::triage::store::UpdateVerificationAttemptRequest {
            attempt_id,
            status: Some(if blocked { "blocked" } else { "failed" }),
            workspace_path: None,
            evidence_dir: None,
            error: Some(&factory_error),
        });
    if let Some(attempt_root) =
        attempt_root_for_cleanup(&workspace.workspace_path, workspace_root, attempt_id)
    {
        let _ = std::fs::remove_dir_all(&attempt_root);
    }
}

fn local_identity_from_attempt(
    attempt: &crate::verification::domain::VerificationAttemptRecord,
) -> Option<crate::triage::process_identity::ProcessIdentity> {
    let pid = attempt.verifier_pid?;
    let process_group_id = attempt.verifier_process_group_id?;
    if pid <= 0 || process_group_id <= 0 {
        return None;
    }
    Some(crate::triage::process_identity::ProcessIdentity {
        pid,
        process_group_id,
        start_token: attempt.verifier_start_token.clone(),
        executable: attempt.verifier_executable.clone(),
    })
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
    use crate::github::client::GithubIssueComment;
    use crate::verification::worker::VerificationWorkerResult;
    use async_trait::async_trait;

    fn open_store(path: &std::path::Path) -> SharedFactoryStore {
        SharedFactoryStore::open(path, 5_000).unwrap()
    }

    fn claim_request(issue_revision: &str, configuration_revision: &str) -> ClaimAttemptRequest {
        ClaimAttemptRequest {
            forge_host: "github.com".to_string(),
            repository: "owner/repo".to_string(),
            issue_id: "123".to_string(),
            issue_identifier: "#123".to_string(),
            issue_revision: issue_revision.to_string(),
            configuration_revision: configuration_revision.to_string(),
            owner_instance: "owner-1".to_string(),
            harness: "pi".to_string(),
            model: Some("model-a".to_string()),
            workspace_path: None,
            output_path: None,
            pid: None,
            process_group_id: None,
            process_start_token: None,
            executable_identity: None,
        }
    }

    /// Fake comment port mirroring the review publisher's fixture.
    struct FakeComments {
        login: String,
        comments: std::sync::Mutex<std::collections::HashMap<u64, GithubIssueComment>>,
        next_id: std::sync::Mutex<u64>,
        create_count: std::sync::Mutex<u32>,
        update_count: std::sync::Mutex<u32>,
    }

    impl FakeComments {
        fn new(login: &str) -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                login: login.to_string(),
                comments: std::sync::Mutex::new(std::collections::HashMap::new()),
                next_id: std::sync::Mutex::new(700),
                create_count: std::sync::Mutex::new(0),
                update_count: std::sync::Mutex::new(0),
            })
        }

        fn comment(&self, id: u64) -> Option<GithubIssueComment> {
            self.comments.lock().unwrap().get(&id).cloned()
        }

        fn counts(&self) -> (u32, u32) {
            (
                *self.create_count.lock().unwrap(),
                *self.update_count.lock().unwrap(),
            )
        }

        fn seed_owned_marker(&self, marker: &str, body: &str) -> u64 {
            let mut comments = self.comments.lock().unwrap();
            let id = *self.next_id.lock().unwrap();
            *self.next_id.lock().unwrap() += 1;
            comments.insert(
                id,
                GithubIssueComment {
                    id,
                    user: Some(crate::github::client::GithubUser {
                        login: self.login.clone(),
                    }),
                    body: Some(format!("{body}\n{marker}")),
                    html_url: None,
                    created_at: None,
                    updated_at: None,
                },
            );
            id
        }
    }

    #[async_trait]
    impl TriageCommentPort for std::sync::Arc<FakeComments> {
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
            self.comment(comment_id)
                .ok_or_else(|| SymphonyError::GithubApiRequest("missing comment".to_string()))
        }

        async fn create_comment(
            &self,
            _issue_number: u64,
            body: &str,
        ) -> Result<GithubIssueComment> {
            let mut comments = self.comments.lock().unwrap();
            let id = *self.next_id.lock().unwrap();
            *self.next_id.lock().unwrap() += 1;
            *self.create_count.lock().unwrap() += 1;
            let comment = GithubIssueComment {
                id,
                user: Some(crate::github::client::GithubUser {
                    login: self.login.clone(),
                }),
                body: Some(body.to_string()),
                html_url: None,
                created_at: None,
                updated_at: None,
            };
            comments.insert(id, comment.clone());
            Ok(comment)
        }

        async fn update_comment(&self, comment_id: u64, body: &str) -> Result<GithubIssueComment> {
            let mut comments = self.comments.lock().unwrap();
            *self.update_count.lock().unwrap() += 1;
            let mut comment = comments
                .get(&comment_id)
                .cloned()
                .ok_or_else(|| SymphonyError::GithubApiRequest("missing comment".to_string()))?;
            comment.body = Some(body.to_string());
            comments.insert(comment_id, comment.clone());
            Ok(comment)
        }
    }

    #[derive(Default)]
    struct FakeWorker;

    #[async_trait]
    impl VerificationWorker for FakeWorker {
        async fn run(
            &self,
            _request: &VerificationWorkerRequest,
        ) -> Result<VerificationWorkerResult> {
            Err(SymphonyError::TriageError("fake worker".to_string()))
        }
    }

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

    #[test]
    fn recovery_interrupts_abandoned_attempts_and_keeps_unsignalable_ones() {
        let temp = tempfile::tempdir().unwrap();
        let store = open_store(&temp.path().join("factory.db"));
        store.disable_foreign_keys_for_test();
        let config = VerificationCoordinatorConfig {
            forge_host: "github.com".to_string(),
            repository: "owner/repo".to_string(),
            owner_instance: "owner-1".to_string(),
            workflow_dir: temp.path().to_path_buf(),
            project_owner: "owner".to_string(),
            project_number: 16,
            max_pages: 5,
            workspace_root: temp.path().join("workspaces"),
        };
        let comments = FakeComments::new("symphony-bot");
        let mut coordinator = VerificationCoordinator::new(
            store.clone(),
            comments,
            GithubClient::with_base_url(
                "token".to_string(),
                "owner".to_string(),
                "repo".to_string(),
                "symphony".to_string(),
                "https://api.github.com",
            ),
            ProjectsV2Client::new(GithubClient::with_base_url(
                "token".to_string(),
                "owner".to_string(),
                "repo".to_string(),
                "symphony".to_string(),
                "https://api.github.com",
            )),
            FakeWorker,
            config,
        );

        // An abandoned attempt with a recorded but dead process identity.
        let stage = store
            .claim_verification_attempt(claim_request("rev", "cfg"))
            .unwrap();
        store
            .store_verification_attempt_inputs(
                crate::triage::store::StoreVerificationAttemptRequest {
                    attempt_id: "attempt-dead".to_string(),
                    stage_run_id: stage.stage_run_id.clone(),
                    pr_number: 42,
                    reviewed_head_sha: "head-sha".to_string(),
                    base_sha: "base-sha".to_string(),
                    spec_artifact_id: "spec".to_string(),
                    implementation_artifact_id: "implementation".to_string(),
                    review_artifact_id: "review".to_string(),
                    configuration_revision: "cfg-rev".to_string(),
                    execution_profile: "local".to_string(),
                },
            )
            .unwrap();
        let command_run_id = store
            .record_verification_command_launch(
                crate::triage::store::RecordVerificationCommandLaunchRequest {
                    run_id: &stage.run_id,
                    attempt_id: "attempt-dead",
                    ordinal: 1,
                    name: "unit",
                    kind: crate::verification::domain::VerificationCommandKind::Test,
                    configuration_revision: "cfg-rev",
                    command_sha256: "sha",
                    execution_profile: "local",
                    launch_nonce: "nonce",
                },
            )
            .unwrap();
        let identity = crate::triage::process_identity::ProcessIdentity {
            pid: 4_200_000_000,
            process_group_id: 4_200_000_000,
            start_token: Some("token".to_string()),
            executable: Some("/bin/sh".to_string()),
        };
        store
            .cas_verification_launch_identity(&command_run_id, "nonce", &identity)
            .unwrap();
        store
            .update_verification_attempt(crate::triage::store::UpdateVerificationAttemptRequest {
                attempt_id: "attempt-dead",
                status: None,
                workspace_path: Some(
                    temp.path()
                        .join("workspaces/verification-attempt-dead/workspace")
                        .to_str()
                        .unwrap(),
                ),
                evidence_dir: None,
                error: None,
            })
            .unwrap();

        // Disabled verification still runs recovery. The recorded identity is
        // not signalable (invalid pid), so the attempt stays RUNNING and the
        // stage pin holds: no new claim can race the surviving process.
        let summary = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(coordinator.poll_once(&ServiceConfig::default()))
            .unwrap();
        assert_eq!(summary.verification_enabled, false);
        assert_eq!(summary.recovered, 1);
        let attempt = store
            .get_verification_attempt("attempt-dead")
            .unwrap()
            .unwrap();
        assert_eq!(
            attempt.status, "running",
            "an unsignalable process must keep the attempt running"
        );
        let runs = store
            .list_verification_command_runs("attempt-dead")
            .unwrap();
        assert_eq!(runs[0].status, "interrupted");
        assert_eq!(
            runs[0].termination_reason.as_deref(),
            Some("restart_recovery")
        );
    }

    #[tokio::test]
    async fn preview_publication_creates_once_and_adopts_the_owned_marker() {
        let temp = tempfile::tempdir().unwrap();
        let store = open_store(&temp.path().join("factory.db"));
        store.disable_foreign_keys_for_test();
        let comments = FakeComments::new("symphony-bot");
        let gate = crate::verification::gate::compute_gate(
            &crate::verification::domain::VerifierManifest {
                schema_version: 1,
                spec_artifact_id: "spec".to_string(),
                implementation_artifact_id: "implementation".to_string(),
                review_artifact_id: "review".to_string(),
                reviewed_head_sha: "head-sha".to_string(),
                base_sha: "base-sha".to_string(),
                summary: "verified".to_string(),
                criteria: vec![],
            },
            &GateIdentity {
                spec_artifact_id: "spec",
                implementation_artifact_id: "implementation",
                review_artifact_id: "review",
                reviewed_head_sha: "head-sha",
                base_sha: "base-sha",
                criterion_count: 0,
            },
            &[],
            &[],
        )
        .unwrap();
        let gate = VerificationGateRecord {
            gate_id: "gate-1".to_string(),
            run_id: "run-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            status: if gate.passed { "passed" } else { "failed" }.to_string(),
            verifier_manifest: None,
            command_summary: Some(serde_json::json!({"reasons": gate.reasons})),
            computed_at: Some(Utc::now()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let intent = store
            .create_verification_publication_intent("run-1", "attempt-1", "preview")
            .unwrap();
        let context = crate::verification::publisher::PreviewCommentContext {
            run_id: "run-1",
            attempt_id: "attempt-1",
            pr_number: 42,
            reviewed_head_sha: "head-sha",
            base_sha: "base-sha",
            spec_artifact_id: "spec",
            implementation_artifact_id: "implementation",
            review_artifact_id: "review",
            gate: &gate,
            commands: &[],
            evidence: &[],
        };

        // First publication creates exactly one comment.
        let first = publish_preview_comment(&comments, &store, &intent, &context, 5)
            .await
            .unwrap();
        let (creates, updates) = comments.counts();
        assert_eq!(creates, 1);
        assert_eq!(updates, 0);
        assert!(comments.comment(first.parse().unwrap()).is_some());

        // A second publication for the same intent updates the owned marker
        // comment in place instead of creating a duplicate.
        let again = publish_preview_comment(&comments, &store, &intent, &context, 5)
            .await
            .unwrap();
        assert_eq!(again, first);
        let (creates, updates) = comments.counts();
        assert_eq!(creates, 1);
        assert_eq!(updates, 1);

        // A foreign marker comment is never adopted: the publisher login is
        // symphony-bot but the marker comment belongs to another author.
        let intent2 = store
            .create_verification_publication_intent("run-1", "attempt-2", "preview")
            .unwrap();
        let foreign = FakeComments::new("symphony-bot");
        let foreign_owned = {
            let mut comments = foreign.comments.lock().unwrap();
            let id = *foreign.next_id.lock().unwrap();
            *foreign.next_id.lock().unwrap() += 1;
            comments.insert(
                id,
                GithubIssueComment {
                    id,
                    user: Some(crate::github::client::GithubUser {
                        login: "intruder".to_string(),
                    }),
                    body: Some(format!(
                        "foreign body\n{}",
                        crate::verification::publisher::verification_marker("run-1", "head-sha")
                    )),
                    html_url: None,
                    created_at: None,
                    updated_at: None,
                },
            );
            id
        };
        let context2 = crate::verification::publisher::PreviewCommentContext {
            run_id: "run-1",
            attempt_id: "attempt-2",
            pr_number: 42,
            reviewed_head_sha: "head-sha",
            base_sha: "base-sha",
            spec_artifact_id: "spec",
            implementation_artifact_id: "implementation",
            review_artifact_id: "review",
            gate: &gate,
            commands: &[],
            evidence: &[],
        };
        let error = publish_preview_comment(&foreign, &store, &intent2, &context2, 5)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("owned by another GitHub login"));
        assert!(foreign.comment(foreign_owned).is_some());
    }

    /// Fake verifier that emits a strict manifest derived from its request:
    /// every approved criterion is `pass` with the first evidence reference
    /// when evidence exists, otherwise `not_proven`.
    #[derive(Default)]
    struct FakeManifestWorker;

    #[async_trait]
    impl VerificationWorker for FakeManifestWorker {
        async fn run(
            &self,
            request: &VerificationWorkerRequest,
        ) -> Result<VerificationWorkerResult> {
            let criteria = request
                .approved_spec
                .acceptance_criteria
                .iter()
                .enumerate()
                .map(
                    |(index, _)| crate::verification::domain::VerifierCriterion {
                        index: (index + 1) as u32,
                        status: if request.evidence.is_empty() {
                            crate::verification::domain::VerifierCriterionStatus::NotProven
                        } else {
                            crate::verification::domain::VerifierCriterionStatus::Pass
                        },
                        rationale: "fake worker assessment".to_string(),
                        evidence: request
                            .evidence
                            .first()
                            .map(|record| vec![record.relative_path.clone()])
                            .unwrap_or_default(),
                    },
                )
                .collect();
            let manifest = crate::verification::domain::VerifierManifest {
                schema_version: 1,
                spec_artifact_id: request.spec_artifact_id.clone(),
                implementation_artifact_id: request.implementation_artifact_id.clone(),
                review_artifact_id: request.review_artifact.artifact_id.clone(),
                reviewed_head_sha: request.review_artifact.reviewed_head_sha.clone(),
                base_sha: request.review_artifact.base_sha.clone(),
                summary: "fake verification".to_string(),
                criteria,
            };
            let output_bytes = serde_json::to_vec(&manifest).unwrap();
            Ok(VerificationWorkerResult {
                output_bytes,
                usage: StageUsage::default(),
            })
        }
    }

    fn init_git_repo() -> (tempfile::TempDir, tempfile::TempDir, String) {
        use std::process::Command;
        let bare = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        for args in [["init", "-q", "--bare", "-b", "main"].as_slice()] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(bare.path())
                .status()
                .unwrap()
                .success());
        }
        for args in [
            ["init", "-q", "-b", "main"].as_slice(),
            ["config", "user.email", "t@example.com"].as_slice(),
            ["config", "user.name", "T"].as_slice(),
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success());
        }
        let repo_path = repo.path();
        std::fs::write(repo_path.join("README.md"), "base\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "README.md"])
            .current_dir(repo_path)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-qm", "init"])
            .current_dir(repo_path)
            .status()
            .unwrap()
            .success());
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        // `origin` remote used by the pull-ref fetch; seed refs/pull/63/head.
        assert!(Command::new("git")
            .args(["remote", "add", "origin", bare.path().to_str().unwrap()])
            .current_dir(repo_path)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["push", "-q", "origin", "main:refs/heads/main"])
            .current_dir(repo_path)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["push", "-q", "origin", "main:refs/pull/63/head",])
            .current_dir(repo_path)
            .status()
            .unwrap()
            .success());
        (bare, repo, head)
    }

    fn seed_full_pipeline_run(store: &SharedFactoryStore, head: &str, base: &str) {
        let now = Utc::now().to_rfc3339();
        store.disable_foreign_keys_for_test();
        let spec_json = serde_json::json!({
            "schema_version": 1,
            "product_behavior": "add a heading",
            "technical_approach": "edit README",
            "acceptance_criteria": ["heading present"],
            "open_decisions": [],
        })
        .to_string();
        store.execute_batch_for_test(&format!(
            "INSERT INTO factory_runs (run_id, forge_host, repository, issue_id, issue_identifier, issue_revision, status, current_stage, created_at, updated_at)
             VALUES ('run-review', 'github.com', 'owner/repo', '63', '#63', 'issue-rev', 'running', 'review', '{now}', '{now}');
             INSERT INTO spec_run_state (run_id, approved_artifact_id, approved_version, decision, updated_at)
             VALUES ('run-review', 'spec-artifact', 1, 'approved', '{now}');
             INSERT INTO spec_artifacts (artifact_id, run_id, stage_run_id, issue_revision, configuration_revision, version, schema_version, artifact_json, review_cycles, unresolved_findings_json, received_at, bytes_len)
             VALUES ('spec-artifact', 'run-review', 'spec-stage', 'issue-rev', 'config-rev', 1, 1, '{spec_json}', 1, '[]', '{now}', 1);
             INSERT INTO implementation_artifacts (artifact_id, run_id, stage_run_id, approved_artifact_id, approved_version, issue_revision, configuration_revision, schema_version, manifest_json, base_commit, approved_spec_path, validation_cycles, execution_profile, received_at, bytes_len)
             VALUES ('implementation-artifact', 'run-review', 'implementation-stage', 'spec-artifact', 1, 'issue-rev', 'config-rev', 1, '{{\"schema_version\":1,\"status\":\"completed\",\"summary\":\"s\",\"acceptance_criteria\":[],\"known_limitations\":[]}}', 'base', 'spec.md', 1, 'local', '{now}', 1);
             INSERT INTO implementation_publication_intents (intent_id, run_id, artifact_id, kind, status, completed_steps_json, desired_effects_json, observed_baseline_json, expected_projection_json, created_at, updated_at)
             VALUES ('implementation-intent', 'run-review', 'implementation-artifact', 'draft_pr', 'applied', '[]', '{{}}', '{{}}', '{{}}', '{now}', '{now}');
             INSERT INTO implementation_draft_pr_artifacts (artifact_id, run_id, implementation_artifact_id, intent_id, number, url, draft, head, base, head_sha, marker, created_at)
             VALUES ('draft-artifact', 'run-review', 'implementation-artifact', 'implementation-intent', 63, 'https://github.com/owner/repo/pull/63', 1, 'feature', 'main', '{head}', 'marker', '{now}');
             INSERT INTO review_attempts (attempt_id, run_id, stage_run_id, draft_pr_artifact_id, implementation_artifact_id, spec_artifact_id, pr_number, reviewed_head_sha, base_sha, status, created_at, updated_at)
             VALUES ('review-attempt', 'run-review', 'review-stage', 'draft-artifact', 'implementation-artifact', 'spec-artifact', 63, '{head}', '{base}', 'completed', '{now}', '{now}');
             INSERT INTO review_findings_artifacts (artifact_id, run_id, stage_run_id, attempt_id, draft_pr_artifact_id, implementation_artifact_id, spec_artifact_id, schema_version, reviewed_head_sha, base_sha, manifest_json, no_findings, finding_count, received_at, bytes_len)
             VALUES ('review-artifact', 'run-review', 'review-stage', 'review-attempt', 'draft-artifact', 'implementation-artifact', 'spec-artifact', 1, '{head}', '{base}', '{{\"schema_version\":1,\"reviewed_head_sha\":\"{head}\",\"base_sha\":\"{base}\",\"spec_conformance_summary\":\"none\",\"no_findings\":true,\"findings\":[]}}', 1, 0, '{now}', 2);
             INSERT INTO review_publication_intents (intent_id, run_id, artifact_id, kind, status, completed_steps_json, retry_count, route_state, desired_effects_json, observed_baseline_json, expected_projection_json, created_at, updated_at)
             VALUES ('automatic-intent', 'run-review', 'review-artifact', 'automatic', 'applied', '[]', 0, 'Human Review', '{{}}', '{{}}', '{{}}', '{now}', '{now}');
            "
        ));
    }

    #[tokio::test]
    async fn process_candidate_runs_the_full_pipeline_against_mock_github() {
        use mockito::Server;
        let (_bare, repo, head) = init_git_repo();
        let base = "base-sha";

        let mut server = Server::new_async().await;
        // Pull endpoint: open PR whose head/base match the reviewed revision.
        let pull_json = serde_json::json!({
            "number": 63,
            "html_url": "https://github.com/owner/repo/pull/63",
            "draft": true,
            "state": "open",
            "title": "UAT",
            "head": {"ref": "feature", "sha": head},
            "base": {"ref": "main", "sha": base},
        })
        .to_string();
        let pull_mock = server
            .mock("GET", "/repos/owner/repo/pulls/63")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(pull_json)
            .expect_at_least(1)
            .create_async()
            .await;

        // Projects v2 GraphQL: status field resolution.
        let fields_json = serde_json::json!({
            "data": {
                "user": {
                    "projectV2": {
                        "id": "project-1",
                        "field": {
                            "id": "field-1",
                            "options": [
                                {"id": "opt-todo", "name": "Todo"},
                                {"id": "opt-verification", "name": "Human Review"}
                            ]
                        }
                    }
                },
                "organization": null
            }
        })
        .to_string();
        let fields_mock = server
            .mock("POST", "/graphql")
            .match_body(mockito::Matcher::Regex(r"field\(name:".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(fields_json)
            .create_async()
            .await;

        // Projects v2 GraphQL: items query returns issue #63 in Human Review.
        let items_json = serde_json::json!({
            "data": {
                "node": {
                    "items": {
                        "nodes": [{
                            "id": "item-1",
                            "content": {
                                "number": 63,
                                "repository": {"name": "repo", "owner": {"login": "owner"}},
                                "blockedBy": {"nodes": []}
                            },
                            "status": {"name": "Human Review", "optionId": "opt-verification"},
                            "kataId": null
                        }],
                        "pageInfo": {"hasNextPage": false, "endCursor": null}
                    }
                }
            }
        })
        .to_string();
        let items_mock = server
            .mock("POST", "/graphql")
            .match_body(mockito::Matcher::Regex("fieldValueByName".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(items_json)
            .create_async()
            .await;

        let temp = tempfile::tempdir().unwrap();
        let store = open_store(&temp.path().join("factory.db"));
        seed_full_pipeline_run(&store, &head, base);
        let comments = FakeComments::new("symphony-bot");
        let github = GithubClient::with_base_url(
            "token".to_string(),
            "owner".to_string(),
            "repo".to_string(),
            "symphony".to_string(),
            &server.url(),
        );
        let mut coordinator = VerificationCoordinator::new(
            store.clone(),
            comments,
            github.clone(),
            ProjectsV2Client::new(github),
            FakeManifestWorker,
            VerificationCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "owner-1".to_string(),
                workflow_dir: temp.path().to_path_buf(),
                project_owner: "owner".to_string(),
                project_number: 16,
                max_pages: 5,
                workspace_root: temp.path().join("workspaces"),
            },
        );

        let service = ServiceConfig {
            verification: VerificationConfig {
                enabled: true,
                mode: "preview".to_string(),
                prompt: "prompts/verification.md".to_string(),
                model: Some("deepseek/deepseek-v4-flash".to_string()),
                max_turns: 1,
                invocation_timeout_ms: 60_000,
                max_attempts: 3,
                max_reprompts: 0,
                max_evidence_files: 100,
                max_evidence_bytes: 1024 * 1024,
                trigger_state: "Human Review".to_string(),
                commands: vec![
                    crate::verification::domain::VerificationCommandConfig {
                        name: "affected-validation".to_string(),
                        kind: crate::verification::domain::VerificationCommandKind::Test,
                        command:
                            "test -f README.md && printf 'ok\n' > \"$SYMPHONY_EVIDENCE_DIR/repo-smoke.txt\""
                                .to_string(),
                        timeout_ms: 60_000,
                    },
                    crate::verification::domain::VerificationCommandConfig {
                        name: "product-acceptance".to_string(),
                        kind: crate::verification::domain::VerificationCommandKind::Acceptance,
                        command: "test -f README.md".to_string(),
                        timeout_ms: 60_000,
                    },
                ],
            },
            workspace: crate::domain::WorkspaceConfig {
                root: temp.path().join("workspaces").display().to_string(),
                repo: Some(repo.path().display().to_string()),
                ..crate::domain::WorkspaceConfig::default()
            },
            storage: crate::triage::domain::StorageConfig {
                path: Some(temp.path().join("factory.db").display().to_string()),
                ..crate::triage::domain::StorageConfig::default()
            },
            ..ServiceConfig::default()
        };

        // The prompt file and workspace root must exist.
        std::fs::create_dir_all(temp.path().join("workspaces")).unwrap();
        std::fs::create_dir_all(temp.path().join("prompts")).unwrap();
        std::fs::write(
            temp.path().join("prompts/verification.md"),
            "You are the verifier.\n",
        )
        .unwrap();

        let eligible = store.list_a5_eligible_verification_runs(3).unwrap();
        assert_eq!(eligible.len(), 1, "one eligible run expected in store");
        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.candidates_seen, 1, "one eligible run expected");
        assert_eq!(summary.attempts_started, 1);
        assert_eq!(summary.attempts_completed, 1);
        assert_eq!(summary.preview_published, 1);

        let attempts = store.list_verification_attempts("run-review").unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].status, "completed");
        let gate = store
            .get_verification_gate(&attempts[0].attempt_id)
            .unwrap()
            .expect("gate must be recorded");
        assert_eq!(gate.status, "passed");
        let evidence = store
            .list_verification_evidence(&attempts[0].attempt_id)
            .unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].relative_path, "repo-smoke.txt");
        let intents = store
            .list_verification_publications_for_run("run-review")
            .unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].status.as_str(), "applied");

        pull_mock.assert_async().await;
        fields_mock.assert_async().await;
        items_mock.assert_async().await;
    }

    #[tokio::test]
    async fn verifier_failure_after_reprompts_records_a_blocked_attempt() {
        use mockito::Server;
        let (_bare, repo, head) = init_git_repo();
        let base = "base-sha";

        let mut server = Server::new_async().await;
        let pull_json = serde_json::json!({
            "number": 63,
            "html_url": "https://github.com/owner/repo/pull/63",
            "draft": true,
            "state": "open",
            "title": "UAT",
            "head": {"ref": "feature", "sha": head},
            "base": {"ref": "main", "sha": base},
        })
        .to_string();
        let _pull_mock = server
            .mock("GET", "/repos/owner/repo/pulls/63")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(pull_json)
            .expect_at_least(1)
            .create_async()
            .await;
        let fields_json = serde_json::json!({
            "data": {
                "user": {"projectV2": {"id": "project-1", "field": {"id": "field-1", "options": [{"id": "opt-verification", "name": "Human Review"}]}}},
                "organization": null
            }
        })
        .to_string();
        let _fields_mock = server
            .mock("POST", "/graphql")
            .match_body(mockito::Matcher::Regex(r"field\(name:".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(fields_json)
            .expect_at_least(1)
            .create_async()
            .await;
        let items_json = serde_json::json!({
            "data": {
                "node": {
                    "items": {
                        "nodes": [{
                            "id": "item-1",
                            "content": {
                                "number": 63,
                                "repository": {"name": "repo", "owner": {"login": "owner"}},
                                "blockedBy": {"nodes": []}
                            },
                            "status": {"name": "Human Review", "optionId": "opt-verification"},
                            "kataId": null
                        }],
                        "pageInfo": {"hasNextPage": false, "endCursor": null}
                    }
                }
            }
        })
        .to_string();
        let _items_mock = server
            .mock("POST", "/graphql")
            .match_body(mockito::Matcher::Regex("fieldValueByName".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(items_json)
            .expect_at_least(1)
            .create_async()
            .await;

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("workspaces")).unwrap();
        std::fs::create_dir_all(temp.path().join("prompts")).unwrap();
        std::fs::write(
            temp.path().join("prompts/verification.md"),
            "You are the verifier.\n",
        )
        .unwrap();
        let store = open_store(&temp.path().join("factory.db"));
        seed_full_pipeline_run(&store, &head, base);
        let comments = FakeComments::new("symphony-bot");
        let github = GithubClient::with_base_url(
            "token".to_string(),
            "owner".to_string(),
            "repo".to_string(),
            "symphony".to_string(),
            &server.url(),
        );
        let mut coordinator = VerificationCoordinator::new(
            store.clone(),
            comments,
            github.clone(),
            ProjectsV2Client::new(github),
            FakeWorker,
            VerificationCoordinatorConfig {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                owner_instance: "owner-1".to_string(),
                workflow_dir: temp.path().to_path_buf(),
                project_owner: "owner".to_string(),
                project_number: 16,
                max_pages: 5,
                workspace_root: temp.path().join("workspaces"),
            },
        );
        let service = ServiceConfig {
            verification: VerificationConfig {
                enabled: true,
                mode: "preview".to_string(),
                prompt: "prompts/verification.md".to_string(),
                max_turns: 1,
                model: None,
                invocation_timeout_ms: 60_000,
                max_attempts: 3,
                max_reprompts: 1,
                max_evidence_files: 100,
                max_evidence_bytes: 1024 * 1024,
                trigger_state: "Human Review".to_string(),
                commands: vec![crate::verification::domain::VerificationCommandConfig {
                    name: "product-acceptance".to_string(),
                    kind: crate::verification::domain::VerificationCommandKind::Acceptance,
                    command: "test -f README.md".to_string(),
                    timeout_ms: 60_000,
                }],
            },
            workspace: crate::domain::WorkspaceConfig {
                root: temp.path().join("workspaces").display().to_string(),
                repo: Some(repo.path().display().to_string()),
                ..crate::domain::WorkspaceConfig::default()
            },
            storage: crate::triage::domain::StorageConfig {
                path: Some(temp.path().join("factory.db").display().to_string()),
                ..crate::triage::domain::StorageConfig::default()
            },
            ..ServiceConfig::default()
        };

        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.candidates_seen, 1);
        assert_eq!(summary.attempts_failed, 1);
        let attempts = store.list_verification_attempts("run-review").unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].status, "blocked",
            "verifier exhaustion after max_reprompts must record the distinct blocked outcome"
        );
        assert_eq!(
            attempts[0].error.as_ref().map(|error| error.code.as_str()),
            Some("verification_verifier_failed")
        );
        // Blocked attempts consume the retry budget: the run stays eligible
        // (1 < max_attempts) but every claim records another blocked attempt
        // instead of auto-completing.
        let summary = coordinator.poll_once(&service).await.unwrap();
        assert_eq!(summary.candidates_seen, 1);
        assert_eq!(summary.attempts_failed, 1);
        let attempts = store.list_verification_attempts("run-review").unwrap();
        assert_eq!(attempts.len(), 2);
        assert!(attempts.iter().all(|attempt| attempt.status == "blocked"));
    }

    #[test]
    fn prompt_and_path_resolution_helpers_fail_actionably() {
        let temp = tempfile::tempdir().unwrap();
        let config = VerificationConfig::default();
        let service = ServiceConfig {
            verification: config.clone(),
            ..ServiceConfig::default()
        };
        // Missing prompt file errors with the resolved path.
        let error = load_verification_prompt(temp.path(), &service).unwrap_err();
        assert!(error.to_string().contains("could not be read"));

        // workspace.repo must resolve; workspace.root must exist.
        let missing_root = temp.path().join("does-not-exist");
        let service = ServiceConfig {
            verification: config,
            workspace: crate::domain::WorkspaceConfig {
                root: missing_root.display().to_string(),
                ..crate::domain::WorkspaceConfig::default()
            },
            ..ServiceConfig::default()
        };
        let error = resolve_repo_path(&service).unwrap_err();
        assert!(error.to_string().contains("workspace.repo"));
        let error = resolve_workspace_root(&service).unwrap_err();
        assert!(error.to_string().contains("workspace.root"));
    }
}
