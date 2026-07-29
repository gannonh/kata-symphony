use std::fmt;

use serde::de::DeserializeOwned;

use crate::implementation::domain::{
    AcceptanceCriterionClaim, BlockerKind, ImplementationBlocker, ImplementationEvidence,
    ImplementationManifest, ManifestStatus, IMPLEMENTATION_BLOCKER_EVIDENCE_MAX_BYTES,
    IMPLEMENTATION_BLOCKER_SUMMARY_MAX_BYTES, IMPLEMENTATION_EVIDENCE_MAX_ITEMS,
    IMPLEMENTATION_EVIDENCE_REFERENCE_MAX_BYTES, IMPLEMENTATION_EVIDENCE_SUMMARY_MAX_BYTES,
    IMPLEMENTATION_LIMITATIONS_MAX_ITEMS, IMPLEMENTATION_LIMITATION_MAX_BYTES,
    IMPLEMENTATION_MANIFEST_MAX_BYTES, IMPLEMENTATION_SCHEMA_VERSION,
    IMPLEMENTATION_SPEC_PATH_MAX_BYTES, IMPLEMENTATION_SUMMARY_MAX_BYTES,
};
use crate::spec::domain::SpecArtifact;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationValidationError {
    pub field: String,
    pub message: String,
}

impl ImplementationValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ImplementationValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ImplementationValidationError {}

pub fn parse_manifest(
    bytes: &[u8],
) -> Result<ImplementationManifest, ImplementationValidationError> {
    let manifest: ImplementationManifest = parse_bounded(bytes)?;
    validate_manifest(&manifest, None)?;
    Ok(manifest)
}

pub fn parse_and_validate_manifest(
    bytes: &[u8],
    approved: &SpecArtifact,
) -> Result<ImplementationManifest, ImplementationValidationError> {
    let manifest: ImplementationManifest = parse_bounded(bytes)?;
    validate_manifest(&manifest, Some(approved))?;
    Ok(manifest)
}

fn parse_bounded<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ImplementationValidationError> {
    if bytes.len() > IMPLEMENTATION_MANIFEST_MAX_BYTES {
        return Err(ImplementationValidationError::new(
            "output",
            format!(
                "manifest is {} bytes; maximum is {} bytes",
                bytes.len(),
                IMPLEMENTATION_MANIFEST_MAX_BYTES
            ),
        ));
    }
    serde_json::from_slice(bytes).map_err(|error| {
        ImplementationValidationError::new("output", format!("invalid JSON: {error}"))
    })
}

pub fn validate_manifest(
    manifest: &ImplementationManifest,
    approved: Option<&SpecArtifact>,
) -> Result<(), ImplementationValidationError> {
    if manifest.schema_version != IMPLEMENTATION_SCHEMA_VERSION {
        return Err(ImplementationValidationError::new(
            "schema_version",
            format!("must be exactly {IMPLEMENTATION_SCHEMA_VERSION}"),
        ));
    }
    validate_text(
        "summary",
        &manifest.summary,
        IMPLEMENTATION_SUMMARY_MAX_BYTES,
    )?;
    if manifest.known_limitations.len() > IMPLEMENTATION_LIMITATIONS_MAX_ITEMS {
        return Err(ImplementationValidationError::new(
            "known_limitations",
            format!("must contain at most {IMPLEMENTATION_LIMITATIONS_MAX_ITEMS} items"),
        ));
    }
    for (index, limitation) in manifest.known_limitations.iter().enumerate() {
        validate_text(
            &format!("known_limitations[{index}]"),
            limitation,
            IMPLEMENTATION_LIMITATION_MAX_BYTES,
        )?;
    }

    match manifest.status {
        ManifestStatus::Completed => validate_completed(manifest, approved)?,
        ManifestStatus::Blocked => validate_blocked(manifest)?,
    }
    Ok(())
}

fn validate_completed(
    manifest: &ImplementationManifest,
    approved: Option<&SpecArtifact>,
) -> Result<(), ImplementationValidationError> {
    let head = manifest.head_commit.as_deref().ok_or_else(|| {
        ImplementationValidationError::new(
            "head_commit",
            "required for completed manifests".to_string(),
        )
    })?;
    validate_commit_oid("head_commit", head)?;
    if manifest.blocker.is_some() {
        return Err(ImplementationValidationError::new(
            "blocker",
            "must be absent for completed manifests".to_string(),
        ));
    }
    if let Some(approved) = approved {
        validate_criterion_coverage(&manifest.acceptance_criteria, approved)?;
    } else if manifest.acceptance_criteria.is_empty() {
        return Err(ImplementationValidationError::new(
            "acceptance_criteria",
            "must be non-empty for completed manifests".to_string(),
        ));
    } else {
        for (index, claim) in manifest.acceptance_criteria.iter().enumerate() {
            validate_claim(index, claim)?;
        }
    }
    Ok(())
}

