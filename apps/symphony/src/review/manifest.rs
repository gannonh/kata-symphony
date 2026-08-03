//! Typed, schema-validated output from the A4 review worker.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

pub const REVIEW_SCHEMA_VERSION: u32 = 1;
pub const REVIEW_MANIFEST_MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSeverity {
    #[default]
    Blocking,
    Major,
    Minor,
    Nit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewFindingCategory {
    Correctness,
    Security,
    SpecConformance,
    TestCoverage,
    Maintainability,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewFinding {
    pub finding_id: String,
    pub severity: ReviewSeverity,
    pub category: ReviewFindingCategory,
    pub path: String,
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    pub claim: String,
    pub rationale: String,
    pub remediation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_criterion: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewFindingsManifest {
    pub schema_version: u32,
    pub reviewed_head_sha: String,
    pub base_sha: String,
    pub spec_conformance_summary: String,
    #[serde(default)]
    pub no_findings: bool,
    #[serde(default)]
    pub findings: Vec<ReviewFinding>,
}

/// An inclusive line range that GitHub accepts on the right side of the PR diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewedLineRange {
    pub start: u32,
    pub end: u32,
}

impl ReviewedLineRange {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    fn contains(self, start: u32, end: u32) -> bool {
        self.start <= start && end <= self.end
    }
}

/// A changed file as it exists at the reviewed head SHA.
///
/// A zero line count and empty right-side ranges represent a deleted file. The
/// ranges include changed and context lines accepted by GitHub for review
/// comments, so validated findings remain publishable by PR2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewedFile {
    pub path: String,
    pub line_count: u32,
    pub right_side_ranges: Vec<ReviewedLineRange>,
}

impl ReviewedFile {
    pub fn new(
        path: impl Into<String>,
        line_count: u32,
        right_side_ranges: Vec<ReviewedLineRange>,
    ) -> Self {
        Self {
            path: path.into(),
            line_count,
            right_side_ranges,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestValidationError {
    pub violations: Vec<String>,
}

impl ManifestValidationError {
    fn one(message: impl Into<String>) -> Self {
        Self {
            violations: vec![message.into()],
        }
    }
}

impl fmt::Display for ManifestValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "review findings manifest rejected: {}",
            self.violations.join("; ")
        )
    }
}

impl std::error::Error for ManifestValidationError {}

/// Parse and validate a worker manifest before any artifact or preview is stored.
///
/// The expected SHAs and changed-file inventory come from Symphony, never from
/// worker output. This keeps a valid-looking manifest from reviewing a different
/// commit or anchoring findings outside the reviewed diff.
pub fn parse_and_validate_review_manifest(
    raw: &str,
    expected_head_sha: &str,
    expected_base_sha: &str,
    changed_files: &[ReviewedFile],
    max_findings: usize,
) -> Result<ReviewFindingsManifest, ManifestValidationError> {
    if raw.len() > REVIEW_MANIFEST_MAX_BYTES {
        return Err(ManifestValidationError::one(format!(
            "manifest is {} bytes; maximum is {REVIEW_MANIFEST_MAX_BYTES}",
            raw.len()
        )));
    }

    let manifest: ReviewFindingsManifest = serde_json::from_str(raw).map_err(|error| {
        ManifestValidationError::one(format!("manifest is not valid strict JSON: {error}"))
    })?;

    let mut violations = Vec::new();
    if manifest.schema_version != REVIEW_SCHEMA_VERSION {
        violations.push(format!(
            "schema_version must be {REVIEW_SCHEMA_VERSION}, got {}",
            manifest.schema_version
        ));
    }
    if manifest.reviewed_head_sha != expected_head_sha {
        violations.push(format!(
            "reviewed_head_sha does not match the pinned head {expected_head_sha}"
        ));
    }
    if manifest.base_sha != expected_base_sha {
        violations.push(format!(
            "base_sha does not match the pinned base {expected_base_sha}"
        ));
    }
    if manifest.spec_conformance_summary.trim().is_empty() {
        violations.push("spec_conformance_summary must not be empty".to_string());
    }
    if manifest.findings.is_empty() && !manifest.no_findings {
        violations.push(
            "an empty findings list requires an explicit no_findings affirmation".to_string(),
        );
    }
    if !manifest.findings.is_empty() && manifest.no_findings {
        violations.push("no_findings cannot be true when findings are present".to_string());
    }
    if manifest.findings.len() > max_findings {
        violations.push(format!(
            "manifest has {} findings; configured maximum is {max_findings}",
            manifest.findings.len()
        ));
    }

    let reviewed_files: BTreeMap<&str, &ReviewedFile> = changed_files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut finding_ids = BTreeSet::new();

    for (index, finding) in manifest.findings.iter().enumerate() {
        let label = if finding.finding_id.trim().is_empty() {
            format!("finding[{index}]")
        } else {
            format!("finding {}", finding.finding_id)
        };
        if finding.finding_id.trim().is_empty() {
            violations.push(format!("{label}: finding_id must not be empty"));
        } else if !finding_ids.insert(finding.finding_id.as_str()) {
            violations.push(format!("{label}: finding_id is duplicated"));
        }

        let Some(file) = reviewed_files.get(finding.path.as_str()).copied() else {
            violations.push(format!(
                "{label}: path {} is absent from the reviewed diff",
                finding.path
            ));
            continue;
        };

        let line_resolves = finding.line > 0 && finding.line <= file.line_count;
        if !line_resolves {
            violations.push(format!(
                "{label}: line {} does not resolve in {} at the reviewed head (1..={})",
                finding.line, finding.path, file.line_count
            ));
        }

        let end_line = finding.end_line.unwrap_or(finding.line);
        let end_resolves = end_line >= finding.line && end_line <= file.line_count;
        if finding.end_line.is_some() && !end_resolves {
            violations.push(format!(
                "{label}: end_line {end_line} must be between line {} and {}",
                finding.line, file.line_count
            ));
        }

        if line_resolves
            && end_resolves
            && !file
                .right_side_ranges
                .iter()
                .any(|range| range.contains(finding.line, end_line))
        {
            violations.push(format!(
                "{label}: anchor {}..={end_line} is absent from the right side of the reviewed diff for {}",
                finding.line, finding.path
            ));
        }

        for (field, value) in [
            ("claim", finding.claim.as_str()),
            ("rationale", finding.rationale.as_str()),
            ("remediation", finding.remediation.as_str()),
        ] {
            if value.trim().is_empty() {
                violations.push(format!("{label}: {field} must not be empty"));
            }
        }
        if finding
            .acceptance_criterion
            .as_deref()
            .is_some_and(|criterion| criterion.trim().is_empty())
        {
            violations.push(format!(
                "{label}: acceptance_criterion must not be empty when present"
            ));
        }
        if !finding.confidence.is_finite() || !(0.0..=1.0).contains(&finding.confidence) {
            violations.push(format!(
                "{label}: confidence must be a finite number from 0 through 1"
            ));
        }
    }

    if violations.is_empty() {
        Ok(manifest)
    } else {
        Err(ManifestValidationError { violations })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn files() -> Vec<ReviewedFile> {
        vec![
            ReviewedFile::new(
                "src/lib.rs",
                40,
                vec![
                    ReviewedLineRange::new(10, 20),
                    ReviewedLineRange::new(30, 35),
                ],
            ),
            ReviewedFile::new("src/deleted.rs", 0, vec![]),
        ]
    }

    fn valid_manifest() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "reviewed_head_sha": HEAD,
            "base_sha": BASE,
            "spec_conformance_summary": "The change satisfies the reviewed criterion.",
            "no_findings": false,
            "findings": [{
                "finding_id": "finding-1",
                "severity": "blocking",
                "category": "correctness",
                "path": "src/lib.rs",
                "line": 12,
                "end_line": 14,
                "claim": "The retry guard is inverted.",
                "rationale": "The changed branch retries only after success.",
                "remediation": "Invert the condition.",
                "acceptance_criterion": "AC-4",
                "confidence": 0.98
            }]
        })
    }

    fn validate(
        value: &serde_json::Value,
    ) -> Result<ReviewFindingsManifest, ManifestValidationError> {
        parse_and_validate_review_manifest(&value.to_string(), HEAD, BASE, &files(), 50)
    }

    #[test]
    fn accepts_a_pinned_manifest_with_resolvable_anchors() {
        let manifest = validate(&valid_manifest()).expect("valid manifest");
        assert_eq!(manifest.findings.len(), 1);
        assert_eq!(manifest.findings[0].severity, ReviewSeverity::Blocking);
    }

    #[test]
    fn rejects_unknown_fields() {
        let mut value = valid_manifest();
        value["unexpected"] = serde_json::json!(true);

        let error = validate(&value).expect_err("unknown fields must fail closed");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn rejects_a_finding_outside_the_reviewed_diff() {
        let mut value = valid_manifest();
        value["findings"][0]["path"] = serde_json::json!("src/other.rs");

        let error = validate(&value).expect_err("path must be in diff");
        assert!(error.to_string().contains("absent from the reviewed diff"));
    }

    #[test]
    fn rejects_an_anchor_that_does_not_resolve_at_the_reviewed_head() {
        let mut value = valid_manifest();
        value["findings"][0]["line"] = serde_json::json!(41);

        let error = validate(&value).expect_err("line must resolve");
        assert!(error.to_string().contains("does not resolve"));
    }

    #[test]
    fn rejects_an_anchor_outside_the_right_side_diff_ranges() {
        let mut value = valid_manifest();
        value["findings"][0]["line"] = serde_json::json!(25);
        value["findings"][0]["end_line"] = serde_json::json!(25);

        let error = validate(&value).expect_err("anchor must be publishable in the diff");
        assert!(error
            .to_string()
            .contains("right side of the reviewed diff"));
    }

    #[test]
    fn rejects_a_multiline_anchor_that_crosses_a_diff_range_boundary() {
        let mut value = valid_manifest();
        value["findings"][0]["line"] = serde_json::json!(18);
        value["findings"][0]["end_line"] = serde_json::json!(22);

        let error = validate(&value).expect_err("anchor must fit one diff range");
        assert!(error
            .to_string()
            .contains("right side of the reviewed diff"));
    }

    #[test]
    fn rejects_a_deleted_file_anchor() {
        let mut value = valid_manifest();
        value["findings"][0]["path"] = serde_json::json!("src/deleted.rs");
        value["findings"][0]["line"] = serde_json::json!(1);

        let error = validate(&value).expect_err("deleted files have no right-side anchor");
        assert!(error.to_string().contains("1..=0"));
    }

    #[test]
    fn requires_an_explicit_clean_review_affirmation() {
        let mut value = valid_manifest();
        value["findings"] = serde_json::json!([]);
        value["no_findings"] = serde_json::json!(false);

        let error = validate(&value).expect_err("empty output is not a clean review");
        assert!(error.to_string().contains("no_findings affirmation"));

        value["no_findings"] = serde_json::json!(true);
        validate(&value).expect("explicit clean review");
    }

    #[test]
    fn rejects_no_findings_when_findings_are_present() {
        let mut value = valid_manifest();
        value["no_findings"] = serde_json::json!(true);

        let error = validate(&value).expect_err("affirmation conflicts with findings");
        assert!(error.to_string().contains("when findings are present"));
    }

    #[test]
    fn accepts_kebab_case_category_vocabulary() {
        let mut value = valid_manifest();
        value["findings"][0]["category"] = serde_json::json!("spec-conformance");
        let manifest = validate(&value).expect("spec-conformance category");
        assert_eq!(
            manifest.findings[0].category,
            ReviewFindingCategory::SpecConformance
        );

        value["findings"][0]["category"] = serde_json::json!("test-coverage");
        let manifest = validate(&value).expect("test-coverage category");
        assert_eq!(
            manifest.findings[0].category,
            ReviewFindingCategory::TestCoverage
        );
    }

    #[test]
    fn rejects_unknown_vocabulary_values() {
        let mut value = valid_manifest();
        value["findings"][0]["severity"] = serde_json::json!("critical");

        let error = validate(&value).expect_err("severity vocabulary is closed");
        assert!(error.to_string().contains("unknown variant"), "{error}");
    }

    #[test]
    fn rejects_a_manifest_for_a_different_head() {
        let mut value = valid_manifest();
        value["reviewed_head_sha"] = serde_json::json!("cccccccccccccccccccccccccccccccccccccccc");

        let error = validate(&value).expect_err("head is pinned");
        assert!(error.to_string().contains("pinned head"));
    }

    #[test]
    fn rejects_duplicate_finding_ids_and_enforces_the_configured_limit() {
        let mut value = valid_manifest();
        let duplicate = value["findings"][0].clone();
        value["findings"]
            .as_array_mut()
            .expect("findings array")
            .push(duplicate);

        let error = parse_and_validate_review_manifest(&value.to_string(), HEAD, BASE, &files(), 1)
            .expect_err("duplicate and limit violations");
        assert!(error.to_string().contains("configured maximum"));
        assert!(error.to_string().contains("duplicated"));
    }
}
