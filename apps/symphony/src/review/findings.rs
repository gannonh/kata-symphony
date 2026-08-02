//! Diff inventory and findings summary helpers.

use std::collections::BTreeMap;

use crate::github::client::GithubPullRequestFile;
use crate::review::domain::{
    ReviewFindingRecord, ReviewFindingsArtifactRecord, REVIEW_COMMENT_MARKER_PREFIX,
    REVIEW_COMMENT_MARKER_SUFFIX,
};
use crate::review::manifest::{ReviewFinding, ReviewSeverity, ReviewedFile, ReviewedLineRange};

/// Stable finding identity used for lifecycle carry-forward across review cycles.
/// Location lines are deliberately excluded so edits that move a finding retain identity.
pub fn finding_identity_key(finding: &ReviewFinding) -> String {
    let category = match finding.category {
        crate::review::manifest::ReviewFindingCategory::Correctness => "correctness",
        crate::review::manifest::ReviewFindingCategory::Security => "security",
        crate::review::manifest::ReviewFindingCategory::SpecConformance => "spec-conformance",
        crate::review::manifest::ReviewFindingCategory::TestCoverage => "test-coverage",
        crate::review::manifest::ReviewFindingCategory::Maintainability => "maintainability",
    };
    format!(
        "{}:{}:{}",
        finding.path.trim(),
        category,
        finding.claim.trim()
    )
}

/// Convert GitHub file patches into the right-side line ranges accepted by
/// review comments. A hunk's full new-file span includes both changed and
/// context lines, which is exactly the range GitHub permits for an anchor.
pub fn reviewed_files(files: &[GithubPullRequestFile]) -> Vec<ReviewedFile> {
    files
        .iter()
        .map(|file| {
            let deleted = file.status.eq_ignore_ascii_case("removed");
            let mut ranges = Vec::new();
            if !deleted {
                if let Some(patch) = file.patch.as_deref() {
                    for line in patch.lines() {
                        if let Some((start, count)) = parse_new_hunk_header(line) {
                            let end = start.saturating_add(count.saturating_sub(1));
                            if count > 0 {
                                ranges.push(ReviewedLineRange::new(start, end));
                            }
                        }
                    }
                }
                // A missing patch does not tell us where the changes landed.
                // Keep the file unanchorable instead of guessing at line 1.
            }
            let line_count = if deleted {
                0
            } else {
                ranges.iter().map(|range| range.end).max().unwrap_or(0)
            };
            ReviewedFile::new(file.filename.clone(), line_count, ranges)
        })
        .collect()
}

fn parse_new_hunk_header(line: &str) -> Option<(u32, u32)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("@@ ")?;
    let plus = rest.split_whitespace().find(|part| part.starts_with('+'))?;
    let plus = plus.strip_prefix('+')?;
    let mut parts = plus.split(',');
    let start = parts.next()?.parse::<u32>().ok()?;
    let count = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(1);
    Some((start, count))
}

pub fn render_preview_comment(
    intent_id: &str,
    run_id: &str,
    artifact: &ReviewFindingsArtifactRecord,
) -> String {
    let mut counts = BTreeMap::<&str, usize>::new();
    for finding in &artifact.manifest.findings {
        let label = match finding.severity {
            ReviewSeverity::Blocking => "blocking",
            ReviewSeverity::Major => "major",
            ReviewSeverity::Minor => "minor",
            ReviewSeverity::Nit => "nit",
        };
        *counts.entry(label).or_default() += 1;
    }
    let marker = format!("{REVIEW_COMMENT_MARKER_PREFIX}{intent_id}{REVIEW_COMMENT_MARKER_SUFFIX}");
    let mut body = format!(
        "{marker}\n## Symphony A4 review preview\n\nFactory run `{run_id}` reviewed head `{}` against base `{}`.\n\n{}\n",
        artifact.reviewed_head_sha,
        artifact.base_sha,
        artifact.manifest.spec_conformance_summary.trim()
    );
    if artifact.manifest.no_findings {
        body.push_str("\n**No findings.** The worker explicitly affirmed `no_findings`.\n");
    } else {
        body.push_str(&format!("\nFindings: {}\n", artifact.finding_count));
        if !counts.is_empty() {
            body.push_str("\n| Severity | Count |\n| --- | ---: |\n");
            for (severity, count) in counts {
                body.push_str(&format!("| {severity} | {count} |\n"));
            }
        }
        body.push_str(
            "\n| ID | Severity | Category | Location | Claim |\n| --- | --- | --- | --- | --- |\n",
        );
        for finding in &artifact.manifest.findings {
            let end = finding
                .end_line
                .map(|line| format!("-{line}"))
                .unwrap_or_default();
            body.push_str(&format!(
                "| `{}` | `{}` | `{}` | `{}`:{}{} | {} |\n",
                finding.finding_id,
                serde_json::to_string(&finding.severity)
                    .unwrap_or_else(|_| "\"unknown\"".into())
                    .trim_matches('"'),
                serde_json::to_string(&finding.category)
                    .unwrap_or_else(|_| "\"unknown\"".into())
                    .trim_matches('"'),
                finding.path,
                finding.line,
                end,
                finding.claim.replace(['\r', '\n'], " ").replace('|', "\\|")
            ));
        }
    }
    body.push_str("\n_This is a preview only. A4 PR1 creates no GitHub review, inline comment, tracker transition, approval, or merge._\n");
    body
}