fn validate_blocked(
    manifest: &ImplementationManifest,
) -> Result<(), ImplementationValidationError> {
    if manifest.head_commit.is_some() {
        return Err(ImplementationValidationError::new(
            "head_commit",
            "must be null for blocked manifests".to_string(),
        ));
    }
    if !manifest.acceptance_criteria.is_empty() {
        return Err(ImplementationValidationError::new(
            "acceptance_criteria",
            "must be empty for blocked manifests".to_string(),
        ));
    }
    let blocker = manifest.blocker.as_ref().ok_or_else(|| {
        ImplementationValidationError::new("blocker", "required for blocked manifests".to_string())
    })?;
    validate_blocker(blocker)
}

fn validate_criterion_coverage(
    claims: &[AcceptanceCriterionClaim],
    approved: &SpecArtifact,
) -> Result<(), ImplementationValidationError> {
    let expected = approved.acceptance_criteria.len();
    if claims.len() != expected {
        return Err(ImplementationValidationError::new(
            "acceptance_criteria",
            format!("must contain exactly {expected} entries matching the approved specification"),
        ));
    }
    let mut seen = vec![false; expected];
    for (index, claim) in claims.iter().enumerate() {
        validate_claim(index, claim)?;
        if claim.index == 0 || claim.index as usize > expected {
            return Err(ImplementationValidationError::new(
                format!("acceptance_criteria[{index}].index"),
                format!("must be between 1 and {expected}"),
            ));
        }
        let slot = (claim.index as usize) - 1;
        if seen[slot] {
            return Err(ImplementationValidationError::new(
                format!("acceptance_criteria[{index}].index"),
                format!("duplicate criterion index {}", claim.index),
            ));
        }
        seen[slot] = true;
    }
    if let Some((missing, _)) = seen.iter().enumerate().find(|(_, present)| !**present) {
        return Err(ImplementationValidationError::new(
            "acceptance_criteria",
            format!("missing criterion index {}", missing + 1),
        ));
    }
    Ok(())
}

fn validate_claim(
    index: usize,
    claim: &AcceptanceCriterionClaim,
) -> Result<(), ImplementationValidationError> {
    if claim.evidence.is_empty() || claim.evidence.len() > IMPLEMENTATION_EVIDENCE_MAX_ITEMS {
        return Err(ImplementationValidationError::new(
            format!("acceptance_criteria[{index}].evidence"),
            format!("must contain 1 to {IMPLEMENTATION_EVIDENCE_MAX_ITEMS} entries"),
        ));
    }
    for (evidence_index, evidence) in claim.evidence.iter().enumerate() {
        validate_evidence(index, evidence_index, evidence)?;
    }
    Ok(())
}

fn validate_evidence(
    claim_index: usize,
    evidence_index: usize,
    evidence: &ImplementationEvidence,
) -> Result<(), ImplementationValidationError> {
    validate_text(
        &format!("acceptance_criteria[{claim_index}].evidence[{evidence_index}].reference"),
        &evidence.reference,
        IMPLEMENTATION_EVIDENCE_REFERENCE_MAX_BYTES,
    )?;
    validate_text(
        &format!("acceptance_criteria[{claim_index}].evidence[{evidence_index}].summary"),
        &evidence.summary,
        IMPLEMENTATION_EVIDENCE_SUMMARY_MAX_BYTES,
    )?;
    Ok(())
}

fn validate_blocker(blocker: &ImplementationBlocker) -> Result<(), ImplementationValidationError> {
    let _ = blocker.kind;
    validate_text(
        "blocker.summary",
        &blocker.summary,
        IMPLEMENTATION_BLOCKER_SUMMARY_MAX_BYTES,
    )?;
    validate_text(
        "blocker.evidence",
        &blocker.evidence,
        IMPLEMENTATION_BLOCKER_EVIDENCE_MAX_BYTES,
    )?;
    Ok(())
}

