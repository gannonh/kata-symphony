//! Deterministic gate computation for the verification stage.
//!
//! Symphony computes the gate from durable command outcomes plus complete
//! criterion coverage. A verifier cannot override a failed command, an
//! unexecuted required command, a missing criterion, or an invalid evidence
//! reference.

use std::collections::HashSet;

use crate::error::{Result, SymphonyError};
use crate::verification::domain::{
    VerifierCriterionStatus, VerifierManifest, VerificationCommandRunRecord,
    VerificationEvidenceRecord, VERIFICATION_CRITERIA_MAX_ITEMS,
};

/// Identity the verifier manifest must match exactly.
#[derive(Debug, Clone)]
pub struct GateIdentity<'a> {
    pub spec_artifact_id: &'a str,
    pub implementation_artifact_id: &'a str,
    pub review_artifact_id: &'a str,
    pub reviewed_head_sha: &'a str,
    pub base_sha: &'a str,
    /// Number of acceptance criteria in the approved A2 spec. Every criterion
    /// index must appear exactly once.
    pub criterion_count: usize,
}

/// Output of the strict manifest validation and gate computation.
#[derive(Debug, Clone)]
pub struct GateVerdict {
    pub passed: bool,
    /// Human-readable reasons the gate did not pass (empty when passed).
    pub reasons: Vec<String>,
    /// Per-criterion status coverage as computed by Symphony.
    pub criteria: Vec<CriterionVerdict>,
}

#[derive(Debug, Clone)]
pub struct CriterionVerdict {
    pub index: u32,
    pub status: VerifierCriterionStatus,
    pub rationale: String,
    pub evidence: Vec<String>,
}

/// Validate the verifier manifest strictly, then compute the gate.
///
/// Returns the verdict; any violation is a hard error (the attempt fails with
/// a manifest validation error rather than a product gate failure).
pub fn compute_gate(
    manifest: &VerifierManifest,
    identity: &GateIdentity<'_>,
    command_runs: &[VerificationCommandRunRecord],
    evidence: &[VerificationEvidenceRecord],
) -> Result<GateVerdict> {
    validate_manifest(manifest, identity, evidence)?;
    let criteria = manifest
        .criteria
        .iter()
        .map(|criterion| CriterionVerdict {
            index: criterion.index,
            status: criterion.status,
            rationale: criterion.rationale.clone(),
            evidence: criterion.evidence.clone(),
        })
        .collect::<Vec<_>>();

    let mut reasons = Vec::new();
    for run in command_runs {
        match run.status.as_str() {
            "completed" if run.passed == Some(true) => {}
            "completed" => reasons.push(format!(
                "command '{}' failed (exit {:?})",
                run.name, run.exit_code
            )),
            "failed" | "interrupted" => reasons.push(format!(
                "command '{}' did not complete ({})",
                run.name, run.status
            )),
            "not_run" => reasons.push(format!(
                "required command '{}' was never executed",
                run.name
            )),
            "launching" | "running" => reasons.push(format!(
                "command '{}' is still {}", run.name, run.status
            )),
            _ => reasons.push(format!(
                "command '{}' has an unexpected status '{}'",
                run.name, run.status
            )),
        }
    }
    // The acceptance command must have run and passed for a green gate.
    if let Some(acceptance) = command_runs
        .iter()
        .find(|run| run.kind == crate::verification::domain::VerificationCommandKind::Acceptance)
    {
        if acceptance.status != "completed" || acceptance.passed != Some(true) {
            reasons.push(format!(
                "acceptance command '{}' did not pass",
                acceptance.name
            ));
        }
    } else {
        reasons.push("no acceptance command recorded".to_string());
    }

    let passed = reasons.is_empty();
    Ok(GateVerdict {
        passed,
        reasons,
        criteria,
    })
}

