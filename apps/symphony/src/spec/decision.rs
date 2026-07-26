use chrono::{DateTime, Utc};

use crate::spec::domain::SpecConfig;
use crate::triage::intake::IntakeComment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionAction {
    None,
    Conflict,
    Revise { feedback: Vec<IntakeComment> },
    ReviseWithoutFeedback,
    Approve,
    StaleApproval,
    ColdRevision,
}

pub struct DecisionInput<'a> {
    pub labels: &'a [String],
    pub comments: &'a [IntakeComment],
    pub publisher_login: &'a str,
    pub published_at: DateTime<Utc>,
    pub revision_is_current: bool,
    pub intake_revision_changed: bool,
    pub config: &'a SpecConfig,
}

/// A comment is Symphony's own only when it is authored by the publisher *and*
/// carries a Symphony marker. Symphony commonly authenticates with a maintainer's
/// own token (`gh auth token`), so matching on author alone would discard that
/// maintainer's feedback and make `spec-revise` undetectable for them.
fn is_publisher_owned(comment: &IntakeComment, publisher_login: &str) -> bool {
    comment.author_login.eq_ignore_ascii_case(publisher_login)
        && comment.body.contains("<!-- symphony:")
}

/// Evaluate exactly one branch of A2's ordered human-decision table.
pub fn detect_decision(input: DecisionInput<'_>) -> DecisionAction {
    let has = |wanted: &str| {
        input
            .labels
            .iter()
            .any(|label| label.eq_ignore_ascii_case(wanted))
    };
    let approved = has(&input.config.labels.approved);
    let revise = has(&input.config.labels.revise);

    if approved && revise {
        return DecisionAction::Conflict;
    }
    if revise {
        let feedback: Vec<IntakeComment> = input
            .comments
            .iter()
            .filter(|comment| {
                !is_publisher_owned(comment, input.publisher_login)
                    && (comment.created_at > input.published_at
                        || comment.updated_at > input.published_at)
            })
            .cloned()
            .collect();
        return if feedback.is_empty() {
            DecisionAction::ReviseWithoutFeedback
        } else {
            DecisionAction::Revise { feedback }
        };
    }
    if approved {
        return if input.revision_is_current {
            DecisionAction::Approve
        } else {
            DecisionAction::StaleApproval
        };
    }
    if input.intake_revision_changed && has(&input.config.intake_label) {
        return DecisionAction::ColdRevision;
    }
    DecisionAction::None
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn comment(created: DateTime<Utc>, updated: DateTime<Utc>) -> IntakeComment {
        IntakeComment {
            id: 1,
            author_login: "human".to_string(),
            body: "change this".to_string(),
            created_at: created,
            updated_at: updated,
        }
    }

    #[test]
    fn both_labels_conflict_before_other_branches() {
        let now = Utc::now();
        let config = SpecConfig::default();
        assert_eq!(
            detect_decision(DecisionInput {
                labels: &[config.labels.approved.clone(), config.labels.revise.clone()],
                comments: &[comment(now, now)],
                publisher_login: "bot",
                published_at: now,
                revision_is_current: true,
                intake_revision_changed: false,
                config: &config,
            }),
            DecisionAction::Conflict
        );
    }

    /// Symphony often runs with a maintainer's own GitHub token, so the publisher
    /// login equals the human's login. Feedback must be distinguished by the
    /// Symphony comment marker, not by author, or that maintainer can never request
    /// a revision on their own repository.
    #[test]
    fn maintainer_feedback_counts_even_when_publisher_shares_their_login() {
        let now = Utc::now();
        let config = SpecConfig::default();
        let mut own_spec_comment = comment(now - Duration::minutes(5), now - Duration::minutes(5));
        own_spec_comment.id = 1;
        own_spec_comment.author_login = "gannonh".to_string();
        own_spec_comment.body = "<!-- symphony:spec:abc -->\n## Symphony specification".to_string();

        let mut feedback = comment(now + Duration::minutes(1), now + Duration::minutes(1));
        feedback.id = 2;
        feedback.author_login = "gannonh".to_string();
        feedback.body = "Please cover the failure path.".to_string();

        let action = detect_decision(DecisionInput {
            labels: std::slice::from_ref(&config.labels.revise),
            comments: &[own_spec_comment, feedback],
            publisher_login: "gannonh",
            published_at: now,
            revision_is_current: true,
            intake_revision_changed: false,
            config: &config,
        });

        match action {
            DecisionAction::Revise { feedback } => {
                assert_eq!(feedback.len(), 1, "only the unmarked comment is feedback");
                assert_eq!(feedback[0].id, 2);
            }
            other => panic!("expected a revision request, got {other:?}"),
        }
    }

    #[test]
    fn edited_old_comment_counts_as_feedback() {
        let now = Utc::now();
        let config = SpecConfig::default();
        let action = detect_decision(DecisionInput {
            labels: std::slice::from_ref(&config.labels.revise),
            comments: &[comment(
                now - Duration::minutes(2),
                now + Duration::minutes(1),
            )],
            publisher_login: "bot",
            published_at: now,
            revision_is_current: false,
            intake_revision_changed: true,
            config: &config,
        });
        assert!(matches!(action, DecisionAction::Revise { .. }));
    }

    #[test]
    fn decision_table_covers_approve_stale_missing_feedback_and_cold_revision() {
        let now = Utc::now();
        let config = SpecConfig::default();

        assert_eq!(
            detect_decision(DecisionInput {
                labels: std::slice::from_ref(&config.labels.approved),
                comments: &[],
                publisher_login: "bot",
                published_at: now,
                revision_is_current: true,
                intake_revision_changed: false,
                config: &config,
            }),
            DecisionAction::Approve
        );
        assert_eq!(
            detect_decision(DecisionInput {
                labels: std::slice::from_ref(&config.labels.approved),
                comments: &[],
                publisher_login: "bot",
                published_at: now,
                revision_is_current: false,
                intake_revision_changed: true,
                config: &config,
            }),
            DecisionAction::StaleApproval
        );
        assert_eq!(
            detect_decision(DecisionInput {
                labels: std::slice::from_ref(&config.labels.revise),
                comments: &[],
                publisher_login: "bot",
                published_at: now,
                revision_is_current: true,
                intake_revision_changed: false,
                config: &config,
            }),
            DecisionAction::ReviseWithoutFeedback
        );
        assert_eq!(
            detect_decision(DecisionInput {
                labels: std::slice::from_ref(&config.intake_label),
                comments: &[],
                publisher_login: "bot",
                published_at: now,
                revision_is_current: false,
                intake_revision_changed: true,
                config: &config,
            }),
            DecisionAction::ColdRevision
        );
        assert_eq!(
            detect_decision(DecisionInput {
                labels: &[],
                comments: &[],
                publisher_login: "bot",
                published_at: now,
                revision_is_current: true,
                intake_revision_changed: false,
                config: &config,
            }),
            DecisionAction::None
        );
    }
}