pub fn validate_commit_oid(field: &str, value: &str) -> Result<(), ImplementationValidationError> {
    let trimmed = value.trim();
    if trimmed != value {
        return Err(ImplementationValidationError::new(
            field,
            "must not have leading or trailing whitespace".to_string(),
        ));
    }
    let valid_len = trimmed.len() == 40 || trimmed.len() == 64;
    let valid_chars = trimmed
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if !valid_len || !valid_chars {
        return Err(ImplementationValidationError::new(
            field,
            "must be a 40- or 64-character lowercase hexadecimal object ID".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_text(
    field: &str,
    value: &str,
    max_bytes: usize,
) -> Result<(), ImplementationValidationError> {
    if value.trim().is_empty() {
        return Err(ImplementationValidationError::new(
            field,
            "must be a non-empty string".to_string(),
        ));
    }
    if value.len() > max_bytes {
        return Err(ImplementationValidationError::new(
            field,
            format!("must be at most {max_bytes} UTF-8 bytes"),
        ));
    }
    Ok(())
}

/// Expand and validate the approved-spec path template.
pub fn expand_spec_file_path(
    template: &str,
    issue_identifier: &str,
    run_id: &str,
    artifact_id: &str,
    version: u32,
) -> Result<String, ImplementationValidationError> {
    let expanded = template
        .replace("{issue_identifier}", issue_identifier)
        .replace("{run_id}", run_id)
        .replace("{artifact_id}", artifact_id)
        .replace("{version}", &version.to_string());
    validate_spec_file_path(&expanded)?;
    Ok(expanded)
}

pub fn validate_spec_file_path(path: &str) -> Result<(), ImplementationValidationError> {
    if path.trim().is_empty() {
        return Err(ImplementationValidationError::new(
            "spec_file",
            "must be a non-empty relative path".to_string(),
        ));
    }
    if path.len() > IMPLEMENTATION_SPEC_PATH_MAX_BYTES {
        return Err(ImplementationValidationError::new(
            "spec_file",
            format!(
                "expanded path must be at most {IMPLEMENTATION_SPEC_PATH_MAX_BYTES} UTF-8 bytes"
            ),
        ));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(ImplementationValidationError::new(
            "spec_file",
            "must be relative to the repository root".to_string(),
        ));
    }
    if !path.ends_with(".md") {
        return Err(ImplementationValidationError::new(
            "spec_file",
            "must end in .md".to_string(),
        ));
    }
    let mut normalized = Vec::new();
    for component in path.split(['/', '\\']) {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(ImplementationValidationError::new(
                "spec_file",
                "must not contain '..' path segments".to_string(),
            ));
        }
        if component == ".git" || normalized.contains(&".git") {
            return Err(ImplementationValidationError::new(
                "spec_file",
                "must not address .git".to_string(),
            ));
        }
        normalized.push(component);
    }
    if normalized.is_empty() {
        return Err(ImplementationValidationError::new(
            "spec_file",
            "must identify a Markdown file".to_string(),
        ));
    }
    Ok(())
}

/// Deterministic approved-spec Markdown render for repository materialization.
pub fn render_approved_spec(
    run_id: &str,
    issue_identifier: &str,
    artifact_id: &str,
    version: u32,
    approved_at: &str,
    artifact: &SpecArtifact,
) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("symphony_factory_run: {run_id}\n"));
    out.push_str(&format!("symphony_issue: {issue_identifier}\n"));
    out.push_str(&format!("symphony_spec_artifact: {artifact_id}\n"));
    out.push_str(&format!("symphony_spec_version: {version}\n"));
    out.push_str(&format!("symphony_approved_at: {approved_at}\n"));
    out.push_str("---\n\n");
    out.push_str(&format!(
        "# Approved specification for {issue_identifier}\n\n"
    ));
    out.push_str("## Product behavior\n\n");
    out.push_str(artifact.product_behavior.trim());
    out.push_str("\n\n## Technical approach\n\n");
    out.push_str(artifact.technical_approach.trim());
    out.push_str("\n\n## Acceptance criteria\n\n");
    for (index, criterion) in artifact.acceptance_criteria.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", index + 1, criterion.trim()));
    }
    out.push_str("\n## Open decisions\n\n");
    if artifact.open_decisions.is_empty() {
        out.push_str("- None.\n");
    } else {
        for decision in &artifact.open_decisions {
            out.push_str(&format!("- {}\n", decision.trim()));
        }
    }
    out
}

