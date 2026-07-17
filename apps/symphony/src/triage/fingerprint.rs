use crate::triage::domain::{
    TriageRoutesConfig, TRIAGE_COMMENT_MARKER_PREFIX, TRIAGE_COMMENT_MARKER_SUFFIX,
    TRIAGE_SCHEMA_VERSION,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueMilestoneFingerprint {
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueCommentFingerprint {
    pub id: u64,
    pub author_login: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedTriageComment {
    pub comment_id: u64,
    pub intent_id: String,
    pub publisher_login: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRevisionInput {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub managed_labels: Vec<String>,
    pub intake_label: String,
    pub assignees: Vec<String>,
    pub milestone: Option<IssueMilestoneFingerprint>,
    pub comments: Vec<IssueCommentFingerprint>,
    pub verified_triage_comments: Vec<VerifiedTriageComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationRevisionInput {
    pub prompt: String,
    pub turn_timeout_ms: u64,
    pub max_attempts: u32,
    pub harness: String,
    pub model: Option<String>,
    pub routes: TriageRoutesConfig,
}

#[derive(Serialize)]
struct CanonicalIssueRevision<'a> {
    schema_version: u32,
    title: String,
    body: String,
    labels: Vec<String>,
    assignees: Vec<String>,
    milestone: Option<CanonicalMilestone>,
    comments: Vec<CanonicalComment<'a>>,
}

#[derive(Serialize)]
struct CanonicalMilestone {
    number: u64,
    title: String,
}

#[derive(Serialize)]
struct CanonicalComment<'a> {
    id: u64,
    author_login: String,
    body: String,
    created_at: &'a str,
    updated_at: &'a str,
}

#[derive(Serialize)]
struct CanonicalConfigurationRevision {
    schema_version: u32,
    prompt: String,
    turn_timeout_ms: u64,
    max_attempts: u32,
    harness: String,
    model: Option<String>,
    route_mapping_hash: String,
}

#[derive(Serialize)]
struct CanonicalRouteMapping<'a> {
    schema_version: u32,
    implement: CanonicalRouteMappingEntry<'a>,
    spec: CanonicalRouteMappingEntry<'a>,
    needs_information: CanonicalRouteMappingEntry<'a>,
    park: CanonicalRouteMappingEntry<'a>,
    human_owned: CanonicalRouteMappingEntry<'a>,
}

#[derive(Serialize)]
struct CanonicalRouteMappingEntry<'a> {
    label: String,
    state: Option<&'a str>,
}

pub fn issue_revision(input: &IssueRevisionInput) -> String {
    let excluded_labels = input
        .managed_labels
        .iter()
        .chain(std::iter::once(&input.intake_label))
        .map(|label| label.trim().to_ascii_lowercase())
        .filter(|label| !label.is_empty())
        .collect::<BTreeSet<_>>();

    let labels = input
        .labels
        .iter()
        .map(|label| label.trim().to_ascii_lowercase())
        .filter(|label| !label.is_empty() && !excluded_labels.contains(label))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let assignees = input
        .assignees
        .iter()
        .map(|login| login.trim().to_ascii_lowercase())
        .filter(|login| !login.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let mut comments = input
        .comments
        .iter()
        .filter(|comment| !is_verified_triage_comment(comment, &input.verified_triage_comments))
        .map(|comment| CanonicalComment {
            id: comment.id,
            author_login: comment.author_login.trim().to_ascii_lowercase(),
            body: normalize_newlines(&comment.body),
            created_at: &comment.created_at,
            updated_at: &comment.updated_at,
        })
        .collect::<Vec<_>>();
    comments.sort_by_key(|comment| comment.id);

    let canonical = CanonicalIssueRevision {
        schema_version: TRIAGE_SCHEMA_VERSION,
        title: normalize_newlines(&input.title),
        body: normalize_newlines(&input.body),
        labels,
        assignees,
        milestone: input
            .milestone
            .as_ref()
            .map(|milestone| CanonicalMilestone {
                number: milestone.number,
                title: normalize_newlines(&milestone.title),
            }),
        comments,
    };

    sha256_json(&canonical)
}

pub fn configuration_revision(input: &ConfigurationRevisionInput) -> String {
    let canonical = CanonicalConfigurationRevision {
        schema_version: TRIAGE_SCHEMA_VERSION,
        prompt: normalize_newlines(&input.prompt),
        turn_timeout_ms: input.turn_timeout_ms,
        max_attempts: input.max_attempts,
        harness: input.harness.trim().to_ascii_lowercase(),
        model: input
            .model
            .as_ref()
            .map(|model| normalize_newlines(model.trim())),
        route_mapping_hash: route_mapping_hash(&input.routes),
    };

    sha256_json(&canonical)
}

pub fn route_mapping_hash(routes: &TriageRoutesConfig) -> String {
    let canonical = CanonicalRouteMapping {
        schema_version: TRIAGE_SCHEMA_VERSION,
        implement: route_entry(&routes.implement.label, routes.implement.state.as_deref()),
        spec: route_entry(&routes.spec.label, routes.spec.state.as_deref()),
        needs_information: route_entry(
            &routes.needs_information.label,
            routes.needs_information.state.as_deref(),
        ),
        park: route_entry(&routes.park.label, routes.park.state.as_deref()),
        human_owned: route_entry(
            &routes.human_owned.label,
            routes.human_owned.state.as_deref(),
        ),
    };

    sha256_json(&canonical)
}

pub fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn route_entry<'a>(label: &str, state: Option<&'a str>) -> CanonicalRouteMappingEntry<'a> {
    CanonicalRouteMappingEntry {
        label: normalize_newlines(label.trim()),
        state: state.map(str::trim).filter(|state| !state.is_empty()),
    }
}

