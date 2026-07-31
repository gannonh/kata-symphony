//! Idempotent marker-owned findings preview publication.

use crate::error::{Result, SymphonyError};
use crate::review::domain::{
    ReviewFindingsArtifactRecord, ReviewPublicationIntent, REVIEW_COMMENT_MARKER_PREFIX,
    REVIEW_COMMENT_MARKER_SUFFIX,
};
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
                    store.bind_review_publication_comment(
                        &intent.intent_id,
                        &comment_id.to_string(),
                        &publisher_login,
                    )?;
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
        let marker =
            format!("{REVIEW_COMMENT_MARKER_PREFIX}{intent_id}{REVIEW_COMMENT_MARKER_SUFFIX}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::{GithubIssueComment, GithubUser};
    use crate::implementation::domain::{
        AcceptanceCriterionClaim, CriterionStatus, EvidenceKind, ExecutionProfile,
        ImplementationEvidence, ImplementationManifest, ImplementationPublicationKind,
        ManifestStatus,
    };
    use crate::review::domain::ReviewFindingsArtifactRecord;
    use crate::review::manifest::ReviewFindingsManifest;
    use crate::spec::domain::{SpecArtifact, SpecPublicationKind};
    use crate::triage::publisher::TriageCommentPort;
    use crate::triage::runtime::SharedFactoryStore;
    use crate::triage::store::{
        ClaimAttemptRequest, StoreDraftPrArtifactRequest, StoreImplementationArtifactRequest,
        StoreReviewArtifactRequest, StoreReviewAttemptRequest, StoreSpecArtifactRequest,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[derive(Default)]
    struct FakeComments {
        login: String,
        comments: Mutex<HashMap<u64, GithubIssueComment>>,
        next_id: Mutex<u64>,
        create_count: Mutex<u32>,
        update_count: Mutex<u32>,
    }

    impl FakeComments {
        fn new(login: &str) -> Arc<Self> {
            Arc::new(Self {
                login: login.to_string(),
                comments: Mutex::new(HashMap::new()),
                next_id: Mutex::new(700),
                create_count: Mutex::new(0),
                update_count: Mutex::new(0),
            })
        }
    }

    #[async_trait]
    impl TriageCommentPort for Arc<FakeComments> {
        async fn authenticated_login(&self) -> Result<String> {
            Ok(self.login.clone())
        }

        async fn list_comments(
            &self,
            _issue_number: u64,
            _max_pages: u32,
        ) -> Result<Vec<GithubIssueComment>> {
            let mut values: Vec<_> = self.comments.lock().unwrap().values().cloned().collect();
            values.sort_by_key(|comment| comment.id);
            Ok(values)
        }

        async fn get_comment(&self, comment_id: u64) -> Result<GithubIssueComment> {
            self.comments
                .lock()
                .unwrap()
                .get(&comment_id)
                .cloned()
                .ok_or_else(|| SymphonyError::GithubApiStatus {
                    status: 404,
                    message: "missing".to_string(),
                })
        }

        async fn create_comment(
            &self,
            _issue_number: u64,
            body: &str,
        ) -> Result<GithubIssueComment> {
            let mut next = self.next_id.lock().unwrap();
            let id = *next;
            *next += 1;
            *self.create_count.lock().unwrap() += 1;
            let comment = GithubIssueComment {
                id,
                user: Some(GithubUser {
                    login: self.login.clone(),
                }),
                body: Some(body.to_string()),
                html_url: None,
                created_at: None,
                updated_at: None,
            };
            self.comments.lock().unwrap().insert(id, comment.clone());
            Ok(comment)
        }

        async fn update_comment(&self, comment_id: u64, body: &str) -> Result<GithubIssueComment> {
            *self.update_count.lock().unwrap() += 1;
            let mut comments = self.comments.lock().unwrap();
            let comment =
                comments
                    .get_mut(&comment_id)
                    .ok_or_else(|| SymphonyError::GithubApiStatus {
                        status: 404,
                        message: "missing".to_string(),
                    })?;
            comment.body = Some(body.to_string());
            Ok(comment.clone())
        }
    }

    fn open_store() -> (tempfile::TempDir, SharedFactoryStore) {
        let dir = tempdir().unwrap();
        let store = SharedFactoryStore::open(&dir.path().join("factory.db"), 5_000).unwrap();
        (dir, store)
    }

    fn claim_request(configuration_revision: &str) -> ClaimAttemptRequest {
        ClaimAttemptRequest {
            forge_host: "github.com".to_string(),
            repository: "acme/repo".to_string(),
            issue_id: "42".to_string(),
            issue_identifier: "#42".to_string(),
            issue_revision: "rev".to_string(),
            configuration_revision: configuration_revision.to_string(),
            owner_instance: "test".to_string(),
            harness: "pi".to_string(),
            model: None,
            workspace_path: None,
            output_path: None,
            pid: None,
            process_group_id: None,
            process_start_token: None,
            executable_identity: None,
        }
    }

    async fn setup_preview_fixture(
        store: &SharedFactoryStore,
    ) -> (
        ReviewFindingsArtifactRecord,
        crate::review::domain::ReviewPublicationIntent,
    ) {
        let spec_stage = store.claim_spec_attempt(claim_request("spec-cfg")).unwrap();
        let approved = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: spec_stage.stage_run_id,
                issue_revision: "rev".to_string(),
                configuration_revision: "spec-cfg".to_string(),
                artifact: SpecArtifact {
                    schema_version: 1,
                    product_behavior: "Behavior".to_string(),
                    technical_approach: "Approach".to_string(),
                    acceptance_criteria: vec!["Done".to_string()],
                    open_decisions: vec![],
                },
                review_cycles: 1,
                unresolved_blocking_findings: vec![],
                bytes_len: 64,
                usage: Default::default(),
            })
            .unwrap();
        store
            .pin_spec_approval(&approved.run_id, &approved.artifact_id)
            .unwrap();
        let approval = store
            .create_spec_publication_intent(
                &approved.run_id,
                Some(&approved.artifact_id),
                SpecPublicationKind::Approval,
                &serde_json::json!({"intake_label":"ready"}),
            )
            .unwrap();
        store
            .finalize_spec_approval(&approved.run_id, &approval.intent_id, "route_applied")
            .unwrap();

        let implementation_stage = store
            .claim_implementation_attempt(claim_request("cfg"))
            .unwrap();
        let implementation_manifest = ImplementationManifest {
            schema_version: 1,
            status: ManifestStatus::Completed,
            head_commit: Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()),
            summary: "Adds retry.".to_string(),
            acceptance_criteria: vec![AcceptanceCriterionClaim {
                index: 1,
                status: CriterionStatus::Implemented,
                evidence: vec![ImplementationEvidence {
                    kind: EvidenceKind::Repository,
                    reference: "src/x.rs".to_string(),
                    summary: "bound".to_string(),
                }],
            }],
            known_limitations: vec![],
            blocker: None,
        };
        let implementation = store
            .store_implementation_artifact(StoreImplementationArtifactRequest {
                stage_run_id: implementation_stage.stage_run_id,
                approved_artifact_id: approved.artifact_id.clone(),
                approved_version: 1,
                issue_revision: "rev".to_string(),
                configuration_revision: "cfg".to_string(),
                manifest: implementation_manifest,
                base_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                head_commit: Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()),
                approved_spec_path: "specs/KATA-42/APPROVED-v1.md".to_string(),
                validation_cycles: 1,
                execution_profile: ExecutionProfile::Local,
                bytes_len: 128,
                usage: Default::default(),
            })
            .unwrap();
        let implementation_intent = store
            .create_implementation_publication_intent(
                &implementation.run_id,
                Some(&implementation.artifact_id),
                ImplementationPublicationKind::Preview,
                &serde_json::json!({"mode": "preview"}),
            )
            .unwrap();
        let draft = store
            .store_draft_pr_artifact(StoreDraftPrArtifactRequest {
                run_id: implementation.run_id.clone(),
                implementation_artifact_id: implementation.artifact_id.clone(),
                intent_id: implementation_intent.intent_id,
                number: 42,
                url: "https://github.com/acme/repo/pull/42".to_string(),
                draft: true,
                head: "symphony/42".to_string(),
                base: "main".to_string(),
                head_sha: "head".to_string(),
                marker: "marker".to_string(),
            })
            .unwrap();

        let review_stage = store
            .claim_review_attempt(claim_request("review-cfg"))
            .unwrap();
        let attempt_id = "review-attempt".to_string();
        store
            .store_review_attempt_inputs(StoreReviewAttemptRequest {
                attempt_id: attempt_id.clone(),
                stage_run_id: review_stage.stage_run_id.clone(),
                draft_pr_artifact_id: draft.artifact_id.clone(),
                implementation_artifact_id: implementation.artifact_id.clone(),
                spec_artifact_id: approved.artifact_id.clone(),
                pr_number: 42,
                reviewed_head_sha: "head".to_string(),
                base_sha: "base".to_string(),
            })
            .unwrap();
        let manifest = ReviewFindingsManifest {
            schema_version: 1,
            reviewed_head_sha: "head".to_string(),
            base_sha: "base".to_string(),
            spec_conformance_summary: "Looks good".to_string(),
            no_findings: true,
            findings: vec![],
        };
        let artifact = store
            .store_review_artifact(StoreReviewArtifactRequest {
                stage_run_id: review_stage.stage_run_id,
                attempt_id,
                draft_pr_artifact_id: draft.artifact_id,
                implementation_artifact_id: implementation.artifact_id,
                spec_artifact_id: approved.artifact_id,
                reviewed_head_sha: "head".to_string(),
                base_sha: "base".to_string(),
                manifest,
                bytes_len: 128,
                usage: Default::default(),
            })
            .unwrap();
        let intent = store
            .create_review_publication_intent(
                &artifact.run_id,
                &artifact.artifact_id,
                "preview",
                &serde_json::json!({"issue_number":42}),
            )
            .unwrap();
        (artifact, intent)
    }

    #[tokio::test]
    async fn preview_publication_against_fake_forge_is_idempotent() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments.clone());
        let (artifact, intent) = setup_preview_fixture(&store).await;

        publisher
            .publish_preview(&store, &intent, &artifact, 42, 2)
            .await
            .unwrap();
        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        assert_eq!(
            store.list_pending_review_publications().unwrap(),
            Vec::new()
        );
        let first = comments
            .comments
            .lock()
            .unwrap()
            .values()
            .next()
            .cloned()
            .unwrap();
        assert!(first
            .body
            .as_deref()
            .unwrap_or_default()
            .contains("<!-- symphony:review:"));
        assert!(first
            .body
            .as_deref()
            .unwrap_or_default()
            .contains("No findings"));

        publisher
            .publish_preview(&store, &intent, &artifact, 42, 2)
            .await
            .unwrap();
        assert_eq!(comments.comments.lock().unwrap().len(), 1);
        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        assert_eq!(*comments.update_count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn missing_bound_comment_recovers_and_rebinds_owned_marker() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments.clone());
        let (artifact, intent) = setup_preview_fixture(&store).await;
        store
            .bind_review_publication_comment(&intent.intent_id, "999", "symphony-bot")
            .unwrap();
        let marker = format!(
            "{REVIEW_COMMENT_MARKER_PREFIX}{}{REVIEW_COMMENT_MARKER_SUFFIX}",
            intent.intent_id
        );
        comments.comments.lock().unwrap().insert(
            701,
            GithubIssueComment {
                id: 701,
                user: Some(GithubUser {
                    login: "symphony-bot".to_string(),
                }),
                body: Some(marker),
                html_url: None,
                created_at: None,
                updated_at: None,
            },
        );
        let bound_intent = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();

        publisher
            .publish_preview(&store, &bound_intent, &artifact, 42, 2)
            .await
            .unwrap();

        let persisted = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(persisted.comment_id.as_deref(), Some("701"));
        assert_eq!(*comments.create_count.lock().unwrap(), 0);
        assert_eq!(*comments.update_count.lock().unwrap(), 1);
    }
}
