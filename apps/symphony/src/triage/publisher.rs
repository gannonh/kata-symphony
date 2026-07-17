use async_trait::async_trait;

use crate::error::{Result, SymphonyError};
use crate::github::client::{GithubClient, GithubIssueComment};
use crate::triage::comment::{
    self, extract_intent_id_from_marker, IneligibleCommentInput, PreviewCommentInput,
};
use crate::triage::domain::{
    PublicationIntentRecord, PublicationMode, PublicationStatus, TriageArtifact,
};
use crate::triage::store::FactoryRunStore;

pub const PREVIEW_COMMENT_STEP: &str = "preview_comment";
pub const DIAGNOSTIC_COMMENT_STEP: &str = "ineligible_diagnostic";

#[derive(Debug, Clone)]
pub struct PreviewPublishRequest<'a> {
    pub intent: &'a PublicationIntentRecord,
    pub issue_number: u64,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub attempt: u32,
    pub artifact: &'a TriageArtifact,
    pub max_pages: u32,
}

#[derive(Debug, Clone)]
pub struct IneligiblePublishRequest<'a> {
    pub intent: &'a PublicationIntentRecord,
    pub issue_number: u64,
    pub run_id: &'a str,
    pub issue_identifier: &'a str,
    pub project_name: &'a str,
    pub remediation: &'a str,
    pub max_pages: u32,
}

#[async_trait]
pub trait TriageCommentPort: Send + Sync {
    async fn authenticated_login(&self) -> Result<String>;
    async fn list_comments(
        &self,
        issue_number: u64,
        max_pages: u32,
    ) -> Result<Vec<GithubIssueComment>>;
    async fn get_comment(&self, comment_id: u64) -> Result<GithubIssueComment>;
    async fn create_comment(&self, issue_number: u64, body: &str) -> Result<GithubIssueComment>;
    async fn update_comment(&self, comment_id: u64, body: &str) -> Result<GithubIssueComment>;
}

#[async_trait]
impl TriageCommentPort for GithubClient {
    async fn authenticated_login(&self) -> Result<String> {
        let user = self.get_authenticated_user().await?;
        if user.login.trim().is_empty() {
            return Err(SymphonyError::GithubApiRequest(
                "authenticated GitHub user login is empty".to_string(),
            ));
        }
        Ok(user.login)
    }

    async fn list_comments(
        &self,
        issue_number: u64,
        max_pages: u32,
    ) -> Result<Vec<GithubIssueComment>> {
        self.list_comments_paginated(issue_number, max_pages).await
    }

    async fn get_comment(&self, comment_id: u64) -> Result<GithubIssueComment> {
        GithubClient::get_comment(self, comment_id).await
    }

    async fn create_comment(&self, issue_number: u64, body: &str) -> Result<GithubIssueComment> {
        self.create_comment_record(issue_number, body).await
    }

    async fn update_comment(&self, comment_id: u64, body: &str) -> Result<GithubIssueComment> {
        GithubClient::update_comment(self, comment_id, body).await
    }
}

/// Preview-slice publisher: marked comments only (no label/state mutations).
pub struct PreviewPublisher<C> {
    comments: C,
}

