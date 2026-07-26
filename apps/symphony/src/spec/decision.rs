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
                !comment
                    .author_login
                    .eq_ignore_ascii_case(input.publisher_login)
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
}
