use crate::triage::domain::{
    EvidenceKind, ReproductionSummary, RiskClass, TriageArtifact, TriageEvidence, TriageRoute,
    ARTIFACT_CLARIFICATION_MAX_BYTES, ARTIFACT_EVIDENCE_MAX_ITEMS, ARTIFACT_EVIDENCE_MIN_ITEMS,
    ARTIFACT_EVIDENCE_REFERENCE_MAX_BYTES, ARTIFACT_EVIDENCE_SUMMARY_MAX_BYTES, ARTIFACT_MAX_BYTES,
    ARTIFACT_NEXT_ACTION_MAX_BYTES, ARTIFACT_RATIONALE_MAX_BYTES,
    ARTIFACT_REPRODUCTION_OUTCOME_MAX_BYTES, TRIAGE_SCHEMA_VERSION,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactValidationError {
    TooLarge {
        max_bytes: usize,
    },
    InvalidUtf8,
    InvalidJson(String),
    InvalidSchemaVersion(u32),
    EmptyString(&'static str),
    StringTooLong {
        field: &'static str,
        max_bytes: usize,
    },
    EvidenceCount {
        min: usize,
        max: usize,
    },
    DuplicateEvidence {
        kind: EvidenceKind,
        reference: String,
    },
    ClarificationRequired,
    ClarificationNotAllowed,
}

impl fmt::Display for ArtifactValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { max_bytes } => write!(f, "artifact exceeds {max_bytes} bytes"),
            Self::InvalidUtf8 => f.write_str("artifact is not valid UTF-8"),
            Self::InvalidJson(err) => write!(f, "artifact JSON is invalid: {err}"),
            Self::InvalidSchemaVersion(version) => {
                write!(f, "artifact schema_version must be 1, got {version}")
            }
            Self::EmptyString(field) => write!(f, "artifact field {field} must be non-empty"),
            Self::StringTooLong { field, max_bytes } => {
                write!(f, "artifact field {field} exceeds {max_bytes} bytes")
            }
            Self::EvidenceCount { min, max } => {
                write!(f, "artifact evidence count must be between {min} and {max}")
            }
            Self::DuplicateEvidence { kind, reference } => {
                write!(f, "duplicate evidence entry {kind}:{reference}")
            }
            Self::ClarificationRequired => {
                f.write_str("needs_information artifacts require clarification_question")
            }
            Self::ClarificationNotAllowed => {
                f.write_str("clarification_question is allowed only for needs_information")
            }
        }
    }
}

