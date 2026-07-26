use std::fmt;

use serde::de::DeserializeOwned;

use crate::spec::domain::{
    FindingSeverity, ReviewFindings, ReviewVerdict, SpecArtifact,
    SPEC_ACCEPTANCE_CRITERIA_MAX_ITEMS, SPEC_ARTIFACT_MAX_BYTES, SPEC_FINDINGS_MAX_ITEMS,
    SPEC_LIST_ITEM_MAX_BYTES, SPEC_OPEN_DECISIONS_MAX_ITEMS, SPEC_SCHEMA_VERSION,
    SPEC_SECTION_MAX_BYTES,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecValidationError {
    pub field: String,
    pub message: String,
}

impl SpecValidationError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for SpecValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for SpecValidationError {}

pub fn parse_spec(bytes: &[u8]) -> Result<SpecArtifact, SpecValidationError> {
    let artifact: SpecArtifact = parse_bounded(bytes)?;
    validate_spec(&artifact)?;
    Ok(artifact)
}

pub fn parse_findings(bytes: &[u8]) -> Result<ReviewFindings, SpecValidationError> {
    let findings: ReviewFindings = parse_bounded(bytes)?;
    validate_findings(&findings)?;
    Ok(findings)
}

fn parse_bounded<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, SpecValidationError> {
    if bytes.len() > SPEC_ARTIFACT_MAX_BYTES {
        return Err(SpecValidationError::new(
            "output",
            format!(
                "artifact is {} bytes; maximum is {} bytes",
                bytes.len(),
                SPEC_ARTIFACT_MAX_BYTES
            ),
        ));
    }
    serde_json::from_slice(bytes)
        .map_err(|error| SpecValidationError::new("output", format!("invalid JSON: {error}")))
}

pub fn validate_spec(artifact: &SpecArtifact) -> Result<(), SpecValidationError> {
    if artifact.schema_version != SPEC_SCHEMA_VERSION {
        return Err(SpecValidationError::new(
            "schema_version",
            format!("must be exactly {SPEC_SCHEMA_VERSION}"),
        ));
    }
    validate_text(
        "product_behavior",
        &artifact.product_behavior,
        SPEC_SECTION_MAX_BYTES,
    )?;
    validate_text(
        "technical_approach",
        &artifact.technical_approach,
        SPEC_SECTION_MAX_BYTES,
    )?;
    validate_list(
        "acceptance_criteria",
        &artifact.acceptance_criteria,
        1,
        SPEC_ACCEPTANCE_CRITERIA_MAX_ITEMS,
    )?;
    validate_list(
        "open_decisions",
        &artifact.open_decisions,
        0,
        SPEC_OPEN_DECISIONS_MAX_ITEMS,
    )?;
    Ok(())
}

pub fn validate_findings(findings: &ReviewFindings) -> Result<(), SpecValidationError> {
    if findings.schema_version != SPEC_SCHEMA_VERSION {
        return Err(SpecValidationError::new(
            "schema_version",
            format!("must be exactly {SPEC_SCHEMA_VERSION}"),
        ));
    }
    if findings.findings.len() > SPEC_FINDINGS_MAX_ITEMS {
        return Err(SpecValidationError::new(
            "findings",
            format!("must contain at most {SPEC_FINDINGS_MAX_ITEMS} items"),
        ));
    }
    for (index, finding) in findings.findings.iter().enumerate() {
        validate_text(
            &format!("findings[{index}].summary"),
            &finding.summary,
            SPEC_LIST_ITEM_MAX_BYTES,
        )?;
        validate_text(
            &format!("findings[{index}].recommendation"),
            &finding.recommendation,
            SPEC_LIST_ITEM_MAX_BYTES,
        )?;
    }
    let blocking = findings
        .findings
        .iter()
        .filter(|finding| finding.severity == FindingSeverity::Blocking)
        .count();
    match findings.verdict {
        ReviewVerdict::Pass if blocking > 0 => Err(SpecValidationError::new(
            "verdict",
            "'pass' requires zero blocking findings",
        )),
        ReviewVerdict::Revise if blocking == 0 => Err(SpecValidationError::new(
            "verdict",
            "'revise' requires at least one blocking finding",
        )),
        _ => Ok(()),
    }
}

fn validate_text(field: &str, value: &str, max_bytes: usize) -> Result<(), SpecValidationError> {
    if value.trim().is_empty() {
        return Err(SpecValidationError::new(field, "must be non-empty"));
    }
    if value.len() > max_bytes {
        return Err(SpecValidationError::new(
            field,
            format!("must be at most {max_bytes} UTF-8 bytes"),
        ));
    }
    Ok(())
}

fn validate_list(
    field: &str,
    values: &[String],
    min: usize,
    max: usize,
) -> Result<(), SpecValidationError> {
    if values.len() < min || values.len() > max {
        return Err(SpecValidationError::new(
            field,
            format!("must contain {min} to {max} items"),
        ));
    }
    for (index, value) in values.iter().enumerate() {
        validate_text(
            &format!("{field}[{index}]"),
            value,
            SPEC_LIST_ITEM_MAX_BYTES,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::domain::{FindingSection, ReviewFinding};

    fn valid_spec() -> SpecArtifact {
        SpecArtifact {
            schema_version: 1,
            product_behavior: "Users receive a spec.".to_string(),
            technical_approach: "Add a durable stage.".to_string(),
            acceptance_criteria: vec!["A comment is published.".to_string()],
            open_decisions: vec![],
        }
    }

    #[test]
    fn strict_json_rejects_unknown_fields() {
        let json = br#"{"schema_version":1,"product_behavior":"p","technical_approach":"t","acceptance_criteria":["a"],"open_decisions":[],"extra":true}"#;
        assert!(parse_spec(json)
            .unwrap_err()
            .message
            .contains("unknown field"));
    }

    #[test]
    fn spec_bounds_are_enforced() {
        let mut artifact = valid_spec();
        artifact.acceptance_criteria.clear();
        assert_eq!(
            validate_spec(&artifact).unwrap_err().field,
            "acceptance_criteria"
        );
        artifact = valid_spec();
        artifact.product_behavior = "x".repeat(SPEC_SECTION_MAX_BYTES + 1);
        assert_eq!(
            validate_spec(&artifact).unwrap_err().field,
            "product_behavior"
        );
    }

    #[test]
    fn verdict_must_match_blocking_findings() {
        let findings = ReviewFindings {
            schema_version: 1,
            verdict: ReviewVerdict::Pass,
            findings: vec![ReviewFinding {
                severity: FindingSeverity::Blocking,
                section: FindingSection::General,
                summary: "Missing detail".to_string(),
                recommendation: "Add detail".to_string(),
            }],
        };
        assert_eq!(validate_findings(&findings).unwrap_err().field, "verdict");
    }
}
