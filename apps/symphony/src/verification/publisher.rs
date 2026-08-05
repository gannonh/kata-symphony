//! Durable marker-owned preview publication for the verification stage.
//!
//! Preview mode publishes exactly one owned issue comment and performs no
//! other tracker or PR mutation. The comment is keyed by a durable marker so a
//! restart updates the same comment instead of duplicating it.

use crate::error::Result;
use crate::triage::domain::PublicationStatus;
use crate::github::client::GithubIssueComment;
use crate::triage::publisher::TriageCommentPort;
use crate::triage::runtime::SharedFactoryStore;
use crate::verification::domain::{
    VerificationCommandRunRecord, VerificationEvidenceRecord, VerificationGateRecord,
    VerificationPublicationIntent, VERIFICATION_COMMENT_MARKER_PREFIX,
    VERIFICATION_COMMENT_MARKER_SUFFIX,
};

pub fn verification_marker(intent_id: &str) -> String {
    format!("{VERIFICATION_COMMENT_MARKER_PREFIX}{intent_id}{VERIFICATION_COMMENT_MARKER_SUFFIX}")
}

/// Render the owned preview summary. Only digests and metadata — never blob
/// bytes — and stable HTTP links for the run.
pub fn render_verification_preview_comment(
    intent_id: &str,
    run_id: &str,
    attempt_id: &str,
    pr_number: u64,
    reviewed_head_sha: &str,
    gate: &VerificationGateRecord,
    commands: &[VerificationCommandRunRecord],
    evidence: &[VerificationEvidenceRecord],
) -> String {
    let marker = verification_marker(intent_id);
    let mut lines = vec![
        format!("{marker}"),
        "## Verification evidence preview".to_string(),
        String::new(),
        format!(
            "Gate: **{}** — computed by Symphony from durable command outcomes and complete criterion coverage.",
            gate.status
        ),
        String::new(),
        "### Commands".to_string(),
        String::new(),
        "| command | kind | status | exit | output sha256 |".to_string(),
        "| --- | --- | --- | --- | --- |".to_string(),
    ];
    for command in commands {
        let exit = command
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_string());
        let digest = command
            .output_sha256
            .as_deref()
            .unwrap_or("-")
            .chars()
            .take(12)
            .collect::<String>();
        lines.push(format!(
            "| {} | {} | {} | {} | {} |",
            command.name, command.kind.as_str(), command.status, exit, digest
        ));
    }
    if let Some(manifest) = &gate.verifier_manifest {
        lines.push(String::new());
        lines.push("### Acceptance criteria".to_string());
        lines.push(String::new());
        lines.push("| criterion | status | evidence |".to_string());
        lines.push("| --- | --- | --- |".to_string());
        for criterion in &manifest.criteria {
            lines.push(format!(
                "| {} | {} | {} |",
                criterion.index,
                criterion.status.as_str(),
                criterion.evidence.join(", ")
            ));
        }
    }
    if !evidence.is_empty() {
        lines.push(String::new());
        lines.push("### Evidence".to_string());
        lines.push(String::new());
        for record in evidence {
            lines.push(format!(
                "- `{}` sha256 `{}` ({} bytes)",
                record.relative_path,
                record
                    .sha256
                    .chars()
                    .take(12)
                    .collect::<String>(),
                record.bytes_len
            ));
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "Run `{run_id}` attempt `{attempt_id}` reviewed head `{reviewed_head_sha}` (PR #{pr_number})."
    ));
    lines.push(format!(
        "State: `/api/v1/verification/runs/{run_id}` · Evidence metadata: `/api/v1/verification/runs/{run_id}/evidence`"
    ));
    lines.join("\n")
}