/// Strict manifest validation. Any violation is an error.
pub fn validate_manifest(
    manifest: &VerifierManifest,
    identity: &GateIdentity<'_>,
    evidence: &[VerificationEvidenceRecord],
) -> Result<()> {
    if manifest.schema_version != 1 {
        return Err(SymphonyError::TriageError(format!(
            "verifier manifest schema_version must be 1, got {}",
            manifest.schema_version
        )));
    }
    if manifest.spec_artifact_id != identity.spec_artifact_id {
        return Err(SymphonyError::TriageError(format!(
            "verifier manifest spec_artifact_id {} does not match {}",
            manifest.spec_artifact_id, identity.spec_artifact_id
        )));
    }
    if manifest.implementation_artifact_id != identity.implementation_artifact_id {
        return Err(SymphonyError::TriageError(format!(
            "verifier manifest implementation_artifact_id {} does not match {}",
            manifest.implementation_artifact_id, identity.implementation_artifact_id
        )));
    }
    if manifest.review_artifact_id != identity.review_artifact_id {
        return Err(SymphonyError::TriageError(format!(
            "verifier manifest review_artifact_id {} does not match {}",
            manifest.review_artifact_id, identity.review_artifact_id
        )));
    }
    if manifest.reviewed_head_sha != identity.reviewed_head_sha {
        return Err(SymphonyError::TriageError(format!(
            "verifier manifest reviewed_head_sha {} does not match {}",
            manifest.reviewed_head_sha, identity.reviewed_head_sha
        )));
    }
    if manifest.base_sha != identity.base_sha {
        return Err(SymphonyError::TriageError(format!(
            "verifier manifest base_sha {} does not match {}",
            manifest.base_sha, identity.base_sha
        )));
    }
    if manifest.summary.trim().is_empty() {
        return Err(SymphonyError::TriageError(
            "verifier manifest summary must be non-empty".to_string(),
        ));
    }
    if manifest.criteria.len() != identity.criterion_count {
        return Err(SymphonyError::TriageError(format!(
            "verifier manifest declares {} criteria; the approved spec has {}",
            manifest.criteria.len(),
            identity.criterion_count
        )));
    }
    if manifest.criteria.len() > VERIFICATION_CRITERIA_MAX_ITEMS {
        return Err(SymphonyError::TriageError(format!(
            "verifier manifest criteria exceed {VERIFICATION_CRITERIA_MAX_ITEMS}"
        )));
    }
    let valid_evidence_paths: HashSet<&str> = evidence
        .iter()
        .map(|record| record.relative_path.as_str())
        .collect();
    let mut seen = HashSet::new();
    for criterion in &manifest.criteria {
        if criterion.index == 0 {
            return Err(SymphonyError::TriageError(
                "verifier manifest criterion index must be 1-based".to_string(),
            ));
        }
        if !seen.insert(criterion.index) {
            return Err(SymphonyError::TriageError(format!(
                "verifier manifest criterion index {} appears more than once",
                criterion.index
            )));
        }
        if criterion.rationale.trim().is_empty() {
            return Err(SymphonyError::TriageError(format!(
                "verifier manifest criterion {} has an empty rationale",
                criterion.index
            )));
        }
        match criterion.status {
            VerifierCriterionStatus::Pass | VerifierCriterionStatus::Fail => {
                if criterion.evidence.is_empty() {
                    return Err(SymphonyError::TriageError(format!(
                        "verifier manifest criterion {} needs at least one evidence reference for status '{}'",
                        criterion.index,
                        criterion.status.as_str()
                    )));
                }
                for reference in &criterion.evidence {
                    if !valid_evidence_paths.contains(reference.as_str()) {
                        return Err(SymphonyError::TriageError(format!(
                            "verifier manifest criterion {} references evidence '{}' outside the attempt",
                            criterion.index, reference
                        )));
                    }
                }
            }
            VerifierCriterionStatus::NotProven => {}
        }
    }
    // Every approved criterion index must appear exactly once.
    for index in 1..=identity.criterion_count {
        if !seen.contains(&(index as u32)) {
            return Err(SymphonyError::TriageError(format!(
                "verifier manifest is missing criterion {index}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::domain::{
        VerifierCriterion, VerifierCriterionStatus, VerifierManifest,
        VerificationCommandKind, VerificationCommandRunRecord,
    };

    fn identity() -> GateIdentity<'static> {
        GateIdentity {
            spec_artifact_id: "spec",
            implementation_artifact_id: "implementation",
            review_artifact_id: "review",
            reviewed_head_sha: "head-sha",
            base_sha: "base-sha",
            criterion_count: 2,
        }
    }

    fn evidence() -> Vec<VerificationEvidenceRecord> {
        vec![VerificationEvidenceRecord {
            evidence_id: "e1".to_string(),
            run_id: "run".to_string(),
            attempt_id: "attempt".to_string(),
            relative_path: "reports/ok.json".to_string(),
            sha256: "digest".to_string(),
            bytes_len: 3,
            collected_at: chrono::Utc::now(),
        }]
    }

    fn command(name: &str, kind: VerificationCommandKind, status: &str, passed: Option<bool>) -> VerificationCommandRunRecord {
        VerificationCommandRunRecord {
            command_run_id: format!("run-{name}"),
            run_id: "run".to_string(),
            attempt_id: "attempt".to_string(),
            ordinal: 1,
            name: name.to_string(),
            kind,
            configuration_revision: "cfg".to_string(),
            command_sha256: "sha".to_string(),
            status: status.to_string(),
            launch_nonce: None,
            pid: None,
            process_group_id: None,
            process_start_token: None,
            executable_identity: None,
            container_id: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            exit_code: None,
            termination_reason: None,
            passed,
            output_tail: None,
            output_sha256: None,
            execution_profile: "local".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn manifest(criteria: Vec<VerifierCriterion>) -> VerifierManifest {
        VerifierManifest {
            schema_version: 1,
            spec_artifact_id: "spec".to_string(),
            implementation_artifact_id: "implementation".to_string(),
            review_artifact_id: "review".to_string(),
            reviewed_head_sha: "head-sha".to_string(),
            base_sha: "base-sha".to_string(),
            summary: "verified".to_string(),
            criteria,
        }
    }

    fn criterion(index: u32, status: VerifierCriterionStatus, evidence_refs: &[&str]) -> VerifierCriterion {
        VerifierCriterion {
            index,
            status,
            rationale: "because".to_string(),
            evidence: evidence_refs.iter().map(|value| value.to_string()).collect(),
        }
    }

    fn passing_commands() -> Vec<VerificationCommandRunRecord> {
        vec![
            command("unit", VerificationCommandKind::Test, "completed", Some(true)),
            command("acceptance", VerificationCommandKind::Acceptance, "completed", Some(true)),
        ]
    }

    #[test]
    fn green_gate_passes_with_full_coverage() {
        let manifest = manifest(vec![
            criterion(1, VerifierCriterionStatus::Pass, &["reports/ok.json"]),
            criterion(2, VerifierCriterionStatus::NotProven, &[]),
        ]);
        let verdict = compute_gate(&manifest, &identity(), &passing_commands(), &evidence()).unwrap();
        assert!(verdict.passed, "{:?}", verdict.reasons);
        assert_eq!(verdict.criteria.len(), 2);
    }

    #[test]
    fn failed_command_overrides_verifier_claims() {
        let manifest = manifest(vec![
            criterion(1, VerifierCriterionStatus::Pass, &["reports/ok.json"]),
            criterion(2, VerifierCriterionStatus::Pass, &["reports/ok.json"]),
        ]);
        let commands = vec![
            command("unit", VerificationCommandKind::Test, "completed", Some(false)),
            command("acceptance", VerificationCommandKind::Acceptance, "not_run", None),
        ];
        let verdict = compute_gate(&manifest, &identity(), &commands, &evidence()).unwrap();
        assert!(!verdict.passed);
        assert!(verdict.reasons.iter().any(|reason| reason.contains("unit")));
        assert!(verdict.reasons.iter().any(|reason| reason.contains("acceptance")));
    }

    #[test]
    fn unexecuted_acceptance_command_fails_the_gate() {
        let manifest = manifest(vec![
            criterion(1, VerifierCriterionStatus::Pass, &["reports/ok.json"]),
            criterion(2, VerifierCriterionStatus::Pass, &["reports/ok.json"]),
        ]);
        let commands = vec![
            command("unit", VerificationCommandKind::Test, "completed", Some(true)),
            command("acceptance", VerificationCommandKind::Acceptance, "not_run", None),
        ];
        let verdict = compute_gate(&manifest, &identity(), &commands, &evidence()).unwrap();
        assert!(!verdict.passed);
        assert!(verdict.reasons.iter().any(|reason| reason.contains("never executed")));
    }

    #[test]
    fn missing_criterion_is_rejected() {
        let manifest = manifest(vec![criterion(1, VerifierCriterionStatus::Pass, &["reports/ok.json"])]);
        let err = compute_gate(&manifest, &identity(), &passing_commands(), &evidence()).unwrap_err();
        assert!(err.to_string().contains("approved spec has 2"));
    }

    #[test]
    fn duplicate_criterion_is_rejected() {
        let manifest = manifest(vec![
            criterion(1, VerifierCriterionStatus::Pass, &["reports/ok.json"]),
            criterion(1, VerifierCriterionStatus::NotProven, &[]),
        ]);
        let err = compute_gate(&manifest, &identity(), &passing_commands(), &evidence()).unwrap_err();
        assert!(err.to_string().contains("more than once"));
    }

    #[test]
    fn evidence_outside_the_attempt_is_rejected() {
        let manifest = manifest(vec![
            criterion(1, VerifierCriterionStatus::Pass, &["reports/other.json"]),
            criterion(2, VerifierCriterionStatus::NotProven, &[]),
        ]);
        let err = compute_gate(&manifest, &identity(), &passing_commands(), &evidence()).unwrap_err();
        assert!(err.to_string().contains("outside the attempt"));
    }

    #[test]
    fn empty_rationale_is_rejected() {
        let manifest = manifest(vec![
            VerifierCriterion {
                index: 1,
                status: VerifierCriterionStatus::NotProven,
                rationale: "   ".to_string(),
                evidence: vec![],
            },
            criterion(2, VerifierCriterionStatus::NotProven, &[]),
        ]);
        let err = compute_gate(&manifest, &identity(), &passing_commands(), &evidence()).unwrap_err();
        assert!(err.to_string().contains("empty rationale"));
    }

    #[test]
    fn wrong_head_or_artifact_identity_is_rejected() {
        let mut manifest = manifest(vec![
            criterion(1, VerifierCriterionStatus::NotProven, &[]),
            criterion(2, VerifierCriterionStatus::NotProven, &[]),
        ]);
        manifest.reviewed_head_sha = "other-head".to_string();
        let err = compute_gate(&manifest, &identity(), &passing_commands(), &evidence()).unwrap_err();
        assert!(err.to_string().contains("reviewed_head_sha"));

        manifest.reviewed_head_sha = "head-sha".to_string();
        manifest.spec_artifact_id = "other-spec".to_string();
        let err = compute_gate(&manifest, &identity(), &passing_commands(), &evidence()).unwrap_err();
        assert!(err.to_string().contains("spec_artifact_id"));
    }
}
