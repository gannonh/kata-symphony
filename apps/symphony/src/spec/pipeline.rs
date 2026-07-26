use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::Result;
use crate::spec::artifact::{validate_findings, validate_spec};
use crate::spec::domain::{
    ReviewFinding, ReviewFindings, ReviewVerdict, SpecArtifact, SpecTurnKind,
    SPEC_LIST_ITEM_MAX_BYTES, SPEC_OPEN_DECISIONS_MAX_ITEMS,
};
use crate::triage::domain::StageUsage;

#[derive(Debug, Clone)]
pub struct SpecPipelineConfig {
    pub max_review_cycles: u32,
    pub turn_timeout_ms: u64,
    pub harness: String,
    pub draft_model: Option<String>,
    pub review_model: Option<String>,
    pub prompts: SpecPipelinePrompts,
}

#[derive(Debug, Clone)]
pub struct SpecPipelinePrompts {
    pub draft: String,
    pub review: String,
    pub revise: String,
}

#[derive(Debug, Clone)]
pub struct SeededRevision {
    pub prior_version: u32,
    pub prior_spec: SpecArtifact,
    pub feedback: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SpecPipelineRequest {
    pub stage_run_id: String,
    pub issue_context: serde_json::Value,
    pub seed: Option<SeededRevision>,
    pub config: SpecPipelineConfig,
}

#[derive(Debug, Clone)]
pub struct SpecTurnRequest {
    pub turn_id: String,
    pub stage_run_id: String,
    pub ordinal: u32,
    pub kind: SpecTurnKind,
    pub prompt: String,
    pub model: Option<String>,
    pub turn_timeout_ms: u64,
    /// Files allowlisted for this isolated turn. Executors must create a fresh
    /// workspace and input directory and expose only these values.
    pub inputs: SpecTurnInputs,
}

#[derive(Debug, Clone)]
pub struct SpecTurnInputs {
    pub issue_context: serde_json::Value,
    pub current_spec: Option<SpecArtifact>,
    pub blocking_findings: Option<Vec<ReviewFinding>>,
    pub prior_version: Option<u32>,
    pub human_feedback: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub enum SpecTurnOutput {
    Spec(SpecArtifact),
    Findings(ReviewFindings),
}

#[derive(Debug, Clone)]
pub struct SpecTurnOutcome {
    pub output: SpecTurnOutput,
    pub usage: StageUsage,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub workspace_identity: String,
}

#[derive(Debug, Clone)]
pub struct CompletedSpecTurn {
    pub turn_id: String,
    pub ordinal: u32,
    pub kind: SpecTurnKind,
    pub model: Option<String>,
    pub output: SpecTurnOutput,
    pub usage: StageUsage,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub workspace_identity: String,
}

#[derive(Debug, Clone)]
pub struct SpecPipelineOutcome {
    pub artifact: SpecArtifact,
    pub turns: Vec<CompletedSpecTurn>,
    pub review_cycles: u32,
    pub unresolved_blocking_findings: Vec<ReviewFinding>,
    pub usage: StageUsage,
}

#[async_trait]
pub trait SpecTurnExecutor: Send + Sync {
    /// Execute one turn in a fresh clone and fresh input directory. The
    /// workspace identity returned for each invocation must be unique.
    async fn execute(&self, request: SpecTurnRequest) -> Result<SpecTurnOutcome>;
}

pub async fn run_pipeline<E: SpecTurnExecutor>(
    executor: &E,
    request: SpecPipelineRequest,
) -> Result<SpecPipelineOutcome> {
    if request.config.max_review_cycles == 0 {
        return Err(crate::error::SymphonyError::InvalidWorkflowConfig(
            "spec.max_review_cycles must be greater than 0".to_string(),
        ));
    }
    if request.config.turn_timeout_ms == 0 {
        return Err(crate::error::SymphonyError::InvalidWorkflowConfig(
            "spec.turn_timeout_ms must be greater than 0".to_string(),
        ));
    }

    let mut turns = Vec::new();
    let mut ordinal = 1;
    let mut current = if let Some(seed) = request.seed.as_ref() {
        let revised = execute_spec_turn(
            executor,
            &request,
            ordinal,
            SpecTurnKind::Revise,
            request.config.prompts.revise.clone(),
            request.config.draft_model.clone(),
            SpecTurnInputs {
                issue_context: request.issue_context.clone(),
                current_spec: Some(seed.prior_spec.clone()),
                blocking_findings: None,
                prior_version: Some(seed.prior_version),
                human_feedback: Some(seed.feedback.clone()),
            },
            &mut turns,
        )
        .await?;
        ordinal += 1;
        revised
    } else {
        let draft = execute_spec_turn(
            executor,
            &request,
            ordinal,
            SpecTurnKind::Draft,
            request.config.prompts.draft.clone(),
            request.config.draft_model.clone(),
            SpecTurnInputs {
                issue_context: request.issue_context.clone(),
                current_spec: None,
                blocking_findings: None,
                prior_version: None,
                human_feedback: None,
            },
            &mut turns,
        )
        .await?;
        ordinal += 1;
        draft
    };

    let mut review_cycles = 0;
    let unresolved = loop {
        review_cycles += 1;
        let findings =
            execute_review_turn(executor, &request, ordinal, current.clone(), &mut turns).await?;
        ordinal += 1;

        let blocking: Vec<ReviewFinding> = findings.blocking().cloned().collect();
        if findings.verdict == ReviewVerdict::Pass {
            break Vec::new();
        }
        if review_cycles >= request.config.max_review_cycles {
            append_unresolved_to_open_decisions(&mut current, &blocking);
            validate_spec(&current).map_err(|error| {
                crate::error::SymphonyError::TriageError(format!(
                    "spec cap post-processing validation failed: {error}"
                ))
            })?;
            break blocking;
        }

        current = execute_spec_turn(
            executor,
            &request,
            ordinal,
            SpecTurnKind::Revise,
            request.config.prompts.revise.clone(),
            request.config.draft_model.clone(),
            SpecTurnInputs {
                issue_context: request.issue_context.clone(),
                current_spec: Some(current),
                blocking_findings: Some(blocking),
                prior_version: None,
                human_feedback: None,
            },
            &mut turns,
        )
        .await?;
        ordinal += 1;
    };

    let usage = turns.iter().fold(StageUsage::default(), |mut total, turn| {
        total.input_tokens = total.input_tokens.saturating_add(turn.usage.input_tokens);
        total.output_tokens = total.output_tokens.saturating_add(turn.usage.output_tokens);
        total.total_tokens = total.total_tokens.saturating_add(turn.usage.total_tokens);
        total
    });

    Ok(SpecPipelineOutcome {
        artifact: current,
        turns,
        review_cycles,
        unresolved_blocking_findings: unresolved,
        usage,
    })
}

#[allow(clippy::too_many_arguments)]
async fn execute_spec_turn<E: SpecTurnExecutor>(
    executor: &E,
    pipeline: &SpecPipelineRequest,
    ordinal: u32,
    kind: SpecTurnKind,
    prompt: String,
    model: Option<String>,
    inputs: SpecTurnInputs,
    turns: &mut Vec<CompletedSpecTurn>,
) -> Result<SpecArtifact> {
    let completed = execute_turn(executor, pipeline, ordinal, kind, prompt, model, inputs).await?;
    let artifact = match &completed.output {
        SpecTurnOutput::Spec(artifact) => {
            validate_spec(artifact).map_err(|error| {
                crate::error::SymphonyError::TriageError(format!(
                    "spec {} turn produced invalid output: {error}",
                    kind.as_str()
                ))
            })?;
            artifact.clone()
        }
        SpecTurnOutput::Findings(_) => {
            return Err(crate::error::SymphonyError::TriageError(format!(
                "spec {} turn produced findings instead of a spec artifact",
                kind.as_str()
            )))
        }
    };
    turns.push(completed);
    Ok(artifact)
}

async fn execute_review_turn<E: SpecTurnExecutor>(
    executor: &E,
    pipeline: &SpecPipelineRequest,
    ordinal: u32,
    current: SpecArtifact,
    turns: &mut Vec<CompletedSpecTurn>,
) -> Result<ReviewFindings> {
    let completed = execute_turn(
        executor,
        pipeline,
        ordinal,
        SpecTurnKind::Review,
        pipeline.config.prompts.review.clone(),
        pipeline.config.review_model.clone(),
        SpecTurnInputs {
            issue_context: pipeline.issue_context.clone(),
            current_spec: Some(current),
            blocking_findings: None,
            prior_version: None,
            human_feedback: None,
        },
    )
    .await?;
    let findings = match &completed.output {
        SpecTurnOutput::Findings(findings) => {
            validate_findings(findings).map_err(|error| {
                crate::error::SymphonyError::TriageError(format!(
                    "spec review turn produced invalid output: {error}"
                ))
            })?;
            findings.clone()
        }
        SpecTurnOutput::Spec(_) => {
            return Err(crate::error::SymphonyError::TriageError(
                "spec review turn produced a spec instead of findings".to_string(),
            ))
        }
    };
    turns.push(completed);
    Ok(findings)
}

async fn execute_turn<E: SpecTurnExecutor>(
    executor: &E,
    pipeline: &SpecPipelineRequest,
    ordinal: u32,
    kind: SpecTurnKind,
    prompt: String,
    model: Option<String>,
    inputs: SpecTurnInputs,
) -> Result<CompletedSpecTurn> {
    let turn_id = Uuid::now_v7().to_string();
    let outcome = executor
        .execute(SpecTurnRequest {
            turn_id: turn_id.clone(),
            stage_run_id: pipeline.stage_run_id.clone(),
            ordinal,
            kind,
            prompt,
            model: model.clone(),
            turn_timeout_ms: pipeline.config.turn_timeout_ms,
            inputs,
        })
        .await?;
    Ok(CompletedSpecTurn {
        turn_id,
        ordinal,
        kind,
        model,
        output: outcome.output,
        usage: outcome.usage,
        started_at: outcome.started_at,
        completed_at: outcome.completed_at,
        workspace_identity: outcome.workspace_identity,
    })
}

fn append_unresolved_to_open_decisions(artifact: &mut SpecArtifact, blocking: &[ReviewFinding]) {
    for finding in blocking {
        if artifact.open_decisions.len() >= SPEC_OPEN_DECISIONS_MAX_ITEMS {
            break;
        }
        let line = format!(
            "Unresolved review finding: {} — {}",
            finding.summary.trim(),
            finding.recommendation.trim()
        );
        artifact
            .open_decisions
            .push(crate::triage::domain::truncate_utf8_bytes(
                &line,
                SPEC_LIST_ITEM_MAX_BYTES,
            ));
    }
}

/// Pi model precedence for the drafting/revision turns. Review defaults to the
/// resolved drafter model unless `spec.review_model` is configured.
pub fn resolve_models(
    spec_model: Option<&str>,
    review_model: Option<&str>,
    agent_model: Option<&str>,
) -> (Option<String>, Option<String>) {
    let draft = spec_model
        .and_then(non_empty)
        .or_else(|| agent_model.and_then(non_empty));
    let review = review_model.and_then(non_empty).or_else(|| draft.clone());
    (draft, review)
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::spec::domain::{
        FindingSection, FindingSeverity, ReviewFinding, ReviewFindings, ReviewVerdict,
    };

    struct FakeExecutor {
        outputs: Mutex<Vec<SpecTurnOutput>>,
        requests: Mutex<Vec<SpecTurnRequest>>,
    }

    #[async_trait]
    impl SpecTurnExecutor for FakeExecutor {
        async fn execute(&self, request: SpecTurnRequest) -> Result<SpecTurnOutcome> {
            self.requests.lock().unwrap().push(request.clone());
            let output = self.outputs.lock().unwrap().remove(0);
            Ok(SpecTurnOutcome {
                output,
                usage: StageUsage::default(),
                started_at: Utc::now(),
                completed_at: Utc::now(),
                workspace_identity: request.turn_id,
            })
        }
    }

    fn artifact() -> SpecArtifact {
        SpecArtifact {
            schema_version: 1,
            product_behavior: "behavior".to_string(),
            technical_approach: "approach".to_string(),
            acceptance_criteria: vec!["observable".to_string()],
            open_decisions: vec![],
        }
    }

    fn blocking() -> ReviewFindings {
        ReviewFindings {
            schema_version: 1,
            verdict: ReviewVerdict::Revise,
            findings: vec![ReviewFinding {
                severity: FindingSeverity::Blocking,
                section: FindingSection::AcceptanceCriteria,
                summary: "Not observable".to_string(),
                recommendation: "Name the response".to_string(),
            }],
        }
    }

    fn pass() -> ReviewFindings {
        ReviewFindings {
            schema_version: 1,
            verdict: ReviewVerdict::Pass,
            findings: vec![],
        }
    }

    fn request(max_review_cycles: u32) -> SpecPipelineRequest {
        SpecPipelineRequest {
            stage_run_id: "stage".to_string(),
            issue_context: serde_json::json!({"title":"Issue"}),
            seed: None,
            config: SpecPipelineConfig {
                max_review_cycles,
                turn_timeout_ms: 100,
                harness: "pi".to_string(),
                draft_model: Some("draft".to_string()),
                review_model: Some("review".to_string()),
                prompts: SpecPipelinePrompts {
                    draft: "draft".to_string(),
                    review: "review".to_string(),
                    revise: "revise".to_string(),
                },
            },
        }
    }

    #[tokio::test]
    async fn runs_draft_review_revise_review_with_isolated_inputs() {
        let executor = FakeExecutor {
            outputs: Mutex::new(vec![
                SpecTurnOutput::Spec(artifact()),
                SpecTurnOutput::Findings(blocking()),
                SpecTurnOutput::Spec(artifact()),
                SpecTurnOutput::Findings(pass()),
            ]),
            requests: Mutex::new(vec![]),
        };
        let outcome = run_pipeline(&executor, request(3)).await.unwrap();
        assert_eq!(outcome.review_cycles, 2);
        let requests = executor.requests.lock().unwrap();
        assert_eq!(
            requests.iter().map(|r| r.kind).collect::<Vec<_>>(),
            vec![
                SpecTurnKind::Draft,
                SpecTurnKind::Review,
                SpecTurnKind::Revise,
                SpecTurnKind::Review
            ]
        );
        assert!(requests[1].inputs.blocking_findings.is_none());
        assert!(requests[1].inputs.human_feedback.is_none());
        assert_eq!(requests[1].model.as_deref(), Some("review"));
    }

    #[tokio::test]
    async fn cap_publishes_with_bounded_open_decision() {
        let executor = FakeExecutor {
            outputs: Mutex::new(vec![
                SpecTurnOutput::Spec(artifact()),
                SpecTurnOutput::Findings(blocking()),
            ]),
            requests: Mutex::new(vec![]),
        };
        let outcome = run_pipeline(&executor, request(1)).await.unwrap();
        assert_eq!(outcome.unresolved_blocking_findings.len(), 1);
        assert!(outcome.artifact.open_decisions[0].contains("Not observable"));
    }

    #[test]
    fn model_resolution_matches_precedence() {
        assert_eq!(
            resolve_models(Some("spec"), None, Some("agent")),
            (Some("spec".to_string()), Some("spec".to_string()))
        );
        assert_eq!(
            resolve_models(None, Some("review"), Some("agent")),
            (Some("agent".to_string()), Some("review".to_string()))
        );
    }
}
