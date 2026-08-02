//! Idempotent marker-owned findings preview publication.

use crate::error::{Result, SymphonyError};
use crate::github::client::{GithubPullRequestReview, GithubPullRequestReviewComment};
use crate::review::domain::{
    ReviewFindingsArtifactRecord, ReviewPublicationIntent, REVIEW_COMMENT_MARKER_PREFIX,
    REVIEW_COMMENT_MARKER_SUFFIX,
};
use crate::review::findings::{render_formal_review_body_with_records, render_preview_comment};
use crate::triage::publisher::TriageCommentPort;
use crate::triage::runtime::SharedFactoryStore;
use uuid::Uuid;

pub const REVIEW_PREVIEW_COMMENT_STEP: &str = "review_preview_comment";
const REVIEW_PUBLICATION_LEASE_SECONDS: i64 = 900;
pub const REVIEW_CREATED_STEP: &str = "review_created";
pub const FINDINGS_RECORDED_STEP: &str = "findings_recorded";

/// Forge operations required for atomic pull-request review publication.
#[async_trait::async_trait]
pub trait ReviewPort: Send + Sync {
    async fn authenticated_login(&self) -> Result<String>;
    async fn list_pull_request_reviews(
        &self,
        number: u64,
        max_pages: u32,
    ) -> Result<Vec<GithubPullRequestReview>>;
    async fn pull_request_head_sha(&self, number: u64) -> Result<String>;
    async fn create_pull_request_review(
        &self,
        number: u64,
        commit_id: &str,
        body: &str,
        comments: &[GithubPullRequestReviewComment],
    ) -> Result<GithubPullRequestReview>;
}

#[async_trait::async_trait]
impl ReviewPort for crate::github::client::GithubClient {
    async fn authenticated_login(&self) -> Result<String> {
        let user = self.get_authenticated_user().await?;
        if user.login.trim().is_empty() {
            return Err(SymphonyError::GithubApiRequest(
                "authenticated GitHub user login is empty".to_string(),
            ));
        }
        Ok(user.login)
    }

    async fn list_pull_request_reviews(
        &self,
        number: u64,
        max_pages: u32,
    ) -> Result<Vec<GithubPullRequestReview>> {
        crate::github::client::GithubClient::list_pull_request_reviews(self, number, max_pages)
            .await
    }

    async fn pull_request_head_sha(&self, number: u64) -> Result<String> {
        Ok(self.get_pull_request(number).await?.head.sha)
    }

    async fn create_pull_request_review(
        &self,
        number: u64,
        commit_id: &str,
        body: &str,
        comments: &[GithubPullRequestReviewComment],
    ) -> Result<GithubPullRequestReview> {
        crate::github::client::GithubClient::create_pull_request_review(
            self, number, commit_id, body, comments,
        )
        .await
    }
}

#[derive(Clone)]
pub struct ReviewPublisher<C> {
    comments: C,
    owner_instance: String,
}