fn is_verified_triage_comment(
    comment: &IssueCommentFingerprint,
    verified_comments: &[VerifiedTriageComment],
) -> bool {
    verified_comments.iter().any(|verified| {
        comment.id == verified.comment_id
            && comment
                .author_login
                .trim()
                .eq_ignore_ascii_case(&verified.publisher_login)
            && comment.body.contains(&format!(
                "{TRIAGE_COMMENT_MARKER_PREFIX}{}{TRIAGE_COMMENT_MARKER_SUFFIX}",
                verified.intent_id
            ))
    })
}

fn sha256_json<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_vec(value).expect("canonical fingerprint JSON serializes");
    hex::encode(Sha256::digest(json))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue_input() -> IssueRevisionInput {
        IssueRevisionInput {
            title: "Fix docs\r\nnow".to_string(),
            body: "Body\r\ntext".to_string(),
            labels: vec![
                "bug".to_string(),
                "Needs-Triage".to_string(),
                "ready-for-agent".to_string(),
            ],
            managed_labels: vec!["ready-for-agent".to_string()],
            intake_label: "needs-triage".to_string(),
            assignees: vec!["ALICE".to_string()],
            milestone: Some(IssueMilestoneFingerprint {
                number: 3,
                title: "M1".to_string(),
            }),
            comments: vec![
                IssueCommentFingerprint {
                    id: 2,
                    author_login: "bot".to_string(),
                    body: "<!-- symphony:triage:intent-1 -->\nPreview".to_string(),
                    created_at: "2026-07-16T00:00:00Z".to_string(),
                    updated_at: "2026-07-16T00:00:00Z".to_string(),
                },
                IssueCommentFingerprint {
                    id: 1,
                    author_login: "reporter".to_string(),
                    body: "Please fix".to_string(),
                    created_at: "2026-07-15T00:00:00Z".to_string(),
                    updated_at: "2026-07-15T00:00:00Z".to_string(),
                },
            ],
            verified_triage_comments: vec![VerifiedTriageComment {
                comment_id: 2,
                intent_id: "intent-1".to_string(),
                publisher_login: "bot".to_string(),
            }],
        }
    }

    #[test]
    fn issue_revision_excludes_intake_managed_and_verified_comment() {
        let mut changed = issue_input();
        let original = issue_revision(&changed);

        changed.labels = vec![
            "READY-FOR-AGENT".to_string(),
            "bug".to_string(),
            "needs-triage".to_string(),
        ];
        changed.comments[0].body = "<!-- symphony:triage:intent-1 -->\r\nUpdated".to_string();

        assert_eq!(issue_revision(&changed), original);
    }

    #[test]
    fn issue_revision_includes_spoofed_or_unverified_marker_comments() {
        let original = issue_revision(&issue_input());
        let mut changed = issue_input();
        changed.verified_triage_comments.clear();

        assert_ne!(issue_revision(&changed), original);
    }

    #[test]
    fn issue_revision_includes_content_changes() {
        let original = issue_revision(&issue_input());
        let mut changed = issue_input();
        changed.comments[1].body = "Different".to_string();

        assert_ne!(issue_revision(&changed), original);
    }

    #[test]
    fn configuration_revision_normalizes_prompt_newlines_and_hashes_mapping() {
        let routes = TriageRoutesConfig::default();
        let one = configuration_revision(&ConfigurationRevisionInput {
            prompt: "Prompt\r\nbody".to_string(),
            turn_timeout_ms: 900_000,
            max_attempts: 3,
            harness: "PI".to_string(),
            model: Some(" model-a ".to_string()),
            routes: routes.clone(),
        });
        let two = configuration_revision(&ConfigurationRevisionInput {
            prompt: "Prompt\nbody".to_string(),
            turn_timeout_ms: 900_000,
            max_attempts: 3,
            harness: "pi".to_string(),
            model: Some("model-a".to_string()),
            routes,
        });

        assert_eq!(one, two);
    }

    #[test]
    fn route_mapping_hash_changes_when_route_changes() {
        let mut routes = TriageRoutesConfig::default();
        let original = route_mapping_hash(&routes);
        routes.implement.state = Some("In Progress".to_string());

        assert_ne!(route_mapping_hash(&routes), original);
    }
}
