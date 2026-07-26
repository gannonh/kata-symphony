use crate::spec::domain::{ReviewFinding, SpecArtifact, SPEC_COMMENT_MARKER_PREFIX};

#[derive(Debug, Clone)]
pub enum SpecCommentState<'a> {
    Preview,
    AwaitingDecision,
    Diagnostic(&'a str),
    ApprovalPending,
    Approved {
        route_label: &'a str,
        project_state: Option<&'a str>,
    },
}

pub fn marker(intent_id: &str) -> String {
    format!("{SPEC_COMMENT_MARKER_PREFIX}{intent_id} -->")
}

#[allow(clippy::too_many_arguments)]
pub fn render_spec_comment(
    intent_id: &str,
    run_id: &str,
    stage_run_id: &str,
    attempt: u32,
    version: u32,
    artifact: &SpecArtifact,
    unresolved: &[ReviewFinding],
    state: SpecCommentState<'_>,
) -> String {
    let mut out = String::new();
    out.push_str(&marker(intent_id));
    out.push_str("\n## Symphony specification\n\n");
    out.push_str(&format!(
        "**Version:** {version} · **Factory run:** `{run_id}` · **Attempt:** {attempt} (`{stage_run_id}`)\n\n"
    ));
    out.push_str("### Product behavior\n\n");
    out.push_str(artifact.product_behavior.trim());
    out.push_str("\n\n### Technical approach\n\n");
    out.push_str(artifact.technical_approach.trim());
    out.push_str("\n\n### Acceptance criteria\n\n");
    for criterion in &artifact.acceptance_criteria {
        out.push_str("- ");
        out.push_str(criterion.trim());
        out.push('\n');
    }
    out.push_str("\n### Open decisions\n\n");
    if artifact.open_decisions.is_empty() {
        out.push_str("_None._\n");
    } else {
        for decision in &artifact.open_decisions {
            out.push_str("- ");
            out.push_str(decision.trim());
            out.push('\n');
        }
    }
    if !unresolved.is_empty() {
        out.push_str("\n### Unresolved blocking review findings\n\n");
        for finding in unresolved {
            out.push_str(&format!(
                "- **{:?}:** {} — {}\n",
                finding.section,
                finding.summary.trim(),
                finding.recommendation.trim()
            ));
        }
    }
    out.push_str("\n---\n");
    match state {
        SpecCommentState::Preview => out.push_str(
            "**Preview:** Symphony does not act on decision labels yet. Review this specification and leave feedback.\n",
        ),
        SpecCommentState::AwaitingDecision => out.push_str(
            "Apply `spec-approved` to approve, or add a feedback comment and apply `spec-revise` to request changes.\n",
        ),
        SpecCommentState::Diagnostic(message) => {
            out.push_str("**Action required:** ");
            out.push_str(message.trim());
            out.push('\n');
        }
        SpecCommentState::ApprovalPending => out.push_str(&format!(
            "**Approval pending:** version {version} is being routed to implementation.\n"
        )),
        SpecCommentState::Approved {
            route_label,
            project_state,
        } => {
            out.push_str(&format!(
                "**Approved — implementation-ready:** version {version}; applied `{route_label}`"
            ));
            if let Some(state) = project_state {
                out.push_str(&format!(" and project state `{state}`"));
            }
            out.push_str(".\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::domain::SpecArtifact;

    #[test]
    fn renders_owned_versioned_preview() {
        let body = render_spec_comment(
            "intent",
            "run",
            "stage",
            1,
            2,
            &SpecArtifact {
                schema_version: 1,
                product_behavior: "Behavior".to_string(),
                technical_approach: "Approach".to_string(),
                acceptance_criteria: vec!["Observable".to_string()],
                open_decisions: vec![],
            },
            &[],
            SpecCommentState::Preview,
        );
        assert!(body.starts_with("<!-- symphony:spec:intent -->"));
        assert!(body.contains("**Version:** 2"));
        assert!(body.contains("### Product behavior"));
        assert!(body.contains("**Preview:**"));
    }
}