impl<C> PreviewPublisher<C>
where
    C: TriageCommentPort,
{
    pub fn new(comments: C) -> Self {
        Self { comments }
    }

    /// Reconcile a pending preview-comment intent without changing labels or project state.
    pub async fn reconcile_preview(
        &self,
        store: &mut dyn FactoryRunStore,
        request: PreviewPublishRequest<'_>,
    ) -> Result<()> {
        if request.intent.mode != PublicationMode::Preview {
            return Err(SymphonyError::TriageError(format!(
                "preview publisher cannot reconcile mode={}",
                request.intent.mode.as_str()
            )));
        }
        if request.intent.status == PublicationStatus::Applied
            && request
                .intent
                .completed_steps
                .iter()
                .any(|step| step == PREVIEW_COMMENT_STEP)
        {
            return Ok(());
        }

        let body = comment::render_preview_comment(&PreviewCommentInput {
            intent_id: &request.intent.intent_id,
            run_id: request.run_id,
            stage_run_id: request.stage_run_id,
            attempt: request.attempt,
            artifact: request.artifact,
        });

        self.upsert_marked_comment(
            store,
            request.intent,
            request.issue_number,
            &body,
            request.max_pages,
        )
        .await?;

        store.update_publication_step(
            &request.intent.intent_id,
            PREVIEW_COMMENT_STEP,
            PublicationStatus::Applied,
            None,
        )?;
        Ok(())
    }

    /// Reconcile an ineligible diagnostic comment intent.
    pub async fn reconcile_ineligible_diagnostic(
        &self,
        store: &mut dyn FactoryRunStore,
        request: IneligiblePublishRequest<'_>,
    ) -> Result<()> {
        if request.intent.status == PublicationStatus::Applied
            && request
                .intent
                .completed_steps
                .iter()
                .any(|step| step == DIAGNOSTIC_COMMENT_STEP)
        {
            return Ok(());
        }

        let body = comment::render_ineligible_diagnostic_comment(&IneligibleCommentInput {
            intent_id: &request.intent.intent_id,
            run_id: request.run_id,
            issue_identifier: request.issue_identifier,
            project_name: request.project_name,
            remediation: request.remediation,
        });

        self.upsert_marked_comment(
            store,
            request.intent,
            request.issue_number,
            &body,
            request.max_pages,
        )
        .await?;

        store.update_publication_step(
            &request.intent.intent_id,
            DIAGNOSTIC_COMMENT_STEP,
            PublicationStatus::Applied,
            None,
        )?;
        Ok(())
    }

    /// Idempotent reconcile entry for pending preview/diagnostic intents.
    pub async fn reconcile_pending_intent(
        &self,
        store: &mut dyn FactoryRunStore,
        intent: &PublicationIntentRecord,
        issue_number: u64,
        max_pages: u32,
        preview: Option<PreviewPublishRequest<'_>>,
        diagnostic: Option<IneligiblePublishRequest<'_>>,
    ) -> Result<()> {
        match intent.mode {
            PublicationMode::Preview => {
                if let Some(request) = preview {
                    return self.reconcile_preview(store, request).await;
                }
                if let Some(request) = diagnostic {
                    return self.reconcile_ineligible_diagnostic(store, request).await;
                }
                Err(SymphonyError::TriageError(
                    "pending preview intent requires preview or diagnostic payload".to_string(),
                ))
            }
            PublicationMode::Automatic => {
                // Automatic route label/state application is PR2.
                let _ = (issue_number, max_pages);
                Err(SymphonyError::TriageError(
                    "automatic publication is not implemented in the preview slice".to_string(),
                ))
            }
        }
    }

    async fn upsert_marked_comment(
        &self,
        store: &mut dyn FactoryRunStore,
        intent: &PublicationIntentRecord,
        issue_number: u64,
        body: &str,
        max_pages: u32,
    ) -> Result<()> {
        let publisher_login = self.comments.authenticated_login().await?;
        let comment_id = match intent.comment_id.as_deref() {
            Some(id) => parse_comment_id(id)?,
            None => {
                match self
                    .recover_owned_comment(
                        issue_number,
                        &intent.intent_id,
                        &publisher_login,
                        max_pages,
                    )
                    .await?
                {
                    Some(id) => id,
                    None => {
                        // create-before-record: mutate GitHub first, then persist IDs.
                        let created = self.comments.create_comment(issue_number, body).await?;
                        store.set_publication_comment(
                            &intent.intent_id,
                            &created.id.to_string(),
                            &publisher_login,
                        )?;
                        return Ok(());
                    }
                }
            }
        };

        let existing = self.comments.get_comment(comment_id).await?;
        let author = existing
            .user
            .as_ref()
            .map(|user| user.login.as_str())
            .unwrap_or("");
        if !author.eq_ignore_ascii_case(&publisher_login) {
            return Err(SymphonyError::TriageError(format!(
                "publication comment {comment_id} is not owned by publisher {publisher_login}"
            )));
        }

        self.comments.update_comment(comment_id, body).await?;
        if intent.comment_id.is_none()
            || intent.publisher_login.as_deref() != Some(&publisher_login)
        {
            store.set_publication_comment(
                &intent.intent_id,
                &comment_id.to_string(),
                &publisher_login,
            )?;
        }
        Ok(())
    }

    async fn recover_owned_comment(
        &self,
        issue_number: u64,
        intent_id: &str,
        publisher_login: &str,
        max_pages: u32,
    ) -> Result<Option<u64>> {
        let comments = self.comments.list_comments(issue_number, max_pages).await?;
        for comment in comments {
            let Some(body) = comment.body.as_deref() else {
                continue;
            };
            let Some(found_intent) = extract_intent_id_from_marker(body) else {
                continue;
            };
            if found_intent != intent_id {
                continue;
            }
            let author = comment
                .user
                .as_ref()
                .map(|user| user.login.as_str())
                .unwrap_or("");
            if author.eq_ignore_ascii_case(publisher_login) {
                return Ok(Some(comment.id));
            }
            // Spoofed marker from another author is ignored for ownership recovery.
        }
        Ok(None)
    }
}

