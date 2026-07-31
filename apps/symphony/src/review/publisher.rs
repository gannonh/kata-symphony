//! Idempotent marker-owned findings preview publication.

use crate::error::{Result, SymphonyError};
use crate::review::domain::{ReviewFindingsArtifactRecord, ReviewPublicationIntent, REVIEW_COMMENT_MARKER_PREFIX, REVIEW_COMMENT_MARKER_SUFFIX};
use crate::review::findings::render_preview_comment;
use crate::triage::publisher::TriageCommentPort;
use crate::triage::runtime::SharedFactoryStore;

pub const REVIEW_PREVIEW_COMMENT_STEP: &str = "review_preview_comment";

#[derive(Clone)]
pub struct ReviewPublisher<C> {
    comments: C,
}

impl<C> ReviewPublisher<C>
where
    C: TriageCommentPort + Clone,
{
    pub fn new(comments: C) -> Self {
        Self { comments }
    }

    pub async fn publish_preview(
        &self,
        store: &SharedFactoryStore,
        intent: &ReviewPublicationIntent,
        artifact: &ReviewFindingsArtifactRecord,
        issue_number: u64,
        max_pages: u32,
    ) -> Result<()> {
        if intent.kind != "preview" {
            return Err(SymphonyError::TriageError(format!(
                "review preview publisher cannot reconcile kind={}",
                intent.kind
            )));
        }
        if intent.status == crate::triage::domain::PublicationStatus::Applied
            && intent
                .completed_steps
                .iter()
                .any(|step| step == REVIEW_PREVIEW_COMMENT_STEP)
        {
            return Ok(());
        }
        let body = render_preview_comment(&intent.intent_id, &intent.run_id, artifact);
        self.upsert_owned_comment(store, intent, issue_number, &body, max_pages)
            .await?;
        store.complete_review_publication(&intent.intent_id, REVIEW_PREVIEW_COMMENT_STEP)
    }

    async fn upsert_owned_comment(
        &self,
        store: &SharedFactoryStore,
        intent: &ReviewPublicationIntent,
        issue_number: u64,
        body: &str,
        max_pages: u32,
    ) -> Result<()> {
        let publisher_login = self.comments.authenticated_login().await?;
        let mut comment_id = if let Some(raw) = intent.comment_id.as_deref() {
            raw.parse::<u64>().map_err(|error| {
                SymphonyError::TriageError(format!(
                    "invalid review publication comment id {raw}: {error}"
                ))
            })?
        } else if let Some(found) = self
            .find_owned_marker(issue_number, &intent.intent_id, &publisher_login, max_pages)
            .await?
        {
            found
        } else {
            let created = self.comments.create_comment(issue_number, body).await?;
            store.bind_review_publication_comment(
                &intent.intent_id,
                &created.id.to_string(),
                &publisher_login,
            )?;
            return Ok(());
        };

        let existing = match self.comments.get_comment(comment_id).await {
            Ok(existing) => existing,
            Err(SymphonyError::GithubApiStatus { status: 404, .. }) => {
                store.clear_review_publication_comment(&intent.intent_id)?;
                if let Some(recovered) = self
                    .find_owned_marker(issue_number, &intent.intent_id, &publisher_login, max_pages)
                    .await?
                {
                    comment_id = recovered;
                    self.comments.get_comment(comment_id).await?
                } else {
                    let created = self.comments.create_comment(issue_number, body).await?;
                    store.bind_review_publication_comment(
                        &intent.intent_id,
                        &created.id.to_string(),
                        &publisher_login,
                    )?;
                    return Ok(());
                }
            }
            Err(error) => return Err(error),
        };
        let author = existing
            .user
            .as_ref()
            .map(|user| user.login.as_str())
            .unwrap_or_default();
        if !author.eq_ignore_ascii_case(&publisher_login) {
            return Err(SymphonyError::TriageError(format!(
                "review publication comment {comment_id} is not owned by publisher {publisher_login}"
            )));
        }
        self.comments.update_comment(comment_id, body).await?;
        if intent.comment_id.is_none()
            || intent.publisher_login.as_deref() != Some(publisher_login.as_str())
        {
            store.bind_review_publication_comment(
                &intent.intent_id,
                &comment_id.to_string(),
                &publisher_login,
            )?;
        }
        Ok(())
    }

    async fn find_owned_marker(
        &self,
        issue_number: u64,
        intent_id: &str,
        publisher_login: &str,
        max_pages: u32,
    ) -> Result<Option<u64>> {
        let marker = format!("{REVIEW_COMMENT_MARKER_PREFIX}{intent_id}{REVIEW_COMMENT_MARKER_SUFFIX}");
        for comment in self.comments.list_comments(issue_number, max_pages).await? {
            let Some(body) = comment.body.as_deref() else {
                continue;
            };
            if !body.contains(&marker) {
                continue;
            }
            let author = comment
                .user
                .as_ref()
                .map(|user| user.login.as_str())
                .unwrap_or_default();
            if author.eq_ignore_ascii_case(publisher_login) {
                return Ok(Some(comment.id));
            }
        }
        Ok(None)
    }
}
