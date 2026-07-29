use crate::implementation::domain::{
    AcceptanceCriterionClaim, ImplementationManifest, ValidationCommandResult,
    IMPLEMENTATION_COMMENT_MARKER_PREFIX,
};

#[derive(Debug, Clone)]
pub struct ImplementationPreviewComment<'a> {
    pub intent_id: &'a str,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub artifact_id: &'a str,
    pub approved_version: u32,
    pub approved_spec_path: &'a str,
    pub base_commit: &'a str,
    pub head_commit: &'a str,
    pub manifest: &'a ImplementationManifest,
    pub changed_paths: &'a [String],
    pub validation: &'a [ValidationCommandResult],
}

pub fn marker(intent_id: &str) -> String {
    format!("{IMPLEMENTATION_COMMENT_MARKER_PREFIX}{intent_id} -->")
}

pub fn render_preview_comment(input: ImplementationPreviewComment<'_>) -> String {
    let mut out = String::new();
    out.push_str(&marker(input.intent_id));
    out.push_str("\n## Symphony implementation preview\n\n");
    out.push_str(
        "**Preview:** Symphony validated this implementation locally and stored a verified change bundle. No remote branch or pull request was created, and tracker state was not advanced.\n\n",
    );
    out.push_str(&format!(
        "**Factory run:** `{}` · **Stage run:** `{}` · **Artifact:** `{}`\n\n",
        input.run_id, input.stage_run_id, input.artifact_id
    ));
    out.push_str(&format!(
        "**Approved spec:** v{} at `{}`\n\n",
        input.approved_version, input.approved_spec_path
    ));
    out.push_str(&format!(
        "**Commits:** `{}` → `{}`\n\n",
        abbreviate(input.base_commit),
        abbreviate(input.head_commit)
    ));
    out.push_str("### Summary\n\n");
    out.push_str(input.manifest.summary.trim());
    out.push_str("\n\n### Changed files\n\n");
    if input.changed_paths.is_empty() {
        out.push_str("_None listed._\n");
    } else {
        for path in input.changed_paths.iter().take(50) {
            out.push_str(&format!("- `{}`\n", path.trim()));
        }
        if input.changed_paths.len() > 50 {
            out.push_str(&format!(
                "- _…and {} more_\n",
                input.changed_paths.len() - 50
            ));
        }
    }
    out.push_str("\n### Acceptance criteria\n\n");
    render_criteria(&mut out, &input.manifest.acceptance_criteria);
    out.push_str("\n### Validation\n\n");
    if input.validation.is_empty() {
        out.push_str("_No validation commands recorded._\n");
    } else {
        for command in input.validation {
            let status = if command.passed { "pass" } else { "fail" };
            out.push_str(&format!(
                "- `{}`: **{}** in {} ms",
                command.name, status, command.duration_ms
            ));
            if let Some(code) = command.exit_code {
                out.push_str(&format!(" (exit {code})"));
            }
            out.push('\n');
        }
    }
    out.push_str("\n### Known limitations\n\n");
    if input.manifest.known_limitations.is_empty() {
        out.push_str("_None._\n");
    } else {
        for limitation in &input.manifest.known_limitations {
            out.push_str(&format!("- {}\n", limitation.trim()));
        }
    }
    out.push_str("\n---\n");
    out.push_str(
        "To publish a draft pull request, set `implementation.mode: automatic` and ensure `completion_route.state` resolves to Agent Review.\n",
    );
    out
}

pub fn render_diagnostic_comment(intent_id: &str, run_id: &str, message: &str) -> String {
    let mut out = String::new();
    out.push_str(&marker(intent_id));
    out.push_str("\n## Symphony implementation\n\n");
    out.push_str(&format!("**Factory run:** `{run_id}`\n\n"));
    out.push_str("**Action required:** ");
    out.push_str(message.trim());
    out.push('\n');
    out
}

fn render_criteria(out: &mut String, claims: &[AcceptanceCriterionClaim]) {
    if claims.is_empty() {
        out.push_str("_None._\n");
        return;
    }
    for claim in claims {
        out.push_str(&format!("- **Criterion {}** — implemented\n", claim.index));
        for evidence in &claim.evidence {
            out.push_str(&format!(
                "  - `{}` {}: {}\n",
                evidence.kind.as_str(),
                evidence.reference.trim(),
                evidence.summary.trim()
            ));
        }
    }
}

fn abbreviate(sha: &str) -> &str {
    if sha.len() >= 12 {
        &sha[..12]
    } else {
        sha
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementation::domain::{
        CriterionStatus, EvidenceKind, ExecutionProfile, ImplementationEvidence, ManifestStatus,
    };
    use chrono::Utc;

    #[test]
    fn renders_preview_with_marker_and_no_publication_claims() {
        let manifest = ImplementationManifest {
            schema_version: 1,
            status: ManifestStatus::Completed,
            head_commit: Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()),
            summary: "Adds retry policy.".to_string(),
            acceptance_criteria: vec![AcceptanceCriterionClaim {
                index: 1,
                status: CriterionStatus::Implemented,
                evidence: vec![ImplementationEvidence {
                    kind: EvidenceKind::Repository,
                    reference: "src/x.rs".to_string(),
                    summary: "Coordinator bound".to_string(),
                }],
            }],
            known_limitations: vec!["SSH deferred".to_string()],
            blocker: None,
        };
        let validation = [ValidationCommandResult {
            name: "affected-validation".to_string(),
            command_sha256: "abc".to_string(),
            cycle: 1,
            started_at: Utc::now(),
            completed_at: Utc::now(),
            duration_ms: 12,
            exit_code: Some(0),
            passed: true,
            termination_reason: None,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            output_sha256: "def".to_string(),
            execution_profile: ExecutionProfile::Local,
        }];
        let body = render_preview_comment(ImplementationPreviewComment {
            intent_id: "intent-1",
            run_id: "run-1",
            stage_run_id: "stage-1",
            artifact_id: "art-1",
            approved_version: 2,
            approved_spec_path: "specs/KATA-1/APPROVED-v2.md",
            base_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            head_commit: "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            manifest: &manifest,
            changed_paths: &[
                "specs/KATA-1/APPROVED-v2.md".to_string(),
                "src/x.rs".to_string(),
            ],
            validation: &validation,
        });
        assert!(body.starts_with("<!-- symphony:implementation:intent-1 -->"));
        assert!(body.contains("No remote branch or pull request was created"));
        assert!(body.contains("Adds retry policy."));
        assert!(body.contains("`affected-validation`: **pass**"));
        assert!(body.contains("SSH deferred"));
        assert!(body.contains("implementation.mode: automatic"));
    }
}
