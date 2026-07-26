use crate::spec::domain::{ReviewFinding, SpecArtifact, SPEC_COMMENT_MARKER_PREFIX};

#[derive(Debug, Clone)]
pub enum SpecCommentState<'a> {
    Preview,
    AwaitingDecision {
        approved_label: &'a str,
        revise_label: &'a str,
    },
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
        SpecCommentState::AwaitingDecision {
            approved_label,
            revise_label,
        } => out.push_str(&format!(
            "Apply `{approved_label}` to approve, or add a feedback comment and apply `{revise_label}` to request changes.\n",
        )),
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
    use crate::spec::domain::{FindingSection, FindingSeverity, ReviewFinding, SpecArtifact};

    fn artifact() -> SpecArtifact {
        SpecArtifact {
            schema_version: 1,
            product_behavior: "Behavior".to_string(),
            technical_approach: "Approach".to_string(),
            acceptance_criteria: vec!["Observable".to_string()],
            open_decisions: vec!["Open item".to_string()],
        }
    }

    fn render(state: SpecCommentState<'_>) -> String {
        render_spec_comment("intent", "run", "stage", 1, 2, &artifact(), &[], state)
    }

    #[test]
    fn renders_owned_versioned_preview() {
        let body = render(SpecCommentState::Preview);
        assert!(body.starts_with("<!-- symphony:spec:intent -->"));
        assert!(body.contains("**Version:** 2"));
        assert!(body.contains("### Product behavior"));
        assert!(body.contains("**Preview:**"));
        assert!(body.contains("- Open item"));
    }

    #[test]
    fn renders_awaiting_decision_and_approval_states() {
        assert!(render(SpecCommentState::AwaitingDecision {
            approved_label: "custom-approved",
            revise_label: "custom-revise",
        })
        .contains("`custom-approved`"));
        assert!(render(SpecCommentState::AwaitingDecision {
            approved_label: "custom-approved",
            revise_label: "custom-revise",
        })
        .contains("`custom-revise`"));
        assert!(render(SpecCommentState::ApprovalPending).contains("Approval pending"));
        assert!(render(SpecCommentState::Diagnostic("Add feedback."))
            .contains("**Action required:** Add feedback."));

        let with_state = render(SpecCommentState::Approved {
            route_label: "ready-for-agent",
            project_state: Some("Todo"),
        });
        assert!(with_state.contains("implementation-ready"));
        assert!(with_state.contains("`ready-for-agent`"));
        assert!(with_state.contains("`Todo`"));

        let without_state = render(SpecCommentState::Approved {
            route_label: "ready-for-agent",
            project_state: None,
        });
        assert!(without_state.contains("`ready-for-agent`"));
        assert!(!without_state.contains("project state"));
    }

    #[test]
    fn renders_unresolved_blocking_findings_and_empty_open_decisions() {
        let body = render_spec_comment(
            "intent",
            "run",
            "stage",
            3,
            1,
            &SpecArtifact {
                open_decisions: vec![],
                ..artifact()
            },
            &[ReviewFinding {
                severity: FindingSeverity::Blocking,
                section: FindingSection::AcceptanceCriteria,
                summary: "Not observable".to_string(),
                recommendation: "Name the signal".to_string(),
            }],
            SpecCommentState::AwaitingDecision {
                approved_label: "spec-approved",
                revise_label: "spec-revise",
            },
        );
        assert!(body.contains("_None._"));
        assert!(body.contains("Unresolved blocking review findings"));
        assert!(body.contains("Not observable"));
        assert!(body.contains("Name the signal"));
    }
}
