//! Credential-isolated A4 review worker invocation.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::domain::{AgentBackend, CodexConfig, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::implementation::domain::ImplementationManifest;
use crate::review::domain::ReviewConfig;
use crate::spec::domain::SpecArtifact;
use crate::triage::domain::StageUsage;
use crate::triage::runner::{
    run_isolated_raw_turn, TriageHarness, TriageIssueIdentity, TriageRunnerRequest,
};

#[derive(Debug, Clone)]
pub struct ReviewWorkerRequest {
    pub attempt_id: String,
    pub workspace_root: PathBuf,
    pub repo_path: PathBuf,
    pub command: Vec<String>,
    pub prompt: String,
    pub config: ReviewConfig,
    pub model: Option<String>,
    pub harness: TriageHarness,
    pub issue: TriageIssueIdentity,
    pub codex: Option<CodexConfig>,
    pub diff: String,
    pub pull_request_body: String,
    pub approved_spec: SpecArtifact,
    pub implementation_manifest: ImplementationManifest,
}

#[derive(Debug, Clone)]
pub struct ReviewWorkerResult {
    pub output_bytes: Vec<u8>,
    pub usage: StageUsage,
}

#[async_trait]
pub trait ReviewWorker: Send + Sync {
    async fn run(&self, request: &ReviewWorkerRequest) -> Result<ReviewWorkerResult>;
}

/// Live worker backed by the existing A1 Pi/Codex isolated process profile.
/// The child environment intentionally omits forge, tracker, SSH, helper, and
/// push credentials. Repository integrity is checked after every turn.
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveReviewWorker;

#[async_trait]
impl ReviewWorker for LiveReviewWorker {
    async fn run(&self, request: &ReviewWorkerRequest) -> Result<ReviewWorkerResult> {
        let context = serde_json::json!({
            "diff": &request.diff,
            "pull_request_body": &request.pull_request_body,
            "approved_spec": &request.approved_spec,
            "implementation_manifest": &request.implementation_manifest,
        });
        let prompt = format!(
            "{}\n\nA4 worker contract: read only the JSON context in the stage-input directory and the cloned repository. Use your file-write tool to write one strict JSON review findings manifest to the exact path in $SYMPHONY_STAGE_OUTPUT (do not merely print it in your reply). Do not modify the repository, invoke forge or tracker APIs, push, approve, merge, or emit prose outside the output file.",
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
            spawned: None,
        };
        let raw = run_isolated_raw_turn(&raw_request, &context).await?;
        Ok(ReviewWorkerResult {
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

pub fn model_for_review(service: &ServiceConfig) -> Option<String> {
    if service.agent_backend == AgentBackend::Codex {
        None
    } else {
        service
            .review
            .model
            .as_deref()
            .or(service.pi_agent.model.as_deref())
            .map(str::to_string)
    }
}

pub fn command_for_review(service: &ServiceConfig) -> Result<Vec<String>> {
    let command = match service.agent_backend {
        AgentBackend::Codex => service.codex.command.clone(),
        AgentBackend::KataCli => service.pi_agent.command.clone(),
    };
    if command.is_empty() {
        return Err(SymphonyError::InvalidWorkflowConfig(
            "review worker command is empty".to_string(),
        ));
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_selection_prefers_stage_then_agent_defaults() {
        let service = ServiceConfig {
            review: crate::review::domain::ReviewConfig {
                model: Some("review-model".to_string()),
                ..crate::review::domain::ReviewConfig::default()
            },
            pi_agent: crate::domain::PiAgentConfig {
                model: Some("agent-model".to_string()),
                ..crate::domain::PiAgentConfig::default()
            },
            ..ServiceConfig::default()
        };
        assert_eq!(model_for_review(&service).as_deref(), Some("review-model"));
        assert_eq!(harness_for_service(&service), TriageHarness::Pi);

        let service = ServiceConfig {
            review: crate::review::domain::ReviewConfig::default(),
            pi_agent: crate::domain::PiAgentConfig {
                model: Some("agent-model".to_string()),
                ..crate::domain::PiAgentConfig::default()
            },
            ..ServiceConfig::default()
        };
        assert_eq!(model_for_review(&service).as_deref(), Some("agent-model"));

        let service = ServiceConfig {
            agent_backend: AgentBackend::Codex,
            codex: crate::domain::CodexConfig {
                command: vec!["codex".to_string()],
                ..crate::domain::CodexConfig::default()
            },
            ..ServiceConfig::default()
        };
        assert_eq!(model_for_review(&service), None);
        assert_eq!(harness_for_service(&service), TriageHarness::Codex);
        assert!(command_for_review(&service).is_ok());

        let service = ServiceConfig {
            pi_agent: crate::domain::PiAgentConfig {
                command: vec![],
                ..crate::domain::PiAgentConfig::default()
            },
            ..ServiceConfig::default()
        };
        assert!(command_for_review(&service).is_err());
    }
}
