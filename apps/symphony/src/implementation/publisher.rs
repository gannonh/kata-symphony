//! Preview comment and (PR2) branch/PR/tracker publication.

use crate::error::{Result, SymphonyError};
use crate::implementation::comment::{
    render_diagnostic_comment, render_preview_comment, ImplementationPreviewComment,
};
use crate::implementation::domain::{
    ImplementationArtifactRecord, ImplementationManifest, ImplementationPublicationIntent,
    ImplementationPublicationKind, ValidationCommandResult, IMPLEMENTATION_COMMENT_MARKER_PREFIX,
    IMPLEMENTATION_COMMENT_MARKER_SUFFIX,
};
use crate::triage::domain::PublicationStatus;
use crate::triage::publisher::TriageCommentPort;
use crate::triage::runtime::SharedFactoryStore;

pub const PREVIEW_COMMENT_STEP: &str = "preview_comment";
pub const DIAGNOSTIC_COMMENT_STEP: &str = "diagnostic_comment";

#[derive(Debug, Clone)]
pub struct PreviewPublishRequest<'a> {
    pub intent: &'a ImplementationPublicationIntent,
    pub issue_number: u64,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub artifact: &'a ImplementationArtifactRecord,
    pub manifest: &'a ImplementationManifest,
    pub changed_paths: &'a [String],
    pub validation: &'a [ValidationCommandResult],
    pub max_pages: u32,
}

#[derive(Debug, Clone)]
pub struct DiagnosticPublishRequest<'a> {
    pub intent: &'a ImplementationPublicationIntent,
    pub issue_number: u64,
    pub run_id: &'a str,
    pub message: &'a str,
    pub max_pages: u32,
}

/// Preview-only publisher: owned issue comments, no branch/PR/label/state mutation.
pub struct ImplementationPublisher<C> {
    comments: C,
}