/// Create or update the marker-owned comment and mark the intent applied.
pub async fn publish_preview_comment<C: TriageCommentPort>(
    comments: &C,
    store: &SharedFactoryStore,
    intent: &VerificationPublicationIntent,
    run_id: &str,
    attempt_id: &str,
    pr_number: u64,
    reviewed_head_sha: &str,
    gate: &VerificationGateRecord,
    commands: &[VerificationCommandRunRecord],
    evidence: &[VerificationEvidenceRecord],
    max_pages: u32,
) -> Result<String> {
    if intent.status == PublicationStatus::Applied {
        if let Some(comment_id) = intent.comment_id.as_deref() {
            return Ok(comment_id.to_string());
        }
    }
    let marker = verification_marker(&intent.intent_id);
    let body = render_verification_preview_comment(
        &intent.intent_id,
        run_id,
        attempt_id,
        pr_number,
        reviewed_head_sha,
        gate,
        commands,
        evidence,
    );
    let mut owned: Option<GithubIssueComment> = None;
    let mut page = 0;
    loop {
        let comments_page = comments.list_comments(pr_number, max_pages).await?;
        for comment in comments_page {
            if comment
                .body
                .as_deref()
                .is_some_and(|body| body.contains(&marker))
            {
                owned = Some(comment);
                break;
            }
        }
        if owned.is_some() {
            break;
        }
        page += 1;
        if page >= max_pages {
            break;
        }
    }

    let comment_id = match owned {
        Some(existing) => {
            let updated = comments.update_comment(existing.id, &body).await?;
            updated.id.to_string()
        }
        None => {
            let created = comments.create_comment(pr_number, &body).await?;
            created.id.to_string()
        }
    };
    store.mark_verification_publication_applied(&intent.intent_id, &comment_id)?;
    Ok(comment_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::domain::{
        VerifierCriterion, VerifierCriterionStatus, VerifierManifest, VerificationCommandKind,
    };
    use chrono::Utc;

    fn gate() -> VerificationGateRecord {
        VerificationGateRecord {
            gate_id: "gate-1".to_string(),
            run_id: "run-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            status: "passed".to_string(),
            verifier_manifest: Some(VerifierManifest {
                schema_version: 1,
                spec_artifact_id: "spec".to_string(),
                implementation_artifact_id: "implementation".to_string(),
                review_artifact_id: "review".to_string(),
                reviewed_head_sha: "head".to_string(),
                base_sha: "base".to_string(),
                summary: "verified".to_string(),
                criteria: vec![VerifierCriterion {
                    index: 1,
                    status: VerifierCriterionStatus::Pass,
                    rationale: "ok".to_string(),
                    evidence: vec!["reports/ok.json".to_string()],
                }],
            }),
            command_summary: None,
            computed_at: Some(Utc::now()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn commands() -> Vec<VerificationCommandRunRecord> {
        vec![VerificationCommandRunRecord {
            command_run_id: "c1".to_string(),
            run_id: "run-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            ordinal: 1,
            name: "affected-validation".to_string(),
            kind: VerificationCommandKind::Test,
            configuration_revision: "cfg".to_string(),
            command_sha256: "sha".to_string(),
            status: "completed".to_string(),
            launch_nonce: None,
            pid: None,
            process_group_id: None,
            process_start_token: None,
            executable_identity: None,
            container_id: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            exit_code: Some(0),
            termination_reason: None,
            passed: Some(true),
            output_tail: None,
            output_sha256: Some("abcdef0123456789".to_string()),
            execution_profile: "local".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }]
    }

    #[test]
    fn rendered_comment_contains_marker_gate_commands_and_digests() {
        let body = render_verification_preview_comment(
            "intent-1",
            "run-1",
            "attempt-1",
            42,
            "head-sha",
            &gate(),
            &commands(),
            &[],
        );
        assert!(body.contains("<!-- symphony:verification:intent-1 -->"));
        assert!(body.contains("**passed**"));
        assert!(body.contains("affected-validation"));
        assert!(body.contains("abcdef012345"));
        assert!(body.contains("reports/ok.json"));
        assert!(!body.contains("sha256 `abcdef0123456789` (100 bytes)"));
    }
}