pub fn is_spec_gap(blocker: &ImplementationBlocker) -> bool {
    blocker.kind == BlockerKind::SpecGap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementation::domain::{
        CriterionStatus, EvidenceKind, ImplementationEvidence, ManifestStatus,
    };
    use crate::spec::domain::SpecArtifact;

    fn approved() -> SpecArtifact {
        SpecArtifact {
            schema_version: 1,
            product_behavior: "Behavior".to_string(),
            technical_approach: "Approach".to_string(),
            acceptance_criteria: vec!["First".to_string(), "Second".to_string()],
            open_decisions: vec![],
        }
    }

    fn completed_manifest() -> ImplementationManifest {
        ImplementationManifest {
            schema_version: 1,
            status: ManifestStatus::Completed,
            head_commit: Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()),
            summary: "Implements the approved behavior.".to_string(),
            acceptance_criteria: vec![
                AcceptanceCriterionClaim {
                    index: 1,
                    status: CriterionStatus::Implemented,
                    evidence: vec![ImplementationEvidence {
                        kind: EvidenceKind::Repository,
                        reference: "src/lib.rs".to_string(),
                        summary: "Core change".to_string(),
                    }],
                },
                AcceptanceCriterionClaim {
                    index: 2,
                    status: CriterionStatus::Implemented,
                    evidence: vec![ImplementationEvidence {
                        kind: EvidenceKind::Test,
                        reference: "tests/a.rs".to_string(),
                        summary: "Coverage".to_string(),
                    }],
                },
            ],
            known_limitations: vec![],
            blocker: None,
        }
    }

    #[test]
    fn accepts_completed_manifest_with_full_coverage() {
        let bytes = serde_json::to_vec(&completed_manifest()).unwrap();
        let parsed = parse_and_validate_manifest(&bytes, &approved()).unwrap();
        assert_eq!(parsed.status, ManifestStatus::Completed);
    }

    #[test]
    fn rejects_unknown_fields_and_missing_coverage() {
        let bad = br#"{"schema_version":1,"status":"completed","head_commit":"4b825dc642cb6eb9a060e54bf8d69288fbee4904","summary":"x","acceptance_criteria":[],"known_limitations":[],"extra":true}"#;
        assert!(parse_manifest(bad).is_err());

        let mut incomplete = completed_manifest();
        incomplete.acceptance_criteria.pop();
        assert!(validate_manifest(&incomplete, Some(&approved())).is_err());
    }

    #[test]
    fn accepts_spec_gap_blocked_manifest() {
        let blocked = ImplementationManifest {
            schema_version: 1,
            status: ManifestStatus::Blocked,
            head_commit: None,
            summary: "Cannot proceed".to_string(),
            acceptance_criteria: vec![],
            known_limitations: vec![],
            blocker: Some(ImplementationBlocker {
                kind: BlockerKind::SpecGap,
                summary: "Ambiguous fork target".to_string(),
                evidence: "Criterion 3 conflicts with itself.".to_string(),
            }),
        };
        validate_manifest(&blocked, Some(&approved())).unwrap();
        assert!(is_spec_gap(blocked.blocker.as_ref().unwrap()));
    }

    #[test]
    fn path_template_rejects_traversal_and_git() {
        assert!(expand_spec_file_path(
            "specs/{issue_identifier}/APPROVED-v{version}.md",
            "KATA-1",
            "run",
            "art",
            2
        )
        .unwrap()
        .ends_with("APPROVED-v2.md"));
        assert!(validate_spec_file_path("../secrets.md").is_err());
        assert!(validate_spec_file_path(".git/config.md").is_err());
        assert!(validate_spec_file_path("/abs/path.md").is_err());
        assert!(validate_spec_file_path("specs/file.txt").is_err());
    }

    #[test]
    fn approved_spec_render_is_deterministic() {
        let a = render_approved_spec(
            "run",
            "KATA-1",
            "art",
            2,
            "2026-07-26T20:00:00Z",
            &approved(),
        );
        let b = render_approved_spec(
            "run",
            "KATA-1",
            "art",
            2,
            "2026-07-26T20:00:00Z",
            &approved(),
        );
        assert_eq!(a, b);
        assert!(a.contains("symphony_spec_version: 2"));
        assert!(a.contains("1. First"));
        assert!(a.contains("- None."));
    }
}