impl<C> ImplementationPublisher<C>
where
    C: TriageCommentPort,
{
    pub fn new(comments: C) -> Self {
        Self { comments }
    }

    pub async fn publish_preview(
        &self,
        store: &SharedFactoryStore,
        request: PreviewPublishRequest<'_>,
    ) -> Result<()> {
        if request.intent.kind != ImplementationPublicationKind::Preview {
            return Err(SymphonyError::TriageError(format!(
                "preview publisher cannot reconcile kind={}",
                request.intent.kind.as_str()
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

        let body = render_preview_comment(ImplementationPreviewComment {
            intent_id: &request.intent.intent_id,
            run_id: request.run_id,
            stage_run_id: request.stage_run_id,
            artifact_id: &request.artifact.artifact_id,
            approved_version: request.artifact.approved_version,
            approved_spec_path: &request.artifact.approved_spec_path,
            base_commit: &request.artifact.base_commit,
            head_commit: request
                .artifact
                .head_commit
                .as_deref()
                .or(request.manifest.head_commit.as_deref())
                .unwrap_or(""),
            manifest: request.manifest,
            changed_paths: request.changed_paths,
            validation: request.validation,
        });

        self.upsert_marked_comment(
            store,
            request.intent,
            request.issue_number,
            &body,
            request.max_pages,
        )
        .await?;
        store
            .complete_implementation_publication(&request.intent.intent_id, PREVIEW_COMMENT_STEP)?;
        Ok(())
    }

    pub async fn publish_diagnostic(
        &self,
        store: &SharedFactoryStore,
        request: DiagnosticPublishRequest<'_>,
    ) -> Result<()> {
        if request.intent.kind != ImplementationPublicationKind::Diagnostic {
            return Err(SymphonyError::TriageError(format!(
                "diagnostic publisher cannot reconcile kind={}",
                request.intent.kind.as_str()
            )));
        }
        if request.intent.status == PublicationStatus::Applied
            && request
                .intent
                .completed_steps
                .iter()
                .any(|step| step == DIAGNOSTIC_COMMENT_STEP)
        {
            return Ok(());
        }

        let body =
            render_diagnostic_comment(&request.intent.intent_id, request.run_id, request.message);
        self.upsert_marked_comment(
            store,
            request.intent,
            request.issue_number,
            &body,
            request.max_pages,
        )
        .await?;
        store.complete_implementation_publication(
            &request.intent.intent_id,
            DIAGNOSTIC_COMMENT_STEP,
        )?;
        Ok(())
    }

    /// PR1: automatic publication is deferred — log and leave pending for PR2.
    pub fn defer_automatic_publication(
        &self,
        intent: &ImplementationPublicationIntent,
    ) -> Result<()> {
        if intent.kind != ImplementationPublicationKind::Automatic {
            return Err(SymphonyError::TriageError(
                "defer_automatic_publication requires automatic kind".to_string(),
            ));
        }
        tracing::info!(
            event = "implementation_automatic_publication_deferred",
            intent_id = %intent.intent_id,
            run_id = %intent.run_id,
            "automatic publication deferred to PR2; preview-only in PR1"
        );
        Ok(())
    }

    async fn upsert_marked_comment(
        &self,
        store: &SharedFactoryStore,
        intent: &ImplementationPublicationIntent,
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
                        store.bind_implementation_publication_comment(
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
                "implementation publication comment {comment_id} is not owned by publisher {publisher_login}"
            )));
        }

        self.comments.update_comment(comment_id, body).await?;
        if intent.comment_id.is_none()
            || intent.publisher_login.as_deref() != Some(&publisher_login)
        {
            store.bind_implementation_publication_comment(
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
            let Some(found_intent) = extract_implementation_intent_id(body) else {
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

pub fn extract_implementation_intent_id(body: &str) -> Option<String> {
    let start = body.find(IMPLEMENTATION_COMMENT_MARKER_PREFIX)?
        + IMPLEMENTATION_COMMENT_MARKER_PREFIX.len();
    let rest = &body[start..];
    let end = rest.find(IMPLEMENTATION_COMMENT_MARKER_SUFFIX)?;
    let intent_id = rest[..end].trim();
    (!intent_id.is_empty()).then(|| intent_id.to_string())
}

fn parse_comment_id(value: &str) -> Result<u64> {
    value.parse::<u64>().map_err(|error| {
        SymphonyError::TriageError(format!(
            "invalid implementation comment id {value}: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::{GithubIssueComment, GithubUser};
    use crate::implementation::domain::{
        AcceptanceCriterionClaim, CriterionStatus, EvidenceKind, ExecutionProfile,
        ImplementationEvidence, ManifestStatus,
    };
    use crate::spec::domain::{SpecArtifact, SpecPublicationKind};
    use crate::triage::store::{
        ClaimAttemptRequest, StoreImplementationArtifactRequest, StoreSpecArtifactRequest,
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
    }

    impl FakeComments {
        fn new(login: &str) -> Arc<Self> {
            Arc::new(Self {
                login: login.into(),
                comments: Mutex::new(HashMap::new()),
                next_id: Mutex::new(700),
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
                    message: "missing".into(),
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
            let mut comments = self.comments.lock().unwrap();
            let comment =
                comments
                    .get_mut(&comment_id)
                    .ok_or_else(|| SymphonyError::GithubApiStatus {
                        status: 404,
                        message: "missing".into(),
                    })?;
            comment.body = Some(body.to_string());
            Ok(comment.clone())
        }
    }

    fn open_store() -> (tempfile::TempDir, SharedFactoryStore) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factory.db");
        let store = SharedFactoryStore::open(&path, 5_000).unwrap();
        (dir, store)
    }

    async fn setup_preview_fixture(
        store: &SharedFactoryStore,
    ) -> (
        ImplementationPublicationIntent,
        ImplementationArtifactRecord,
        ImplementationManifest,
    ) {
        let spec_stage = store
            .claim_spec_attempt(ClaimAttemptRequest {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                issue_id: "42".into(),
                issue_identifier: "#42".into(),
                issue_revision: "rev".into(),
                configuration_revision: "spec-cfg".into(),
                owner_instance: "test".into(),
                harness: "pi".into(),
                model: None,
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })
            .unwrap();
        let approved = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: spec_stage.stage_run_id,
                issue_revision: "rev".into(),
                configuration_revision: "spec-cfg".into(),
                artifact: SpecArtifact {
                    schema_version: 1,
                    product_behavior: "Behavior".into(),
                    technical_approach: "Approach".into(),
                    acceptance_criteria: vec!["Done".into()],
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
                &serde_json::json!({"intake_label": "ready"}),
            )
            .unwrap();
        store
            .finalize_spec_approval(&approved.run_id, &approval.intent_id, "route_applied")
            .unwrap();

        let stage = store
            .claim_implementation_attempt(ClaimAttemptRequest {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                issue_id: "42".into(),
                issue_identifier: "#42".into(),
                issue_revision: "rev".into(),
                configuration_revision: "cfg".into(),
                owner_instance: "test".into(),
                harness: "pi".into(),
                model: None,
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
            })
            .unwrap();
        let manifest = ImplementationManifest {
            schema_version: 1,
            status: ManifestStatus::Completed,
            head_commit: Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904".into()),
            summary: "Adds retry.".into(),
            acceptance_criteria: vec![AcceptanceCriterionClaim {
                index: 1,
                status: CriterionStatus::Implemented,
                evidence: vec![ImplementationEvidence {
                    kind: EvidenceKind::Repository,
                    reference: "src/x.rs".into(),
                    summary: "bound".into(),
                }],
            }],
            known_limitations: vec![],
            blocker: None,
        };
        let artifact = store
            .store_implementation_artifact(StoreImplementationArtifactRequest {
                stage_run_id: stage.stage_run_id.clone(),
                approved_artifact_id: approved.artifact_id,
                approved_version: 1,
                issue_revision: "rev".into(),
                configuration_revision: "cfg".into(),
                manifest: manifest.clone(),
                base_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                head_commit: Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904".into()),
                approved_spec_path: "specs/KATA-42/APPROVED-v1.md".into(),
                validation_cycles: 1,
                execution_profile: ExecutionProfile::Local,
                bytes_len: 128,
                usage: Default::default(),
            })
            .unwrap();
        let intent = store
            .create_implementation_publication_intent(
                &stage.run_id,
                Some(&artifact.artifact_id),
                ImplementationPublicationKind::Preview,
                &serde_json::json!({"mode":"preview"}),
            )
            .unwrap();
        (intent, artifact, manifest)
    }

    #[tokio::test]
    async fn preview_create_before_record_and_idempotent_reconcile() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let publisher = ImplementationPublisher::new(comments.clone());
        let (intent, artifact, manifest) = setup_preview_fixture(&store).await;

        publisher
            .publish_preview(
                &store,
                PreviewPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    manifest: &manifest,
                    changed_paths: &["src/x.rs".into()],
                    validation: &[],
                    max_pages: 2,
                },
            )
            .await
            .unwrap();

        let bound = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert!(bound.comment_id.is_some());
        assert_eq!(bound.status, PublicationStatus::Applied);
        assert!(bound
            .completed_steps
            .iter()
            .any(|step| step == PREVIEW_COMMENT_STEP));

        let bodies: Vec<_> = comments
            .comments
            .lock()
            .unwrap()
            .values()
            .map(|c| c.body.clone().unwrap_or_default())
            .collect();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("<!-- symphony:implementation:"));
        assert!(bodies[0].contains("No remote branch or pull request was created"));

        // Idempotent re-publish updates the same comment.
        publisher
            .publish_preview(
                &store,
                PreviewPublishRequest {
                    intent: &bound,
                    issue_number: 42,
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    manifest: &manifest,
                    changed_paths: &["src/x.rs".into()],
                    validation: &[],
                    max_pages: 2,
                },
            )
            .await
            .unwrap();
        assert_eq!(comments.comments.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn spoofed_marker_is_ignored_during_recovery() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let (intent, artifact, manifest) = setup_preview_fixture(&store).await;

        // Spoofed comment from another author with our marker.
        comments.comments.lock().unwrap().insert(
            1,
            GithubIssueComment {
                id: 1,
                user: Some(GithubUser {
                    login: "attacker".into(),
                }),
                body: Some(format!(
                    "<!-- symphony:implementation:{} -->\nspoof",
                    intent.intent_id
                )),
                html_url: None,
                created_at: None,
                updated_at: None,
            },
        );

        let publisher = ImplementationPublisher::new(comments.clone());
        publisher
            .publish_preview(
                &store,
                PreviewPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    manifest: &manifest,
                    changed_paths: &[],
                    validation: &[],
                    max_pages: 2,
                },
            )
            .await
            .unwrap();

        let bound = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        // New owned comment, not the spoofed id=1.
        assert_ne!(bound.comment_id.as_deref(), Some("1"));
        let owned = comments
            .comments
            .lock()
            .unwrap()
            .get(&bound.comment_id.as_ref().unwrap().parse::<u64>().unwrap())
            .unwrap()
            .clone();
        assert_eq!(owned.user.unwrap().login, "symphony-bot");
    }

    #[tokio::test]
    async fn diagnostic_path_for_spec_gap() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let stage = store
            .claim_implementation_attempt(ClaimAttemptRequest {
                forge_host: "github.com".into(),
                repository: "acme/repo".into(),
                issue_id: "7".into(),
                issue_identifier: "#7".into(),
                issue_revision: "rev".into(),
                configuration_revision: "cfg".into(),
                owner_instance: "test".into(),
                harness: "pi".into(),
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
            .create_implementation_publication_intent(
                &stage.run_id,
                None,
                ImplementationPublicationKind::Diagnostic,
                &serde_json::json!({"kind":"spec_gap"}),
            )
            .unwrap();
        let publisher = ImplementationPublisher::new(comments.clone());
        publisher
            .publish_diagnostic(
                &store,
                DiagnosticPublishRequest {
                    intent: &intent,
                    issue_number: 7,
                    run_id: &stage.run_id,
                    message: "The approved spec does not define fork behavior.",
                    max_pages: 2,
                },
            )
            .await
            .unwrap();
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
        assert!(body.contains("Action required"));
        assert!(body.contains("fork behavior"));
    }

    #[test]
    fn extract_intent_id_from_marker() {
        assert_eq!(
            extract_implementation_intent_id("<!-- symphony:implementation:abc -->\nbody")
                .as_deref(),
            Some("abc")
        );
        assert_eq!(
            extract_implementation_intent_id("<!-- symphony:triage:x -->"),
            None
        );
    }
}