impl std::error::Error for ArtifactValidationError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTriageArtifact {
    schema_version: u32,
    route: TriageRoute,
    risk_class: RiskClass,
    rationale: String,
    evidence: Vec<RawTriageEvidence>,
    next_action: String,
    clarification_question: Option<String>,
    reproduction: Option<RawReproductionSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTriageEvidence {
    kind: EvidenceKind,
    reference: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReproductionSummary {
    attempted: bool,
    outcome: String,
}

pub fn parse_and_validate_artifact(
    bytes: &[u8],
) -> Result<TriageArtifact, ArtifactValidationError> {
    if bytes.len() > ARTIFACT_MAX_BYTES {
        return Err(ArtifactValidationError::TooLarge {
            max_bytes: ARTIFACT_MAX_BYTES,
        });
    }

    let text = std::str::from_utf8(bytes).map_err(|_| ArtifactValidationError::InvalidUtf8)?;
    let raw: RawTriageArtifact = serde_json::from_str(text)
        .map_err(|err| ArtifactValidationError::InvalidJson(err.to_string()))?;

    validate_raw(raw)
}

/// Alias for triage runner and publisher call sites.
pub fn parse_and_validate(bytes: &[u8]) -> Result<TriageArtifact, ArtifactValidationError> {
    parse_and_validate_artifact(bytes)
}

fn validate_raw(raw: RawTriageArtifact) -> Result<TriageArtifact, ArtifactValidationError> {
    if raw.schema_version != TRIAGE_SCHEMA_VERSION {
        return Err(ArtifactValidationError::InvalidSchemaVersion(
            raw.schema_version,
        ));
    }

    validate_bounded_string("rationale", &raw.rationale, ARTIFACT_RATIONALE_MAX_BYTES)?;
    validate_bounded_string(
        "next_action",
        &raw.next_action,
        ARTIFACT_NEXT_ACTION_MAX_BYTES,
    )?;

    if raw.evidence.len() < ARTIFACT_EVIDENCE_MIN_ITEMS
        || raw.evidence.len() > ARTIFACT_EVIDENCE_MAX_ITEMS
    {
        return Err(ArtifactValidationError::EvidenceCount {
            min: ARTIFACT_EVIDENCE_MIN_ITEMS,
            max: ARTIFACT_EVIDENCE_MAX_ITEMS,
        });
    }

    match (&raw.route, raw.clarification_question.as_deref()) {
        (TriageRoute::NeedsInformation, Some(question)) => {
            validate_bounded_string(
                "clarification_question",
                question,
                ARTIFACT_CLARIFICATION_MAX_BYTES,
            )?;
        }
        (TriageRoute::NeedsInformation, None) => {
            return Err(ArtifactValidationError::ClarificationRequired);
        }
        (_, Some(question)) => {
            validate_bounded_string(
                "clarification_question",
                question,
                ARTIFACT_CLARIFICATION_MAX_BYTES,
            )?;
            return Err(ArtifactValidationError::ClarificationNotAllowed);
        }
        (_, None) => {}
    }

    let mut seen = HashSet::new();
    let mut evidence = Vec::with_capacity(raw.evidence.len());
    for item in raw.evidence {
        validate_bounded_string(
            "evidence.reference",
            &item.reference,
            ARTIFACT_EVIDENCE_REFERENCE_MAX_BYTES,
        )?;
        validate_bounded_string(
            "evidence.summary",
            &item.summary,
            ARTIFACT_EVIDENCE_SUMMARY_MAX_BYTES,
        )?;

        let normalized_key = format!(
            "{}:{}",
            item.kind.as_str(),
            item.reference
                .trim()
                .replace("\r\n", "\n")
                .to_ascii_lowercase()
        );
        if !seen.insert(normalized_key) {
            return Err(ArtifactValidationError::DuplicateEvidence {
                kind: item.kind,
                reference: item.reference,
            });
        }

        evidence.push(TriageEvidence {
            kind: item.kind,
            reference: item.reference,
            summary: item.summary,
        });
    }

    let reproduction = raw
        .reproduction
        .map(|reproduction| {
            validate_bounded_string(
                "reproduction.outcome",
                &reproduction.outcome,
                ARTIFACT_REPRODUCTION_OUTCOME_MAX_BYTES,
            )?;
            Ok(ReproductionSummary {
                attempted: reproduction.attempted,
                outcome: reproduction.outcome,
            })
        })
        .transpose()?;

    Ok(TriageArtifact {
        schema_version: raw.schema_version,
        route: raw.route,
        risk_class: raw.risk_class,
        rationale: raw.rationale,
        evidence,
        next_action: raw.next_action,
        clarification_question: raw.clarification_question,
        reproduction,
    })
}

fn validate_bounded_string(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), ArtifactValidationError> {
    if value.trim().is_empty() {
        return Err(ArtifactValidationError::EmptyString(field));
    }
    if value.len() > max_bytes {
        return Err(ArtifactValidationError::StringTooLong { field, max_bytes });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_json(route: &str, question: serde_json::Value) -> Vec<u8> {
        serde_json::json!({
            "schema_version": 1,
            "route": route,
            "risk_class": "medium",
            "rationale": "The issue is bounded but needs validation.",
            "evidence": [
                {
                    "kind": "issue",
                    "reference": "body",
                    "summary": "The issue names the expected behavior."
                }
            ],
            "next_action": "Proceed with the selected route.",
            "clarification_question": question,
            "reproduction": {
                "attempted": true,
                "outcome": "The scenario can be exercised locally."
            }
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn accepts_valid_needs_information_artifact() {
        let artifact = parse_and_validate_artifact(&valid_json(
            "needs_information",
            serde_json::json!("Which fallback behavior should be used?"),
        ))
        .expect("valid artifact");

        assert_eq!(artifact.route, TriageRoute::NeedsInformation);
        assert_eq!(
            artifact.clarification_question.as_deref(),
            Some("Which fallback behavior should be used?")
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&valid_json("implement", serde_json::Value::Null)).unwrap();
        value["extra"] = serde_json::json!(true);

        assert!(matches!(
            parse_and_validate_artifact(value.to_string().as_bytes()),
            Err(ArtifactValidationError::InvalidJson(_))
        ));
    }

    #[test]
    fn rejects_duplicate_evidence_after_normalization() {
        let value = serde_json::json!({
            "schema_version": 1,
            "route": "implement",
            "risk_class": "low",
            "rationale": "Bounded.",
            "evidence": [
                {"kind": "issue", "reference": "Body", "summary": "One."},
                {"kind": "issue", "reference": " body ", "summary": "Two."}
            ],
            "next_action": "Implement.",
            "clarification_question": null,
            "reproduction": null
        });

        assert!(matches!(
            parse_and_validate_artifact(value.to_string().as_bytes()),
            Err(ArtifactValidationError::DuplicateEvidence { .. })
        ));
    }

    #[test]
    fn enforces_clarification_rules() {
        assert!(matches!(
            parse_and_validate_artifact(&valid_json("needs_information", serde_json::Value::Null)),
            Err(ArtifactValidationError::ClarificationRequired)
        ));

        assert!(matches!(
            parse_and_validate_artifact(&valid_json("implement", serde_json::json!("Not allowed"))),
            Err(ArtifactValidationError::ClarificationNotAllowed)
        ));
    }

    #[test]
    fn enforces_bounds_and_empty_strings() {
        let too_large = vec![b' '; ARTIFACT_MAX_BYTES + 1];
        assert!(matches!(
            parse_and_validate_artifact(&too_large),
            Err(ArtifactValidationError::TooLarge { .. })
        ));

        let value = serde_json::json!({
            "schema_version": 1,
            "route": "implement",
            "risk_class": "low",
            "rationale": "   ",
            "evidence": [{"kind": "issue", "reference": "body", "summary": "Summary."}],
            "next_action": "Implement.",
            "clarification_question": null,
            "reproduction": null
        });

        assert!(matches!(
            parse_and_validate_artifact(value.to_string().as_bytes()),
            Err(ArtifactValidationError::EmptyString("rationale"))
        ));
    }
}