fn parse_comment_id(raw: &str) -> Result<u64> {
    raw.parse::<u64>().map_err(|err| {
        SymphonyError::TriageError(format!("invalid publication comment_id '{raw}': {err}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::GithubUser;
    use crate::triage::domain::{
        EvidenceKind, RiskClass, TriageEvidence, TriageRoute, TRIAGE_SCHEMA_VERSION,
    };
    use crate::triage::store::{
        ClaimAttemptRequest, CreatePublicationIntentRequest, SqliteFactoryStore,
        StoreArtifactRequest,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockComments {
        login: String,
        comments: Mutex<HashMap<u64, GithubIssueComment>>,
        next_id: Mutex<u64>,
        create_count: Mutex<u32>,
        update_count: Mutex<u32>,
    }

    impl MockComments {
        fn new(login: &str) -> Arc<Self> {
            Arc::new(Self {
                login: login.to_string(),
                comments: Mutex::new(HashMap::new()),
                next_id: Mutex::new(100),
                create_count: Mutex::new(0),
                update_count: Mutex::new(0),
            })
        }
    }

    #[async_trait]
    impl TriageCommentPort for Arc<MockComments> {
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
                    message: "comment not found".to_string(),
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
                        message: "comment not found".to_string(),
                    })?;
            comment.body = Some(body.to_string());
            Ok(comment.clone())
        }
    }

    fn artifact() -> TriageArtifact {
        TriageArtifact {
            schema_version: TRIAGE_SCHEMA_VERSION,
            route: TriageRoute::Implement,
            risk_class: RiskClass::Low,
            rationale: "Bounded fix.".to_string(),
            evidence: vec![TriageEvidence {
                kind: EvidenceKind::Issue,
                reference: "body".to_string(),
                summary: "Exact replacement named.".to_string(),
            }],
            next_action: "Apply the fix.".to_string(),
            clarification_question: None,
            reproduction: None,
        }
    }

    fn store_with_preview_intent() -> (
        tempfile::TempDir,
        SqliteFactoryStore,
        PublicationIntentRecord,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = SqliteFactoryStore::acquire_lock_and_migrate(&path, 5_000).unwrap();
        let attempt = store
            .claim_attempt(ClaimAttemptRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: "12".to_string(),
                issue_identifier: "#12".to_string(),
                issue_revision: "rev".to_string(),
                configuration_revision: "cfg".to_string(),
                owner_instance: "owner".to_string(),
                harness: "pi".to_string(),
                model: None,
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })
            .unwrap();
        let artifact_record = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: attempt.stage_run_id.clone(),
                issue_revision: "rev".to_string(),
                configuration_revision: "cfg".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: artifact(),
                bytes_len: 100,
                usage: Default::default(),
            })
            .unwrap();
        let intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact_record.run_id,
                artifact_id: Some(artifact_record.artifact_id),
                mode: PublicationMode::Preview,
                intake_label: "needs-triage".to_string(),
                route_label: "ready-for-agent".to_string(),
                project_state: None,
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": "preview_comment"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();
        (temp, store, intent)
    }

    #[tokio::test]
    async fn preview_create_before_record_and_idempotent_update() {
        let (_temp, mut store, intent) = store_with_preview_intent();
        let comments = MockComments::new("symphony-bot");
        let publisher = PreviewPublisher::new(comments.clone());
        let artifact = artifact();

        publisher
            .reconcile_preview(
                &mut store,
                PreviewPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact,
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        let pending = store.list_pending_intents(10).unwrap();
        assert!(pending.is_empty());

        let applied = store
            .get_run_by_issue("github.com", "owner/repo", "12")
            .unwrap()
            .unwrap();
        let _ = applied;

        // Reload intent via a fresh pending list is empty; create another pending intent
        // path is covered by re-running reconcile with recorded comment id.
        let mut reloaded = intent.clone();
        // Simulate restart after create-before-record by reading comment from mock and
        // ensuring second reconcile updates instead of creating.
        let comment_id = comments
            .comments
            .lock()
            .unwrap()
            .keys()
            .next()
            .copied()
            .unwrap();
        store
            .set_publication_comment(&intent.intent_id, &comment_id.to_string(), "symphony-bot")
            .unwrap();
        // Reset status to pending for second reconcile.
        store
            .update_publication_step(&intent.intent_id, "", PublicationStatus::Pending, None)
            .unwrap();
        reloaded.comment_id = Some(comment_id.to_string());
        reloaded.publisher_login = Some("symphony-bot".to_string());
        reloaded.status = PublicationStatus::Pending;
        reloaded.completed_steps.clear();

        publisher
            .reconcile_preview(
                &mut store,
                PreviewPublishRequest {
                    intent: &reloaded,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact,
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        assert!(*comments.update_count.lock().unwrap() >= 1);
    }

    #[tokio::test]
    async fn recovery_ignores_spoofed_marker_and_creates_owned_comment() {
        let (_temp, mut store, intent) = store_with_preview_intent();
        let comments = MockComments::new("symphony-bot");
        let marker = comment::marker(&intent.intent_id);
        comments.comments.lock().unwrap().insert(
            1,
            GithubIssueComment {
                id: 1,
                user: Some(GithubUser {
                    login: "attacker".to_string(),
                }),
                body: Some(format!("{marker}\n\nspoofed")),
                html_url: None,
                created_at: None,
                updated_at: None,
            },
        );

        let publisher = PreviewPublisher::new(comments.clone());
        publisher
            .reconcile_preview(
                &mut store,
                PreviewPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        let owned: Vec<_> = comments
            .comments
            .lock()
            .unwrap()
            .values()
            .filter(|comment| {
                comment
                    .user
                    .as_ref()
                    .map(|user| user.login == "symphony-bot")
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        assert_eq!(owned.len(), 1);
    }

    #[tokio::test]
    async fn recover_create_before_record_crash_window() {
        let (_temp, mut store, intent) = store_with_preview_intent();
        let comments = MockComments::new("symphony-bot");
        let body = comment::render_preview_comment(&PreviewCommentInput {
            intent_id: &intent.intent_id,
            run_id: "run",
            stage_run_id: "stage",
            attempt: 1,
            artifact: &artifact(),
        });
        // Simulate create succeeding before SQLite recorded the comment id.
        let created = comments.create_comment(12, &body).await.unwrap();
        assert!(intent.comment_id.is_none());

        let publisher = PreviewPublisher::new(comments.clone());
        publisher
            .reconcile_preview(
                &mut store,
                PreviewPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        // Recovery should update the existing owned comment, not create another.
        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        assert_eq!(comments.comments.lock().unwrap().len(), 1);
        let _ = created;
    }

    #[tokio::test]
    async fn ineligible_diagnostic_upserts_marked_comment() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = SqliteFactoryStore::acquire_lock_and_migrate(&path, 5_000).unwrap();
        let attempt = store
            .claim_attempt(ClaimAttemptRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: "99".to_string(),
                issue_identifier: "#99".to_string(),
                issue_revision: "rev".to_string(),
                configuration_revision: "cfg".to_string(),
                owner_instance: "owner".to_string(),
                harness: "pi".to_string(),
                model: None,
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })
            .unwrap();
        let intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: attempt.run_id.clone(),
                artifact_id: None,
                mode: PublicationMode::Preview,
                intake_label: "needs-triage".to_string(),
                route_label: "".to_string(),
                project_state: None,
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": "ineligible_diagnostic"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();

        let comments = MockComments::new("symphony-bot");
        let publisher = PreviewPublisher::new(comments.clone());
        publisher
            .reconcile_ineligible_diagnostic(
                &mut store,
                IneligiblePublishRequest {
                    intent: &intent,
                    issue_number: 99,
                    run_id: &attempt.run_id,
                    issue_identifier: "#99",
                    project_name: "Factory",
                    remediation: "Add the issue to the project.",
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        let body = comments
            .comments
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .body
            .clone()
            .unwrap();
        assert!(body.contains("not eligible for triage"));
        assert!(body.contains(&intent.intent_id));
    }
}