/// Render the marker-owned body used by an atomic GitHub pull-request review.
/// Inline finding details are carried by the review comments themselves.
pub fn render_formal_review_body(
    issue_number: u64,
    intent_id: &str,
    run_id: &str,
    artifact: &ReviewFindingsArtifactRecord,
    approved_spec_version: Option<u32>,
) -> String {
    render_formal_review_body_with_records(
        issue_number,
        intent_id,
        run_id,
        artifact,
        approved_spec_version,
        &[],
    )
}

pub fn render_formal_review_body_with_records(
    issue_number: u64,
    intent_id: &str,
    run_id: &str,
    artifact: &ReviewFindingsArtifactRecord,
    approved_spec_version: Option<u32>,
    finding_records: &[ReviewFindingRecord],
) -> String {
    let marker = format!("{REVIEW_COMMENT_MARKER_PREFIX}{intent_id}{REVIEW_COMMENT_MARKER_SUFFIX}");
    let persisting = finding_records
        .iter()
        .filter(|record| record.lifecycle_state == "persisting")
        .collect::<Vec<_>>();
    let findings = if artifact.manifest.no_findings {
        "**No findings.** The worker explicitly affirmed `no_findings`.".to_string()
    } else if persisting.is_empty() {
        format!(
            "Findings: {}. Every finding is attached as an inline comment.",
            artifact.finding_count
        )
    } else {
        format!(
            "Findings: {}. New findings are attached as inline comments. {} persisting finding(s) remain summarized below.",
            artifact.finding_count,
            persisting.len()
        )
    };
    let persisting_summary = if persisting.is_empty() {
        String::new()
    } else {
        let mut summary = String::from("\n\n### Persisting findings\n\n");
        summary.push_str("| ID | Location | Claim |\n| --- | --- | --- |\n");
        for record in persisting {
            let end = record
                .end_line
                .map(|line| format!("-{line}"))
                .unwrap_or_default();
            summary.push_str(&format!(
                "| `{}` | `{}`:{}{} | {} |\n",
                record.finding_id,
                record.path,
                record.line,
                end,
                record.claim.replace(['\r', '\n'], " ").replace('|', "\\|")
            ));
        }
        summary
    };
    format!(
        "{marker}\n## Symphony A4 formal review\n\nIssue `#{issue_number}`.\n\nSpec artifact `{}` (version `{}`).\n\nFactory run `{run_id}` reviewed head `{}` against base `{}`.\n\n{}\n\n{}{}\n",
        artifact.spec_artifact_id,
        approved_spec_version.unwrap_or(artifact.schema_version),
        artifact.reviewed_head_sha,
        artifact.base_sha,
        artifact.manifest.spec_conformance_summary.trim(),
        findings,
        persisting_summary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hunk_ranges() {
        assert_eq!(parse_new_hunk_header("@@ -2,3 +10,4 @@"), Some((10, 4)));
        assert_eq!(parse_new_hunk_header("@@ -1 +1 @@"), Some((1, 1)));
    }

    #[test]
    fn preserves_deleted_files_as_unanchorable() {
        let files = vec![GithubPullRequestFile {
            filename: "gone.rs".into(),
            status: "removed".into(),
            ..Default::default()
        }];
        let result = reviewed_files(&files);
        assert_eq!(result[0].line_count, 0);
        assert!(result[0].right_side_ranges.is_empty());
    }

    #[test]
    fn treats_missing_patches_as_unanchorable() {
        let files = vec![GithubPullRequestFile {
            filename: "large.rs".into(),
            status: "modified".into(),
            additions: 20,
            patch: None,
            ..Default::default()
        }];
        let result = reviewed_files(&files);
        assert_eq!(result[0].line_count, 0);
        assert!(result[0].right_side_ranges.is_empty());
    }
}
