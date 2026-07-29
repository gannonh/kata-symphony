use crate::implementation::domain::{
    AcceptanceCriterionClaim, ImplementationManifest, ValidationCommandResult,
    IMPLEMENTATION_COMMENT_MARKER_PREFIX, IMPLEMENTATION_PR_MARKER_PREFIX,
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

#[derive(Debug, Clone)]
pub struct ImplementationDraftPrBody<'a> {
    pub intent_id: &'a str,
    pub issue_number: u64,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub artifact_id: &'a str,
    pub bundle_artifact_id: &'a str,
    pub approved_artifact_id: &'a str,
    pub approved_version: u32,
    pub approved_spec_path: &'a str,
    pub base_commit: &'a str,
    pub head_commit: &'a str,
    pub manifest: &'a ImplementationManifest,
    pub validation: &'a [ValidationCommandResult],
}

#[derive(Debug, Clone)]
pub struct ImplementationPublicationPendingComment<'a> {
    pub intent_id: &'a str,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub artifact_id: &'a str,
}

#[derive(Debug, Clone)]
pub struct ImplementationPublicationFinalComment<'a> {
    pub intent_id: &'a str,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub artifact_id: &'a str,
    pub pr_number: u64,
    pub pr_url: &'a str,
    pub branch: &'a str,
    pub head_commit: &'a str,
}

pub fn marker(intent_id: &str) -> String {
    format!("{IMPLEMENTATION_COMMENT_MARKER_PREFIX}{intent_id} -->")
}

pub fn pr_marker(intent_id: &str) -> String {
    format!("{IMPLEMENTATION_PR_MARKER_PREFIX}{intent_id} -->")
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

pub fn render_publication_pending_comment(
    input: ImplementationPublicationPendingComment<'_>,
) -> String {
    let mut out = String::new();
    out.push_str(&marker(input.intent_id));
    out.push_str("\n## Symphony implementation\n\n");
    out.push_str("**Publication:** pending\n\n");
    out.push_str(&format!(
        "**Factory run:** `{}` · **Stage run:** `{}` · **Artifact:** `{}`\n\n",
        input.run_id, input.stage_run_id, input.artifact_id
    ));
    out.push_str("Symphony is publishing the verified change bundle to a draft pull request.\n");
    out
}

pub fn render_publication_final_comment(
    input: ImplementationPublicationFinalComment<'_>,
) -> String {
    let mut out = String::new();
    out.push_str(&marker(input.intent_id));
    out.push_str("\n## Symphony implementation\n\n");
    out.push_str("**Publication:** draft PR created — Agent Review\n\n");
    out.push_str(&format!(
        "**Pull request:** [#{}]({})\n\n",
        input.pr_number, input.pr_url
    ));
    out.push_str(&format!(
        "**Branch:** `{}` @ `{}`\n\n",
        input.branch,
        abbreviate(input.head_commit)
    ));
    out.push_str(&format!(
        "**Factory run:** `{}` · **Stage run:** `{}` · **Artifact:** `{}`\n",
        input.run_id, input.stage_run_id, input.artifact_id
    ));
    out
}

pub fn render_draft_pr_body(input: ImplementationDraftPrBody<'_>) -> String {
    let mut out = String::new();
    out.push_str(&pr_marker(input.intent_id));
    out.push('\n');
    out.push_str(&format!("Closes #{}\n\n", input.issue_number));
    out.push_str("## Symphony implementation\n\n");
    out.push_str(&format!(
        "**Factory run:** `{}` · **Stage run:** `{}`\n\n",
        input.run_id, input.stage_run_id
    ));
    out.push_str(&format!(
        "**Implementation artifact:** `{}` · **Bundle:** `{}`\n\n",
        input.artifact_id, input.bundle_artifact_id
    ));
    out.push_str(&format!(
        "**Approved spec:** `{}` v{} at `{}`\n\n",
        input.approved_artifact_id, input.approved_version, input.approved_spec_path
    ));
    out.push_str(&format!(
        "**Commits:** `{}` → `{}`\n\n",
        input.base_commit, input.head_commit
    ));
    out.push_str("### Summary\n\n");
    out.push_str(input.manifest.summary.trim());
    out.push_str("\n\n### Acceptance criteria\n\n");
    render_criteria(&mut out, &input.manifest.acceptance_criteria);
    out.push_str("\n### Validation\n\n");
    if input.validation.is_empty() {
        out.push_str("_No validation commands recorded._\n");
    } else {
        for command in input.validation {
            let status = if command.passed { "pass" } else { "fail" };
            out.push_str(&format!(
                "- `{}`: **{}** in {} ms\n",
                command.name, status, command.duration_ms
            ));
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
    out
}

pub fn extract_implementation_pr_intent_id(body: &str) -> Option<String> {
    let start = body.find(IMPLEMENTATION_PR_MARKER_PREFIX)? + IMPLEMENTATION_PR_MARKER_PREFIX.len();
    let rest = &body[start..];
    let end = rest.find(" -->")?;
    let intent_id = rest[..end].trim();
    (!intent_id.is_empty()).then(|| intent_id.to_string())
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
        CriterionStatus, EvidenceKind, ImplementationEvidence, ManifestStatus,
    };

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
                    reference: "src/x.rs".into(),
                    summary: "bound".into(),
                }],
            }],
            known_limitations: vec![],
            blocker: None,
        };
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
            changed_paths: &["src/x.rs".into()],
            validation: &[],
        });
        assert!(body.contains("<!-- symphony:implementation:intent-1 -->"));
        assert!(body.contains("No remote branch or pull request was created"));
        assert!(!body.contains("<!-- symphony:implementation-pr:"));
    }

    #[test]
    fn renders_draft_pr_body_with_ownership_marker_and_closes() {
        let manifest = ImplementationManifest {
            schema_version: 1,
            status: ManifestStatus::Completed,
            head_commit: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into()),
            summary: "Implements feature.".into(),
            acceptance_criteria: vec![],
            known_limitations: vec!["no docs".into()],
            blocker: None,
        };
        let body = render_draft_pr_body(ImplementationDraftPrBody {
            intent_id: "intent-pr",
            issue_number: 42,
            run_id: "run",
            stage_run_id: "stage",
            artifact_id: "impl",
            bundle_artifact_id: "bundle",
            approved_artifact_id: "spec",
            approved_version: 1,
            approved_spec_path: "specs/42/APPROVED-v1.md",
            base_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            head_commit: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            manifest: &manifest,
            validation: &[],
        });
        assert!(body.starts_with("<!-- symphony:implementation-pr:intent-pr -->"));
        assert!(body.contains("Closes #42"));
        assert!(body.contains("no docs"));
        assert_eq!(
            extract_implementation_pr_intent_id(&body).as_deref(),
            Some("intent-pr")
        );
    }

    #[test]
    fn renders_final_comment_agent_review() {
        let body = render_publication_final_comment(ImplementationPublicationFinalComment {
            intent_id: "i",
            run_id: "r",
            stage_run_id: "s",
            artifact_id: "a",
            pr_number: 9,
            pr_url: "https://github.com/acme/repo/pull/9",
            branch: "symphony/42",
            head_commit: "cccccccccccccccccccccccccccccccccccccccc",
        });
        assert!(body.contains("draft PR created — Agent Review"));
        assert!(body.contains("#9"));
    }
}
