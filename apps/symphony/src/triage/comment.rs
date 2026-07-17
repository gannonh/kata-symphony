use crate::triage::domain::{
    truncate_utf8_bytes, TriageArtifact, TRIAGE_COMMENT_MARKER_PREFIX, TRIAGE_COMMENT_MARKER_SUFFIX,
};

const COMMENT_FIELD_MAX_BYTES: usize = 2_000;
const COMMENT_EVIDENCE_MAX_ITEMS: usize = 20;

#[derive(Debug, Clone)]
pub struct PreviewCommentInput<'a> {
    pub intent_id: &'a str,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub attempt: u32,
    pub artifact: &'a TriageArtifact,
}

#[derive(Debug, Clone)]
pub struct IneligibleCommentInput<'a> {
    pub intent_id: &'a str,
    pub run_id: &'a str,
    pub issue_identifier: &'a str,
    pub project_name: &'a str,
    pub remediation: &'a str,
}

pub fn render_preview_comment(input: &PreviewCommentInput<'_>) -> String {
    let artifact = input.artifact;
    let mut body = String::new();
    body.push_str(&marker(input.intent_id));
    body.push_str("\n\n## Symphony triage preview\n\n");
    body.push_str("No labels or project state were changed.\n\n");
    body.push_str(&format!("- Route: `{}`\n", artifact.route));
    body.push_str(&format!("- Risk: `{}`\n", artifact.risk_class));
    body.push_str(&format!(
        "- Rationale: {}\n",
        bounded(&artifact.rationale, COMMENT_FIELD_MAX_BYTES)
    ));
    body.push_str(&format!(
        "- Next action: {}\n",
        bounded(&artifact.next_action, COMMENT_FIELD_MAX_BYTES)
    ));

    if let Some(question) = artifact.clarification_question.as_deref() {
        body.push_str(&format!(
            "- Clarification question: {}\n",
            bounded(question, COMMENT_FIELD_MAX_BYTES)
        ));
    }

    body.push_str("\n### Evidence\n");
    for evidence in artifact.evidence.iter().take(COMMENT_EVIDENCE_MAX_ITEMS) {
        body.push_str(&format!(
            "- `{}` `{}`: {}\n",
            evidence.kind,
            bounded(&evidence.reference, COMMENT_FIELD_MAX_BYTES),
            bounded(&evidence.summary, COMMENT_FIELD_MAX_BYTES)
        ));
    }

    if let Some(reproduction) = artifact.reproduction.as_ref() {
        body.push_str("\n### Reproduction\n");
        body.push_str(&format!("- Attempted: `{}`\n", reproduction.attempted));
        body.push_str(&format!(
            "- Outcome: {}\n",
            bounded(&reproduction.outcome, COMMENT_FIELD_MAX_BYTES)
        ));
    }

    body.push_str("\n### Run\n");
    body.push_str(&format!(
        "- Factory run: `{}`\n",
        bounded(input.run_id, 200)
    ));
    body.push_str(&format!(
        "- Stage attempt: `{}` attempt `{}`\n",
        bounded(input.stage_run_id, 200),
        input.attempt
    ));
    body.push_str(&format!(
        "- Publication intent: `{}`\n",
        bounded(input.intent_id, 200)
    ));

    body
}

pub fn render_ineligible_diagnostic_comment(input: &IneligibleCommentInput<'_>) -> String {
    format!(
        "{}\n\n## Symphony triage diagnostic\n\nIssue `{}` is not eligible for triage because it is not in project `{}`.\n\nRemediation: {}\n\nThe intake label remains unchanged and no agent attempt was started.\n\n- Factory run: `{}`\n- Publication intent: `{}`\n",
        marker(input.intent_id),
        bounded(input.issue_identifier, 200),
        bounded(input.project_name, COMMENT_FIELD_MAX_BYTES),
        bounded(input.remediation, COMMENT_FIELD_MAX_BYTES),
        bounded(input.run_id, 200),
        bounded(input.intent_id, 200)
    )
}

pub fn marker(intent_id: &str) -> String {
    format!(
        "{TRIAGE_COMMENT_MARKER_PREFIX}{}{TRIAGE_COMMENT_MARKER_SUFFIX}",
        bounded(intent_id, 200)
    )
}

pub fn extract_intent_id_from_marker(body: &str) -> Option<String> {
    let start = body.find(TRIAGE_COMMENT_MARKER_PREFIX)? + TRIAGE_COMMENT_MARKER_PREFIX.len();
    let rest = &body[start..];
    let end = rest.find(TRIAGE_COMMENT_MARKER_SUFFIX)?;
    let intent_id = rest[..end].trim();
    (!intent_id.is_empty()).then(|| intent_id.to_string())
}

fn bounded(value: &str, max_bytes: usize) -> String {
    truncate_utf8_bytes(value.trim(), max_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triage::domain::{
        EvidenceKind, RiskClass, TriageEvidence, TriageRoute, TRIAGE_SCHEMA_VERSION,
    };

    fn artifact() -> TriageArtifact {
        TriageArtifact {
            schema_version: TRIAGE_SCHEMA_VERSION,
            route: TriageRoute::NeedsInformation,
            risk_class: RiskClass::Medium,
            rationale: "Missing expected behavior.".to_string(),
            evidence: vec![TriageEvidence {
                kind: EvidenceKind::Issue,
                reference: "body".to_string(),
                summary: "Reporter asks for fallback behavior.".to_string(),
            }],
            next_action: "Ask for the fallback rule.".to_string(),
            clarification_question: Some("Which fallback should Symphony use?".to_string()),
            reproduction: None,
        }
    }

    #[test]
    fn renders_preview_marker_and_required_fields() {
        let body = render_preview_comment(&PreviewCommentInput {
            intent_id: "intent-123",
            run_id: "run-1",
            stage_run_id: "stage-1",
            attempt: 2,
            artifact: &artifact(),
        });

        assert!(body.contains("<!-- symphony:triage:intent-123 -->"));
        assert!(body.contains("Route: `needs_information`"));
        assert!(body.contains("No labels or project state were changed."));
        assert!(body.contains("Which fallback should Symphony use?"));
        assert_eq!(
            extract_intent_id_from_marker(&body).as_deref(),
            Some("intent-123")
        );
    }

    #[test]
    fn renders_ineligible_diagnostic() {
        let body = render_ineligible_diagnostic_comment(&IneligibleCommentInput {
            intent_id: "intent-off-project",
            run_id: "run-1",
            issue_identifier: "#12",
            project_name: "Factory",
            remediation: "Add the issue to the project.",
        });

        assert!(body.contains("not eligible for triage"));
        assert!(body.contains("Add the issue to the project."));
        assert_eq!(
            extract_intent_id_from_marker(&body).as_deref(),
            Some("intent-off-project")
        );
    }

    #[test]
    fn marker_extraction_ignores_missing_or_empty_markers() {
        assert_eq!(extract_intent_id_from_marker("no marker"), None);
        assert_eq!(
            extract_intent_id_from_marker("<!-- symphony:triage: -->"),
            None
        );
    }
}
