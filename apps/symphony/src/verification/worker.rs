//! Credential-isolated A5 verifier invocation.
//!
//! The read-only verifier receives the pinned A2 spec, A3 implementation
//! claims, A4 findings, recorded command results, and stored evidence metadata
//! for the current attempt — nothing else. It writes one strict criterion
//! matrix; Symphony computes the gate from durable command outcomes plus
//! complete criterion coverage.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::domain::{AgentBackend, CodexConfig, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::implementation::domain::ImplementationManifest;
use crate::review::domain::ReviewFindingsArtifactRecord;
use crate::spec::domain::SpecArtifact;
use crate::triage::domain::StageUsage;
use crate::triage::runner::{
    run_isolated_raw_turn, TriageHarness, TriageIssueIdentity, TriageRunnerRequest, TriageSpawnSink,
};
use crate::verification::domain::{
    VerificationCommandRunRecord, VerificationConfig, VerificationEvidenceRecord,
};

#[derive(Clone)]
pub struct VerificationWorkerRequest {
    pub attempt_id: String,
    pub workspace_root: PathBuf,
    pub repo_path: PathBuf,
    pub command: Vec<String>,
    pub prompt: String,
    pub config: VerificationConfig,
    pub model: Option<String>,
    pub harness: TriageHarness,
    pub issue: TriageIssueIdentity,
    pub codex: Option<CodexConfig>,
    pub approved_spec: SpecArtifact,
    pub implementation_manifest: ImplementationManifest,
    pub review_artifact: ReviewFindingsArtifactRecord,
    pub command_runs: Vec<VerificationCommandRunRecord>,
    pub evidence: Vec<VerificationEvidenceRecord>,
    /// Fired with the verifier's process identity as soon as the child exists,
    /// so the attempt record stays durable for restart recovery.
    pub spawned: Option<TriageSpawnSink>,
}

#[derive(Debug, Clone)]
pub struct VerificationWorkerResult {
    pub output_bytes: Vec<u8>,
    pub usage: StageUsage,
}

#[async_trait]
pub trait VerificationWorker: Send + Sync {
    async fn run(&self, request: &VerificationWorkerRequest) -> Result<VerificationWorkerResult>;
}

/// Live verifier backed by the existing A1 Pi/Codex isolated process profile.
/// The child environment omits forge, tracker, SSH, helper, push, approval,
/// merge, and deployment credentials, and the prompt forbids any mutation.
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveVerificationWorker;

#[async_trait]
impl VerificationWorker for LiveVerificationWorker {
    async fn run(&self, request: &VerificationWorkerRequest) -> Result<VerificationWorkerResult> {
        let commands = request
            .command_runs
            .iter()
            .map(|run| {
                serde_json::json!({
                    "name": run.name,
                    "kind": run.kind.as_str(),
                    "ordinal": run.ordinal,
                    "status": run.status,
                    "exit_code": run.exit_code,
                    "passed": run.passed,
                    "termination_reason": run.termination_reason,
                    "duration_ms": run.duration_ms,
                    "output_sha256": run.output_sha256,
                })
            })
            .collect::<Vec<_>>();
        let evidence = request
            .evidence
            .iter()
            .map(|record| {
                serde_json::json!({
                    "relative_path": record.relative_path,
                    "sha256": record.sha256,
                    "bytes_len": record.bytes_len,
                })
            })
            .collect::<Vec<_>>();
        let context = serde_json::json!({
            "approved_spec": &request.approved_spec,
            "implementation_manifest": &request.implementation_manifest,
            "review_manifest": &request.review_artifact.manifest,
            "command_runs": commands,
            "evidence": evidence,
            "attempt_id": &request.attempt_id,
        });
        let prompt = format!(
            "{}\n\nA5 verifier contract: read only the JSON context in the stage-input directory and the cloned repository. Assess whether the A3 implementation satisfies every acceptance criterion of the approved A2 spec, using the A4 findings, the recorded command results, and the evidence metadata. Write ONE strict JSON verification manifest to $SYMPHONY_STAGE_OUTPUT with schema_version 1, spec_artifact_id, implementation_artifact_id, review_artifact_id, reviewed_head_sha, base_sha, a summary, and a criteria array. Every criterion needs an index, status (pass|fail|not_proven), a non-empty rationale, and for pass/fail at least one evidence reference that names a relative_path exactly as listed in the evidence array. Never reference evidence that is not listed. Do not modify the repository, invoke forge or tracker APIs, push, approve, merge, or emit prose outside the output file.",
            request.prompt
        );
        let raw_request = TriageRunnerRequest {
            attempt_id: request.attempt_id.clone(),
            workspace_root: request.workspace_root.clone(),
            repo_path: request.repo_path.clone(),
            command: request.command.clone(),
            prompt,
            turn_timeout_ms: request.config.invocation_timeout_ms,
            model: request.model.clone(),
            harness: request.harness,
            issue: request.issue.clone(),
            codex: request.codex.clone(),
            progress: None,
            spawned: request.spawned.clone(),
        };
        let raw = run_isolated_raw_turn(&raw_request, &context).await?;
        Ok(VerificationWorkerResult {
            output_bytes: raw.output_bytes,
            usage: raw.usage,
        })
    }
}

pub fn harness_for_service(service: &ServiceConfig) -> TriageHarness {
    match service.agent_backend {
        AgentBackend::Codex => TriageHarness::Codex,
        AgentBackend::KataCli => TriageHarness::Pi,
    }
}

pub fn model_for_verification(service: &ServiceConfig) -> Option<String> {
    if service.agent_backend == AgentBackend::Codex {
        None
    } else {
        service
            .verification
            .model
            .as_deref()
            .or(service.pi_agent.model.as_deref())
            .map(str::to_string)
    }
}

pub fn command_for_verification(service: &ServiceConfig) -> Result<Vec<String>> {
    let command = match service.agent_backend {
        AgentBackend::Codex => service.codex.command.clone(),
        AgentBackend::KataCli => service.pi_agent.command.clone(),
    };
    if command.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "verification worker command is empty".to_string(),
        ));
    }
    Ok(command)
}