impl<C> ReviewPublisher<C>
where
    C: TriageCommentPort + Clone,
{
    pub fn new(comments: C) -> Self {
        Self::with_owner(comments, format!("review-publisher-{}", Uuid::now_v7()))
    }

    pub fn with_owner(comments: C, owner_instance: impl Into<String>) -> Self {
        Self {
            comments,
            owner_instance: owner_instance.into(),
        }
    }

    pub async fn publish_formal<R>(
        &self,
        store: &SharedFactoryStore,
        review_port: &R,
        intent: &ReviewPublicationIntent,
        artifact: &ReviewFindingsArtifactRecord,
        pull_request_number: u64,
        max_pages: u32,
    ) -> Result<bool>
    where
        R: ReviewPort + ?Sized,
    {
        if intent.kind != "automatic" && intent.kind != "formal" {
            return Err(SymphonyError::TriageError(format!(
                "formal review publisher cannot reconcile kind={}",
                intent.kind
            )));
        }
        if intent
            .completed_steps
            .iter()
            .any(|step| step == FINDINGS_RECORDED_STEP)
        {
            return Ok(true);
        }
        if !store.claim_review_publication(
            &intent.intent_id,
            &self.owner_instance,
            REVIEW_PUBLICATION_LEASE_SECONDS,
        )? {
            return Ok(false);
        }

        let publisher_login = review_port.authenticated_login().await?;
        let marker = format!(
            "{REVIEW_COMMENT_MARKER_PREFIX}{}{REVIEW_COMMENT_MARKER_SUFFIX}",
            intent.intent_id
        );
        // The approved specification version is captured in the intent at claim time.
        // Older intents fall back to the review schema version for compatibility.
        let approved_spec_version = intent
            .desired_effects
            .get("approved_spec_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok());
        let finding_records =
            store.list_review_finding_records_for_artifact(&artifact.artifact_id)?;
        let body = render_formal_review_body_with_records(
            pull_request_number,
            &intent.intent_id,
            &intent.run_id,
            artifact,
            approved_spec_version,
            &finding_records,
        );
        let comments = render_review_comments(artifact, &finding_records);

        let review_created = intent
            .completed_steps
            .iter()
            .any(|step| step == REVIEW_CREATED_STEP);
        let bound_identity = intent.review_id.is_some()
            && intent.review_url.is_some()
            && intent.publisher_login.is_some();
        if bound_identity {
            let durable_publisher_login = intent
                .publisher_login
                .as_deref()
                .expect("bound publisher login");
            if !durable_publisher_login.eq_ignore_ascii_case(&publisher_login) {
                return Err(SymphonyError::TriageError(format!(
                    "formal review publisher identity conflict: durable login {durable_publisher_login} does not match authenticated login {publisher_login}"
                )));
            }
        }

        let review = if review_created || bound_identity || intent.review_id.is_some() {
            None
        } else {
            let existing = review_port
                .list_pull_request_reviews(pull_request_number, max_pages)
                .await?;
            let mut owned = None;
            for candidate in existing.into_iter().filter(|review| {
                review
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains(&marker))
            }) {
                let author = candidate
                    .user
                    .as_ref()
                    .map(|user| user.login.as_str())
                    .unwrap_or_default();
                if !author.eq_ignore_ascii_case(&publisher_login) {
                    return Err(SymphonyError::TriageError(format!(
                        "formal review marker {marker} is owned by another GitHub login {author}"
                    )));
                }
                if candidate.commit_id != artifact.reviewed_head_sha {
                    return Err(SymphonyError::TriageError(format!(
                        "formal review marker {marker} conflict: head {} does not match expected {}",
                        candidate.commit_id, artifact.reviewed_head_sha
                    )));
                }
                if owned.is_some() {
                    return Err(SymphonyError::TriageError(format!(
                        "multiple formal reviews found for marker {marker}"
                    )));
                }
                owned = Some(candidate);
            }
            if let Some(existing) = owned {
                Some(existing)
            } else {
                let live_head = review_port
                    .pull_request_head_sha(pull_request_number)
                    .await?;
                if live_head != artifact.reviewed_head_sha {
                    store
                        .clear_review_publication_lease(&intent.intent_id, &self.owner_instance)?;
                    return Err(SymphonyError::TriageError(format!(
                        "review cycle reopened before formal review creation: live head {} does not match expected {}",
                        live_head, artifact.reviewed_head_sha
                    )));
                }
                let created = match review_port
                    .create_pull_request_review(
                        pull_request_number,
                        &artifact.reviewed_head_sha,
                        &body,
                        &comments,
                    )
                    .await
                {
                    Ok(review) => review,
                    Err(error) => {
                        let live_head =
                            review_port.pull_request_head_sha(pull_request_number).await;
                        if let Ok(live_head) = live_head {
                            if live_head != artifact.reviewed_head_sha {
                                store.clear_review_publication_lease(
                                    &intent.intent_id,
                                    &self.owner_instance,
                                )?;
                                return Err(SymphonyError::TriageError(format!(
                                    "review cycle reopened during formal review creation: live head {} does not match expected {}",
                                    live_head, artifact.reviewed_head_sha
                                )));
                            }
                        }
                        return Err(error);
                    }
                };
                Some(created)
            }
        };

        if review.is_some() || bound_identity {
            let live_head = review_port
                .pull_request_head_sha(pull_request_number)
                .await?;
            if live_head != artifact.reviewed_head_sha {
                store.clear_review_publication_lease(&intent.intent_id, &self.owner_instance)?;
                return Err(SymphonyError::TriageError(format!(
                    "review cycle reopened while accepting formal review identity: live head {} does not match expected {}",
                    live_head, artifact.reviewed_head_sha
                )));
            }
        }

        if bound_identity && !review_created {
            let review_id = intent.review_id.as_deref().expect("bound review id");
            let review_url = intent.review_url.as_deref().expect("bound review URL");
            let durable_publisher_login = intent
                .publisher_login
                .as_deref()
                .expect("bound publisher login");
            store.record_review_publication_step(
                &intent.intent_id,
                REVIEW_CREATED_STEP,
                crate::triage::domain::PublicationStatus::Pending,
                &serde_json::json!({
                    "review_id": review_id,
                    "review_url": review_url,
                    "publisher_login": durable_publisher_login,
                }),
            )?;
        }

        if let Some(review) = review {
            let review_url = review.html_url.clone().unwrap_or_default();
            store.bind_review_publication_review(
                &intent.intent_id,
                &review.id.to_string(),
                &review_url,
                &publisher_login,
            )?;
            store.record_review_publication_step(
                &intent.intent_id,
                REVIEW_CREATED_STEP,
                crate::triage::domain::PublicationStatus::Pending,
                &serde_json::json!({
                    "review_id": review.id.to_string(),
                    "review_url": review_url,
                    "publisher_login": publisher_login,
                }),
            )?;
        } else if !review_created && !bound_identity {
            return Err(SymphonyError::TriageError(
                "formal review has no forge identity to record".to_string(),
            ));
        }

        store.record_review_publication_step(
            &intent.intent_id,
            FINDINGS_RECORDED_STEP,
            crate::triage::domain::PublicationStatus::Pending,
            &serde_json::json!({
                "finding_count": artifact.finding_count,
                "inline_comment_count": comments.len(),
            }),
        )?;
        store.clear_review_publication_lease(&intent.intent_id, &self.owner_instance)?;
        Ok(true)
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

fn render_review_comments(
    artifact: &ReviewFindingsArtifactRecord,
    finding_records: &[crate::review::domain::ReviewFindingRecord],
) -> Vec<GithubPullRequestReviewComment> {
    artifact
        .manifest
        .findings
        .iter()
        .filter(|finding| {
            !finding_records.iter().any(|record| {
                record.artifact_id == artifact.artifact_id
                    && record.finding_id == finding.finding_id
                    && record.lifecycle_state == "persisting"
            })
        })
        .map(|finding| {
            let end_line = finding.end_line.unwrap_or(finding.line);
            let multiline = end_line > finding.line;
            let severity = serde_json::to_string(&finding.severity)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
                .to_string();
            let category = serde_json::to_string(&finding.category)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
                .to_string();
            let mut body = format!(
                "**{severity}** ({category})\n\n{}\n\n**Why:** {}\n\n**Suggested remediation:** {}",
                finding.claim, finding.rationale, finding.remediation
            );
            if let Some(criterion) = finding.acceptance_criterion.as_deref() {
                body.push_str(&format!("\n\n**Acceptance criterion:** {criterion}"));
            }
            GithubPullRequestReviewComment {
                path: finding.path.clone(),
                line: end_line,
                side: "RIGHT".to_string(),
                start_line: multiline.then_some(finding.line),
                start_side: multiline.then_some("RIGHT".to_string()),
                body,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::{
        GithubIssueComment, GithubPullRequestReview, GithubPullRequestReviewComment, GithubUser,
    };
    use crate::implementation::domain::{
        AcceptanceCriterionClaim, CriterionStatus, EvidenceKind, ExecutionProfile,
        ImplementationEvidence, ImplementationManifest, ImplementationPublicationKind,
        ManifestStatus,
    };
    use crate::review::domain::{ReviewFindingRecord, ReviewFindingsArtifactRecord};
    use crate::review::manifest::{
        ReviewFinding, ReviewFindingCategory, ReviewFindingsManifest, ReviewSeverity,
    };
    use crate::spec::domain::{SpecArtifact, SpecPublicationKind};
    use crate::triage::domain::PublicationStatus;
    use crate::triage::publisher::TriageCommentPort;
    use crate::triage::runtime::SharedFactoryStore;
    use crate::triage::store::{
        ClaimAttemptRequest, StoreDraftPrArtifactRequest, StoreImplementationArtifactRequest,
        StoreReviewArtifactRequest, StoreReviewAttemptRequest, StoreSpecArtifactRequest,
    };
    use async_trait::async_trait;
    use chrono::Utc;
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

    struct FakeReviews {
        login: String,
        head_sha: Mutex<String>,
        change_head_on_create: Mutex<bool>,
        change_head_after_create: Mutex<bool>,
        reviews: Mutex<Vec<GithubPullRequestReview>>,
        review_payloads: Mutex<Vec<(String, Vec<GithubPullRequestReviewComment>)>>,
    }

    impl FakeReviews {
        fn new(login: &str) -> Arc<Self> {
            Arc::new(Self {
                login: login.to_string(),
                head_sha: Mutex::new("head".to_string()),
                change_head_on_create: Mutex::new(false),
                change_head_after_create: Mutex::new(false),
                reviews: Mutex::new(Vec::new()),
                review_payloads: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl ReviewPort for Arc<FakeReviews> {
        async fn authenticated_login(&self) -> Result<String> {
            Ok(self.login.clone())
        }

        async fn list_pull_request_reviews(
            &self,
            _number: u64,
            _max_pages: u32,
        ) -> Result<Vec<GithubPullRequestReview>> {
            Ok(self.reviews.lock().unwrap().clone())
        }

        async fn pull_request_head_sha(&self, _number: u64) -> Result<String> {
            Ok(self.head_sha.lock().unwrap().clone())
        }

        async fn create_pull_request_review(
            &self,
            _number: u64,
            commit_id: &str,
            body: &str,
            comments: &[GithubPullRequestReviewComment],
        ) -> Result<GithubPullRequestReview> {
            if *self.change_head_on_create.lock().unwrap() {
                *self.head_sha.lock().unwrap() = "new-head".to_string();
                return Err(SymphonyError::GithubApiStatus {
                    status: 422,
                    message: "head changed during create".to_string(),
                });
            }
            self.review_payloads
                .lock()
                .unwrap()
                .push((body.to_string(), comments.to_vec()));
            let review = GithubPullRequestReview {
                id: 900,
                user: Some(GithubUser {
                    login: self.login.clone(),
                }),
                body: Some(body.to_string()),
                commit_id: commit_id.to_string(),
                state: "COMMENTED".to_string(),
                html_url: Some("https://github.test/reviews/900".to_string()),
                submitted_at: None,
            };
            self.reviews.lock().unwrap().push(review.clone());
            if *self.change_head_after_create.lock().unwrap() {
                *self.head_sha.lock().unwrap() = "new-head".to_string();
            }
            Ok(review)
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
        setup_review_fixture(store, "preview").await
    }

    async fn setup_review_fixture(
        store: &SharedFactoryStore,
        kind: &str,
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
                kind,
                &serde_json::json!({"issue_number":42, "approved_spec_version": 7}),
            )
            .unwrap();
        (artifact, intent)
    }

    #[test]
    fn formal_comments_suppress_only_persisting_findings() {
        let now = Utc::now();
        let finding = |id: &str, claim: &str| ReviewFinding {
            finding_id: id.to_string(),
            severity: ReviewSeverity::Major,
            category: ReviewFindingCategory::Correctness,
            path: "src/lib.rs".to_string(),
            line: 10,
            end_line: None,
            claim: claim.to_string(),
            rationale: "rationale".to_string(),
            remediation: "remediation".to_string(),
            acceptance_criterion: None,
            confidence: 0.9,
        };
        let artifact = ReviewFindingsArtifactRecord {
            artifact_id: "artifact".to_string(),
            run_id: "run".to_string(),
            stage_run_id: "stage".to_string(),
            attempt_id: "attempt".to_string(),
            draft_pr_artifact_id: "draft".to_string(),
            implementation_artifact_id: "implementation".to_string(),
            spec_artifact_id: "spec".to_string(),
            schema_version: 1,
            reviewed_head_sha: "head".to_string(),
            base_sha: "base".to_string(),
            manifest: ReviewFindingsManifest {
                schema_version: 1,
                reviewed_head_sha: "head".to_string(),
                base_sha: "base".to_string(),
                spec_conformance_summary: "Conforms".to_string(),
                no_findings: false,
                findings: vec![
                    finding("persisting", "keep in summary"),
                    finding("new", "fresh"),
                ],
            },
            no_findings: false,
            finding_count: 2,
            received_at: now,
            bytes_len: 1,
        };
        let records = vec![
            ReviewFindingRecord {
                finding_record_id: "record-1".to_string(),
                run_id: "run".to_string(),
                artifact_id: "artifact".to_string(),
                finding_id: "persisting".to_string(),
                identity_key: "src/lib.rs:10:10:keep in summary".to_string(),
                reviewed_head_sha: "head".to_string(),
                severity: ReviewSeverity::Major,
                category: ReviewFindingCategory::Correctness,
                path: "src/lib.rs".to_string(),
                line: 10,
                end_line: None,
                claim: "keep in summary".to_string(),
                rationale: "rationale".to_string(),
                remediation: "remediation".to_string(),
                acceptance_criterion: None,
                confidence: 0.9,
                lifecycle_state: "persisting".to_string(),
                created_at: now,
                updated_at: now,
            },
            ReviewFindingRecord {
                finding_record_id: "record-2".to_string(),
                run_id: "run".to_string(),
                artifact_id: "artifact".to_string(),
                finding_id: "new".to_string(),
                identity_key: "src/lib.rs:10:10:fresh".to_string(),
                reviewed_head_sha: "head".to_string(),
                severity: ReviewSeverity::Major,
                category: ReviewFindingCategory::Correctness,
                path: "src/lib.rs".to_string(),
                line: 10,
                end_line: None,
                claim: "fresh".to_string(),
                rationale: "rationale".to_string(),
                remediation: "remediation".to_string(),
                acceptance_criterion: None,
                confidence: 0.9,
                lifecycle_state: "new".to_string(),
                created_at: now,
                updated_at: now,
            },
        ];
        let comments = render_review_comments(&artifact, &records);
        assert_eq!(comments.len(), 1);
        assert!(comments[0].body.contains("fresh"));
        assert!(!comments[0].body.contains("keep in summary"));
        let body =
            render_formal_review_body_with_records(42, "intent", "run", &artifact, None, &records);
        assert!(body.contains("keep in summary"));
    }

    #[tokio::test]
    async fn artifact_head_lookup_distinguishes_review_cycles() {
        let (_dir, store) = open_store();
        let (artifact, _intent) = setup_preview_fixture(&store).await;

        assert!(store
            .review_artifact_exists_for_head(&artifact.run_id, "head")
            .unwrap());
        assert!(!store
            .review_artifact_exists_for_head(&artifact.run_id, "new-head")
            .unwrap());
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
    async fn fresh_intent_round_trips_formal_identity_and_progressive_steps() {
        let (_dir, store) = open_store();
        let (_artifact, intent) = setup_preview_fixture(&store).await;

        let fresh = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(fresh.review_id, None);
        assert_eq!(fresh.review_url, None);
        assert_eq!(fresh.route_state, None);

        store
            .bind_review_publication_review(
                &intent.intent_id,
                "review-17",
                "https://example.test/reviews/17",
                "symphony-bot",
            )
            .unwrap();
        store
            .set_review_publication_route_state(&intent.intent_id, "Human Review")
            .unwrap();
        store
            .record_review_publication_step(
                &intent.intent_id,
                "review_created",
                PublicationStatus::Pending,
                &serde_json::json!({"review_id": "review-17"}),
            )
            .unwrap();
        let pending = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(pending.status, PublicationStatus::Pending);
        assert_eq!(pending.review_id.as_deref(), Some("review-17"));
        assert_eq!(
            pending.review_url.as_deref(),
            Some("https://example.test/reviews/17")
        );
        assert_eq!(pending.route_state.as_deref(), Some("Human Review"));

        store
            .record_review_publication_step(
                &intent.intent_id,
                "findings_recorded",
                PublicationStatus::Pending,
                &serde_json::json!({"finding_count": 0}),
            )
            .unwrap();
        store
            .record_review_publication_step(
                &intent.intent_id,
                "route_applied",
                PublicationStatus::Pending,
                &serde_json::json!({"route_state": "Human Review"}),
            )
            .unwrap();
        store
            .record_review_publication_step(
                &intent.intent_id,
                "comment_final",
                PublicationStatus::Pending,
                &serde_json::json!({"comment_id": "701"}),
            )
            .unwrap();
        let applied = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(applied.status, PublicationStatus::Applied);
        assert_eq!(applied.completed_steps.len(), 4);
    }

    #[tokio::test]
    async fn applied_formal_intent_stays_terminal_when_a_nonfinal_step_replays() {
        let (_dir, store) = open_store();
        let (_artifact, intent) = setup_review_fixture(&store, "formal").await;

        store
            .record_review_publication_step(
                &intent.intent_id,
                "comment_final",
                PublicationStatus::Pending,
                &serde_json::json!({"comment_id":"701"}),
            )
            .unwrap();
        store
            .record_review_publication_step(
                &intent.intent_id,
                "route_applied",
                PublicationStatus::Pending,
                &serde_json::json!({"route_state":"Human Review"}),
            )
            .unwrap();

        let applied = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(applied.status, PublicationStatus::Applied);
        assert_eq!(applied.completed_steps, vec!["comment_final"]);
        assert_eq!(
            applied.expected_projection,
            serde_json::json!({"comment_id":"701"})
        );
    }

    #[tokio::test]
    async fn preview_completion_helper_rejects_formal_intents() {
        let (_dir, store) = open_store();
        let (_artifact, intent) = setup_review_fixture(&store, "formal").await;

        assert!(store
            .complete_review_publication(&intent.intent_id, REVIEW_PREVIEW_COMMENT_STEP)
            .is_err());
        let unchanged = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.status, PublicationStatus::Pending);
        assert!(unchanged.completed_steps.is_empty());
    }

    #[tokio::test]
    async fn formal_publication_sends_one_atomic_review_with_multiline_anchor() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("comment-bot");
        let reviews = FakeReviews::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments.clone());
        let (mut artifact, intent) = setup_review_fixture(&store, "automatic").await;
        artifact.manifest.no_findings = false;
        artifact.manifest.findings = vec![ReviewFinding {
            finding_id: "f-1".to_string(),
            severity: ReviewSeverity::Major,
            category: ReviewFindingCategory::Correctness,
            path: "src/lib.rs".to_string(),
            line: 10,
            end_line: Some(12),
            claim: "The retry loses the error".to_string(),
            rationale: "The error is discarded in the changed branch".to_string(),
            remediation: "Return the original error".to_string(),
            acceptance_criterion: None,
            confidence: 0.9,
        }];
        artifact.finding_count = 1;

        publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap();

        let payloads = reviews.review_payloads.lock().unwrap();
        assert_eq!(payloads.len(), 1);
        assert!(payloads[0].0.contains("<!-- symphony:review:"));
        assert!(payloads[0].0.contains("formal review"));
        assert!(payloads[0].0.contains("Issue `#42`"));
        assert!(payloads[0].0.contains(&artifact.spec_artifact_id));
        assert!(payloads[0].0.contains("version `7`"));
        assert!(payloads[0].0.contains(&artifact.run_id));
        assert!(payloads[0]
            .0
            .contains("reviewed head `head` against base `base`"));
        assert_eq!(payloads[0].1.len(), 1);
        assert_eq!(payloads[0].1[0].line, 12);
        assert_eq!(payloads[0].1[0].start_line, Some(10));
        assert_eq!(payloads[0].1[0].start_side.as_deref(), Some("RIGHT"));
        let persisted = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(persisted.status, PublicationStatus::Pending);
        assert_eq!(persisted.review_id.as_deref(), Some("900"));
        assert_eq!(persisted.publisher_login.as_deref(), Some("symphony-bot"));
        assert_eq!(
            persisted.completed_steps,
            vec![REVIEW_CREATED_STEP, FINDINGS_RECORDED_STEP]
        );
    }

    #[tokio::test]
    async fn formal_publication_adopts_owned_marker_without_duplicate_create() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("comment-bot");
        let reviews = FakeReviews::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments.clone());
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;

        publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap();
        publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap();

        assert_eq!(reviews.review_payloads.lock().unwrap().len(), 1);
        let persisted = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(persisted.status, PublicationStatus::Pending);
        assert_eq!(
            persisted.completed_steps,
            vec![REVIEW_CREATED_STEP, FINDINGS_RECORDED_STEP]
        );
    }

    #[tokio::test]
    async fn resuming_bound_formal_review_requires_the_durable_publisher_identity() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("comment-bot");
        let reviews = FakeReviews::new("current-bot");
        let publisher = ReviewPublisher::new(comments);
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;
        store
            .bind_review_publication_review(
                &intent.intent_id,
                "review-17",
                "https://github.test/reviews/17",
                "durable-bot",
            )
            .unwrap();
        let bound_intent = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();

        let error = publisher
            .publish_formal(&store, &reviews, &bound_intent, &artifact, 42, 2)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("publisher identity conflict"));
        assert_eq!(
            store
                .get_review_publication_intent(&intent.intent_id)
                .unwrap()
                .unwrap()
                .retry_count,
            0
        );
    }

    #[tokio::test]
    async fn formal_publication_recovers_bound_identity_before_recorded_step() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let reviews = FakeReviews::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments);
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;
        store
            .bind_review_publication_review(
                &intent.intent_id,
                "review-17",
                "https://github.test/reviews/17",
                "symphony-bot",
            )
            .unwrap();
        let bound = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();

        publisher
            .publish_formal(&store, &reviews, &bound, &artifact, 42, 2)
            .await
            .unwrap();

        let persisted = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            persisted.completed_steps,
            vec![REVIEW_CREATED_STEP, FINDINGS_RECORDED_STEP]
        );
        assert_eq!(persisted.review_id.as_deref(), Some("review-17"));
        assert!(reviews.review_payloads.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn formal_publication_rejects_live_head_change_before_create() {
        let (_temp, store) = open_store();
        let reviews = FakeReviews::new("symphony-bot");
        let publisher = ReviewPublisher::new(FakeComments::new("symphony-bot"));
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;
        *reviews.head_sha.lock().unwrap() = "new-head".to_string();

        let error = publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("live head"));
        assert!(reviews.review_payloads.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn formal_publication_waits_when_head_changes_during_create() {
        let (_temp, store) = open_store();
        let reviews = FakeReviews::new("symphony-bot");
        *reviews.change_head_on_create.lock().unwrap() = true;
        let publisher = ReviewPublisher::new(FakeComments::new("symphony-bot"));
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;

        let error = publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("review cycle reopened"));
        assert!(reviews.review_payloads.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn formal_publication_waits_when_successful_create_returns_stale_head() {
        let (_temp, store) = open_store();
        let reviews = FakeReviews::new("symphony-bot");
        *reviews.change_head_after_create.lock().unwrap() = true;
        let publisher = ReviewPublisher::new(FakeComments::new("symphony-bot"));
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;

        let error = publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("review cycle reopened"));
        assert_eq!(reviews.review_payloads.lock().unwrap().len(), 1);
        assert_eq!(reviews.reviews.lock().unwrap().len(), 1);
        let persisted = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert!(persisted.completed_steps.is_empty());
        assert_eq!(persisted.retry_count, 0);
    }

    #[tokio::test]
    async fn formal_publication_rejects_marker_with_stale_head() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let reviews = FakeReviews::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments);
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;
        let marker = format!(
            "{REVIEW_COMMENT_MARKER_PREFIX}{}{REVIEW_COMMENT_MARKER_SUFFIX}",
            intent.intent_id
        );
        reviews
            .reviews
            .lock()
            .unwrap()
            .push(GithubPullRequestReview {
                id: 901,
                user: Some(GithubUser {
                    login: "symphony-bot".to_string(),
                }),
                body: Some(marker),
                commit_id: "stale-head".to_string(),
                state: "COMMENTED".to_string(),
                html_url: Some("https://github.test/reviews/901".to_string()),
                submitted_at: None,
            });

        let error = publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("conflict"));
        assert!(reviews.review_payloads.lock().unwrap().is_empty());
        assert!(store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap()
            .completed_steps
            .is_empty());
    }

    #[tokio::test]
    async fn formal_publication_waits_when_owned_marker_head_changes_after_precheck() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let reviews = FakeReviews::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments);
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;
        let marker = format!(
            "{REVIEW_COMMENT_MARKER_PREFIX}{}{REVIEW_COMMENT_MARKER_SUFFIX}",
            intent.intent_id
        );
        reviews
            .reviews
            .lock()
            .unwrap()
            .push(GithubPullRequestReview {
                id: 902,
                user: Some(GithubUser {
                    login: "symphony-bot".to_string(),
                }),
                body: Some(marker),
                commit_id: "head".to_string(),
                state: "COMMENTED".to_string(),
                html_url: Some("https://github.test/reviews/902".to_string()),
                submitted_at: None,
            });
        *reviews.head_sha.lock().unwrap() = "new-head".to_string();

        let error = publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("review cycle reopened"));
        assert!(store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap()
            .completed_steps
            .is_empty());
    }

    #[tokio::test]
    async fn formal_publication_waits_when_bound_identity_head_changes_after_precheck() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let reviews = FakeReviews::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments);
        let (artifact, intent) = setup_review_fixture(&store, "formal").await;
        store
            .bind_review_publication_review(
                &intent.intent_id,
                "review-17",
                "https://github.test/reviews/17",
                "symphony-bot",
            )
            .unwrap();
        *reviews.head_sha.lock().unwrap() = "new-head".to_string();
        let bound = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();

        let error = publisher
            .publish_formal(&store, &reviews, &bound, &artifact, 42, 2)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("review cycle reopened"));
        let persisted = store
            .get_review_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert!(persisted.completed_steps.is_empty());
        assert_eq!(persisted.retry_count, 0);
    }

    #[tokio::test]
    async fn formal_publication_rejects_foreign_marker_owner() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let reviews = FakeReviews::new("symphony-bot");
        let publisher = ReviewPublisher::new(comments.clone());
        let (artifact, intent) = setup_review_fixture(&store, "automatic").await;
        let marker = format!(
            "{REVIEW_COMMENT_MARKER_PREFIX}{}{REVIEW_COMMENT_MARKER_SUFFIX}",
            intent.intent_id
        );
        reviews
            .reviews
            .lock()
            .unwrap()
            .push(GithubPullRequestReview {
                id: 901,
                user: Some(GithubUser {
                    login: "human".to_string(),
                }),
                body: Some(marker),
                commit_id: "head".to_string(),
                state: "COMMENTED".to_string(),
                html_url: Some("https://github.test/reviews/901".to_string()),
                submitted_at: None,
            });

        let error = publisher
            .publish_formal(&store, &reviews, &intent, &artifact, 42, 2)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("another GitHub login"));
        assert!(reviews.review_payloads.lock().unwrap().is_empty());
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
