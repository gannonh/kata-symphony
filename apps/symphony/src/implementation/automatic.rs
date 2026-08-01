//! Automatic draft-PR publication and Agent Review handoff (A3 PR2).

use crate::error::{Result, SymphonyError};
use crate::github::client::GithubPullRequest;
use crate::implementation::branch::{publish_branch, BranchPublishRequest, BRANCH_PUSHED_STEP};
use crate::implementation::bundle::artifacts_dir;
use crate::implementation::comment::{
    extract_implementation_pr_intent_id, pr_marker, render_draft_pr_body,
    render_publication_final_comment, render_publication_pending_comment,
    ImplementationDraftPrBody, ImplementationPublicationFinalComment,
    ImplementationPublicationPendingComment,
};
use crate::implementation::domain::{
    BundleArtifactRecord, DraftPrArtifactRecord, ImplementationArtifactRecord,
    ImplementationManifest, ImplementationPublicationIntent, ImplementationPublicationKind,
    ValidationCommandResult,
};
use crate::implementation::publisher::{
    ImplementationPublisher, COMMENT_FINAL_STEP, COMMENT_PENDING_STEP, PR_VERIFIED_STEP,
    ROUTE_APPLIED_STEP,
};
use crate::triage::domain::{FactoryError, PublicationStatus};
use crate::triage::publisher::{TriageCommentPort, TriageRoutingPort};
use crate::triage::runtime::SharedFactoryStore;
use crate::triage::store::StoreDraftPrArtifactRequest;
use async_trait::async_trait;
use std::path::Path;

/// Re-export step names used by coordinator/tests.
pub use crate::implementation::branch::BRANCH_PUSHED_STEP as BRANCH_STEP;
pub use crate::implementation::publisher::{
    COMMENT_FINAL_STEP as FINAL_STEP, COMMENT_PENDING_STEP as PENDING_STEP,
    PR_VERIFIED_STEP as PR_STEP, ROUTE_APPLIED_STEP as ROUTE_STEP,
};

#[derive(Debug, Clone)]
pub struct AutomaticPublishRequest<'a> {
    pub intent: &'a ImplementationPublicationIntent,
    pub issue_number: u64,
    pub issue_title: &'a str,
    pub current_issue_revision: &'a str,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub artifact: &'a ImplementationArtifactRecord,
    pub bundle: &'a BundleArtifactRecord,
    pub manifest: &'a ImplementationManifest,
    pub validation: &'a [ValidationCommandResult],
    pub branch: &'a str,
    pub base_branch: &'a str,
    pub remote_url: &'a str,
    pub github_token: Option<&'a str>,
    pub repo_owner: &'a str,
    pub approval_route_label: &'a str,
    pub completion_route_state: &'a str,
    pub storage_path: &'a Path,
    pub max_pages: u32,
}

#[async_trait]
pub trait ImplementationPullRequestPort: Send + Sync {
    async fn list_pull_requests(
        &self,
        state: &str,
        head: Option<&str>,
        base: Option<&str>,
        max_pages: u32,
    ) -> Result<Vec<GithubPullRequest>>;

    async fn create_pull_request(
        &self,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
        draft: bool,
    ) -> Result<GithubPullRequest>;
}

#[async_trait]
impl ImplementationPullRequestPort for crate::github::client::GithubClient {
    async fn list_pull_requests(
        &self,
        state: &str,
        head: Option<&str>,
        base: Option<&str>,
        max_pages: u32,
    ) -> Result<Vec<GithubPullRequest>> {
        crate::github::client::GithubClient::list_pull_requests(self, state, head, base, max_pages)
            .await
    }

    async fn create_pull_request(
        &self,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
        draft: bool,
    ) -> Result<GithubPullRequest> {
        crate::github::client::GithubClient::create_pull_request(
            self, title, body, head, base, draft,
        )
        .await
    }
}

#[async_trait]
impl ImplementationPullRequestPort for std::sync::Arc<dyn ImplementationPullRequestPort> {
    async fn list_pull_requests(
        &self,
        state: &str,
        head: Option<&str>,
        base: Option<&str>,
        max_pages: u32,
    ) -> Result<Vec<GithubPullRequest>> {
        (**self)
            .list_pull_requests(state, head, base, max_pages)
            .await
    }

    async fn create_pull_request(
        &self,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
        draft: bool,
    ) -> Result<GithubPullRequest> {
        (**self)
            .create_pull_request(title, body, head, base, draft)
            .await
    }
}

pub struct AutomaticImplementationPublisher<C, R, P> {
    comments: C,
    routing: R,
    pulls: P,
}

impl<C, R, P> AutomaticImplementationPublisher<C, R, P>
where
    C: TriageCommentPort + Clone,
    R: TriageRoutingPort,
    P: ImplementationPullRequestPort,
{
    pub fn new(comments: C, routing: R, pulls: P) -> Self {
        Self {
            comments,
            routing,
            pulls,
        }
    }

    pub async fn publish_automatic(
        &self,
        store: &SharedFactoryStore,
        request: AutomaticPublishRequest<'_>,
    ) -> Result<DraftPrArtifactRecord> {
        if request.intent.kind != ImplementationPublicationKind::Automatic {
            return Err(SymphonyError::TriageError(format!(
                "automatic publisher cannot reconcile kind={}",
                request.intent.kind.as_str()
            )));
        }
        if request.intent.status == PublicationStatus::Applied {
            if let Some(existing) = store.get_draft_pr_for_intent(&request.intent.intent_id)? {
                return Ok(existing);
            }
            return Err(SymphonyError::TriageError(
                "automatic publication is applied but draft PR artifact is missing".into(),
            ));
        }
        if matches!(
            request.intent.status,
            PublicationStatus::Conflict | PublicationStatus::Blocked
        ) {
            return Err(SymphonyError::TriageError(format!(
                "automatic publication is terminal status={}",
                request.intent.status.as_str()
            )));
        }

        if request.current_issue_revision != request.artifact.issue_revision {
            let error = FactoryError::new(
                "issue_revision_drift",
                "implementation_publication",
                format!(
                    "issue revision drifted: approved={} current={}",
                    request.artifact.issue_revision, request.current_issue_revision
                ),
                true,
                None,
            );
            // Drift clears only when the issue returns to the approved revision,
            // which waits on a human. Recording it as a waiting condition keeps it
            // off the retry budget so a slow re-approval cannot terminalize the
            // intent as `blocked`.
            store.set_implementation_publication_waiting(
                &request.intent.intent_id,
                PublicationStatus::Pending,
                error.clone(),
            )?;
            return Err(SymphonyError::TriageError(error.remediation));
        }

        let comment_publisher = ImplementationPublisher::new(self.comments.clone());
        let mut intent = store
            .get_implementation_publication_intent(&request.intent.intent_id)?
            .ok_or_else(|| {
                SymphonyError::TriageError(format!(
                    "implementation publication intent {} missing",
                    request.intent.intent_id
                ))
            })?;

        if !intent
            .completed_steps
            .iter()
            .any(|s| s == COMMENT_PENDING_STEP)
        {
            let pending =
                render_publication_pending_comment(ImplementationPublicationPendingComment {
                    intent_id: &intent.intent_id,
                    run_id: request.run_id,
                    stage_run_id: request.stage_run_id,
                    artifact_id: &request.artifact.artifact_id,
                });
            comment_publisher
                .upsert_marked_comment(
                    store,
                    &intent,
                    request.issue_number,
                    &pending,
                    request.max_pages,
                )
                .await?;
            let projection = serde_json::json!({
                "step": COMMENT_PENDING_STEP,
                "branch": request.branch,
                "desired_head": request.bundle.head_commit,
                "bundle_sha256": request.bundle.sha256,
            });
            store.record_implementation_publication_step(
                &intent.intent_id,
                COMMENT_PENDING_STEP,
                PublicationStatus::Pending,
                &projection,
            )?;
            intent = reload_intent(store, &intent.intent_id)?;
        }

        let branch = request.branch.to_string();
        let desired_head = request.bundle.head_commit.clone();
        let expected_remote = intent
            .expected_projection
            .get("remote_sha")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        if !intent
            .completed_steps
            .iter()
            .any(|s| s == BRANCH_PUSHED_STEP)
        {
            let arts = artifacts_dir(request.storage_path);
            let expected = expected_remote.clone();
            let remote_url = request.remote_url.to_string();
            let github_token = request.github_token.map(str::to_string);
            let branch_owned = branch.clone();
            let base_branch = request.base_branch.to_string();
            let sha = request.bundle.sha256.clone();
            let bytes = request.bundle.bytes_len;
            let desired = desired_head.clone();
            let base_commit = request.bundle.base_commit.clone();
            let intent_id = intent.intent_id.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                publish_branch(&BranchPublishRequest {
                    artifacts_dir: &arts,
                    bundle_sha256: &sha,
                    bundle_bytes_len: bytes,
                    base_commit: &base_commit,
                    desired_head: &desired,
                    expected_remote_sha: expected.as_deref(),
                    branch_name: &branch_owned,
                    base_branch: &base_branch,
                    remote_url: &remote_url,
                    github_token: github_token.as_deref(),
                })
            })
            .await
            .map_err(|error| {
                SymphonyError::TriageError(format!("branch publish join failed: {error}"))
            })?;

            let outcome = match outcome {
                Ok(value) => value,
                Err(error) => {
                    let message = error.to_string();
                    if message.contains("base commit conflict") {
                        let _ = store.set_implementation_publication_error(
                            &intent_id,
                            PublicationStatus::Conflict,
                            FactoryError::new(
                                "base_commit_unreachable",
                                "implementation_publication",
                                message.clone(),
                                false,
                                None,
                            ),
                        );
                    } else if message.contains("conflict") {
                        let _ = store.set_implementation_publication_error(
                            &intent_id,
                            PublicationStatus::Conflict,
                            FactoryError::new(
                                "branch_conflict",
                                "implementation_publication",
                                message.clone(),
                                false,
                                None,
                            ),
                        );
                    }
                    return Err(error);
                }
            };

            let projection = serde_json::json!({
                "step": BRANCH_PUSHED_STEP,
                "branch": branch,
                "desired_head": desired_head,
                "remote_sha": outcome.remote_sha_after,
                "pushed": outcome.pushed,
                "bundle_sha256": request.bundle.sha256,
            });
            store.record_implementation_publication_step(
                &intent.intent_id,
                BRANCH_PUSHED_STEP,
                PublicationStatus::Pending,
                &projection,
            )?;
            intent = reload_intent(store, &intent.intent_id)?;
        }

        let pr_verified = intent
            .completed_steps
            .iter()
            .any(|step| step == PR_VERIFIED_STEP);
        let publisher_login = self.comments.authenticated_login().await?;
        let draft = if let Some(existing) =
            store.get_draft_pr_for_implementation_artifact(&request.artifact.artifact_id)?
        {
            self.reverify_persisted_draft_pr(store, &intent, &request, &existing, &publisher_login)
                .await?;
            if !pr_verified {
                let projection = serde_json::json!({
                    "step": PR_VERIFIED_STEP,
                    "pr_number": existing.number,
                    "pr_url": existing.url,
                    "head": existing.head,
                    "base": existing.base,
                    "head_sha": existing.head_sha,
                });
                store.record_implementation_publication_step(
                    &intent.intent_id,
                    PR_VERIFIED_STEP,
                    PublicationStatus::Pending,
                    &projection,
                )?;
                intent = reload_intent(store, &intent.intent_id)?;
            }
            existing
        } else {
            let pr = self
                .reconcile_draft_pr(
                    store,
                    &intent,
                    &request,
                    &branch,
                    &desired_head,
                    &publisher_login,
                )
                .await?;
            let record = store.store_draft_pr_artifact(StoreDraftPrArtifactRequest {
                run_id: request.run_id.to_string(),
                implementation_artifact_id: request.artifact.artifact_id.clone(),
                intent_id: intent.intent_id.clone(),
                number: pr.number,
                url: pr.html_url.clone(),
                draft: pr.draft,
                head: pr.head.ref_name.clone(),
                base: pr.base.ref_name.clone(),
                head_sha: pr.head.sha.clone(),
                marker: pr_marker(&intent.intent_id),
            })?;
            let projection = serde_json::json!({
                "step": PR_VERIFIED_STEP,
                "pr_number": pr.number,
                "pr_url": pr.html_url,
                "head": pr.head.ref_name,
                "base": pr.base.ref_name,
                "head_sha": pr.head.sha,
            });
            store.record_implementation_publication_step(
                &intent.intent_id,
                PR_VERIFIED_STEP,
                PublicationStatus::Pending,
                &projection,
            )?;
            intent = reload_intent(store, &intent.intent_id)?;
            record
        };

        // Tracker state never advances before the verified draft-PR artifact exists.
        if store
            .get_draft_pr_for_implementation_artifact(&request.artifact.artifact_id)?
            .is_none()
        {
            return Err(SymphonyError::TriageError(
                "refusing tracker handoff without draft-PR artifact".into(),
            ));
        }

        if !intent
            .completed_steps
            .iter()
            .any(|s| s == ROUTE_APPLIED_STEP)
        {
            let labels = self.routing.list_issue_labels(request.issue_number).await?;
            if labels
                .iter()
                .any(|label| label.eq_ignore_ascii_case(request.approval_route_label))
            {
                self.routing
                    .remove_issue_label(request.issue_number, request.approval_route_label)
                    .await?;
            }
            self.routing
                .set_issue_project_state(request.issue_number, request.completion_route_state)
                .await?;
            let projection = serde_json::json!({
                "step": ROUTE_APPLIED_STEP,
                "removed_label": request.approval_route_label,
                "completion_state": request.completion_route_state,
                "pr_number": draft.number,
            });
            store.record_implementation_publication_step(
                &intent.intent_id,
                ROUTE_APPLIED_STEP,
                PublicationStatus::Pending,
                &projection,
            )?;
            intent = reload_intent(store, &intent.intent_id)?;
        }

        if !intent
            .completed_steps
            .iter()
            .any(|s| s == COMMENT_FINAL_STEP)
        {
            let final_body =
                render_publication_final_comment(ImplementationPublicationFinalComment {
                    intent_id: &intent.intent_id,
                    run_id: request.run_id,
                    stage_run_id: request.stage_run_id,
                    artifact_id: &request.artifact.artifact_id,
                    pr_number: draft.number,
                    pr_url: &draft.url,
                    branch: &branch,
                    head_commit: &desired_head,
                });
            comment_publisher
                .upsert_marked_comment(
                    store,
                    &intent,
                    request.issue_number,
                    &final_body,
                    request.max_pages,
                )
                .await?;
            let projection = serde_json::json!({
                "step": COMMENT_FINAL_STEP,
                "pr_number": draft.number,
                "pr_url": draft.url,
            });
            store.record_implementation_publication_step(
                &intent.intent_id,
                COMMENT_FINAL_STEP,
                PublicationStatus::Pending,
                &projection,
            )?;
        }

        Ok(draft)
    }

    async fn reconcile_draft_pr(
        &self,
        store: &SharedFactoryStore,
        intent: &ImplementationPublicationIntent,
        request: &AutomaticPublishRequest<'_>,
        branch: &str,
        desired_head: &str,
        publisher_login: &str,
    ) -> Result<GithubPullRequest> {
        let head_filter = format!("{}:{}", request.repo_owner, branch);
        let candidates = self
            .pulls
            .list_pull_requests(
                "all",
                Some(&head_filter),
                Some(request.base_branch),
                request.max_pages,
            )
            .await?;

        let mut owned: Vec<GithubPullRequest> = Vec::new();
        let mut closed_owned: Vec<GithubPullRequest> = Vec::new();
        let mut foreign_open = false;
        for pr in candidates {
            let marker_match = pr
                .body
                .as_deref()
                .and_then(extract_implementation_pr_intent_id)
                .as_deref()
                == Some(intent.intent_id.as_str());
            if marker_match {
                if pr.state == "open" {
                    owned.push(pr);
                } else {
                    closed_owned.push(pr);
                }
            } else if pr.state == "open" {
                foreign_open = true;
            }
        }

        if let Some(pr) = closed_owned.first() {
            let error = FactoryError::new(
                "closed_owned_pr",
                "implementation_publication",
                format!(
                    "owned PR #{} is {}, refusing to replace maintainer divergence",
                    pr.number, pr.state
                ),
                false,
                None,
            );
            store.set_implementation_publication_error(
                &intent.intent_id,
                PublicationStatus::Conflict,
                error.clone(),
            )?;
            return Err(SymphonyError::TriageError(error.remediation));
        }
        if foreign_open && owned.is_empty() {
            let error = FactoryError::new(
                "foreign_pr",
                "implementation_publication",
                format!("open PR on {branch} lacks ownership marker"),
                false,
                None,
            );
            store.set_implementation_publication_error(
                &intent.intent_id,
                PublicationStatus::Conflict,
                error.clone(),
            )?;
            return Err(SymphonyError::TriageError(error.remediation));
        }
        if owned.len() > 1 {
            let error = FactoryError::new(
                "multiple_prs",
                "implementation_publication",
                format!("multiple owned PRs for intent {}", intent.intent_id),
                false,
                None,
            );
            store.set_implementation_publication_error(
                &intent.intent_id,
                PublicationStatus::Conflict,
                error.clone(),
            )?;
            return Err(SymphonyError::TriageError(error.remediation));
        }
        if let Some(pr) = owned.into_iter().next() {
            return verify_owned_draft_pr(
                &pr,
                intent,
                branch,
                request.base_branch,
                desired_head,
                publisher_login,
            )
            .inspect_err(|error| {
                let _ = store.set_implementation_publication_error(
                    &intent.intent_id,
                    PublicationStatus::Conflict,
                    FactoryError::new(
                        "pr_drift",
                        "implementation_publication",
                        error.to_string(),
                        false,
                        None,
                    ),
                );
            });
        }

        let title = format!(
            "Symphony implementation: {}",
            truncate_title(request.issue_title, 72)
        );
        let body = render_draft_pr_body(ImplementationDraftPrBody {
            intent_id: &intent.intent_id,
            issue_number: request.issue_number,
            run_id: request.run_id,
            stage_run_id: request.stage_run_id,
            artifact_id: &request.artifact.artifact_id,
            bundle_artifact_id: &request.bundle.artifact_id,
            approved_artifact_id: &request.artifact.approved_artifact_id,
            approved_version: request.artifact.approved_version,
            approved_spec_path: &request.artifact.approved_spec_path,
            base_commit: &request.bundle.base_commit,
            head_commit: desired_head,
            manifest: request.manifest,
            validation: request.validation,
        });
        let created = self
            .pulls
            .create_pull_request(&title, &body, branch, request.base_branch, true)
            .await?;
        verify_owned_draft_pr(
            &created,
            intent,
            branch,
            request.base_branch,
            desired_head,
            publisher_login,
        )?;
        Ok(created)
    }

    async fn reverify_persisted_draft_pr(
        &self,
        store: &SharedFactoryStore,
        intent: &ImplementationPublicationIntent,
        request: &AutomaticPublishRequest<'_>,
        persisted: &DraftPrArtifactRecord,
        publisher_login: &str,
    ) -> Result<()> {
        let head_filter = format!("{}:{}", request.repo_owner, request.branch);
        let live = self
            .pulls
            .list_pull_requests(
                "all",
                Some(&head_filter),
                Some(request.base_branch),
                request.max_pages,
            )
            .await?
            .into_iter()
            .find(|pr| pr.number == persisted.number);

        let Some(live) = live.as_ref() else {
            let error = FactoryError::new(
                "pr_not_visible",
                "implementation_publication",
                format!(
                    "persisted draft PR #{} is not visible in the bounded head/base listing",
                    persisted.number
                ),
                true,
                Some(PR_VERIFIED_STEP.into()),
            );
            store.set_implementation_publication_error(
                &intent.intent_id,
                PublicationStatus::Pending,
                error.clone(),
            )?;
            return Err(SymphonyError::TriageError(error.remediation));
        };

        let verification = verify_owned_draft_pr(
            live,
            intent,
            request.branch,
            request.base_branch,
            &request.bundle.head_commit,
            publisher_login,
        )
        .and_then(|pr| verify_persisted_draft_pr_record(persisted, &pr));

        if let Err(error) = verification {
            store.set_implementation_publication_error(
                &intent.intent_id,
                PublicationStatus::Conflict,
                FactoryError::new(
                    "pr_drift",
                    "implementation_publication",
                    error.to_string(),
                    false,
                    None,
                ),
            )?;
            return Err(error);
        }
        Ok(())
    }
}

fn reload_intent(
    store: &SharedFactoryStore,
    intent_id: &str,
) -> Result<ImplementationPublicationIntent> {
    store
        .get_implementation_publication_intent(intent_id)?
        .ok_or_else(|| {
            SymphonyError::TriageError(format!(
                "implementation publication intent {intent_id} missing"
            ))
        })
}

fn verify_owned_draft_pr(
    pr: &GithubPullRequest,
    intent: &ImplementationPublicationIntent,
    branch: &str,
    base_branch: &str,
    desired_head: &str,
    publisher_login: &str,
) -> Result<GithubPullRequest> {
    let marker = pr
        .body
        .as_deref()
        .and_then(extract_implementation_pr_intent_id);
    if marker.as_deref() != Some(intent.intent_id.as_str()) {
        return Err(SymphonyError::TriageError(
            "draft PR missing ownership marker".into(),
        ));
    }
    let author = pr
        .user
        .as_ref()
        .map(|user| user.login.as_str())
        .unwrap_or("");
    if !author.eq_ignore_ascii_case(publisher_login) {
        return Err(SymphonyError::TriageError(format!(
            "owned PR #{} author {author:?} does not match authenticated publisher {publisher_login}",
            pr.number
        )));
    }
    if pr.state != "open" {
        return Err(SymphonyError::TriageError(format!(
            "owned PR #{} is {}, not open",
            pr.number, pr.state
        )));
    }
    if !pr.draft {
        return Err(SymphonyError::TriageError(format!(
            "owned PR #{} is ready-for-review, not draft",
            pr.number
        )));
    }
    if pr.head.ref_name != branch || pr.base.ref_name != base_branch {
        return Err(SymphonyError::TriageError(format!(
            "owned PR #{} head/base drift: {}/{} vs {}/{}",
            pr.number, pr.head.ref_name, pr.base.ref_name, branch, base_branch
        )));
    }
    if pr.head.sha != desired_head {
        return Err(SymphonyError::TriageError(format!(
            "owned PR #{} head SHA drift: {} vs {desired_head}",
            pr.number, pr.head.sha
        )));
    }
    Ok(pr.clone())
}

fn verify_persisted_draft_pr_record(
    persisted: &DraftPrArtifactRecord,
    live: &GithubPullRequest,
) -> Result<GithubPullRequest> {
    if persisted.number != live.number
        || persisted.url != live.html_url
        || persisted.draft != live.draft
        || persisted.head != live.head.ref_name
        || persisted.base != live.base.ref_name
        || persisted.head_sha != live.head.sha
    {
        return Err(SymphonyError::TriageError(format!(
            "persisted draft PR artifact for #{} does not match live GitHub state",
            persisted.number
        )));
    }
    Ok(live.clone())
}

fn truncate_title(title: &str, max_chars: usize) -> String {
    let trimmed = title.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn resolve_publication_remote(
    desired_effects: &serde_json::Value,
    forge_host: &str,
    repo_owner: &str,
    repo_name: &str,
) -> Result<String> {
    let host = forge_host.trim().trim_end_matches('/');
    let owner = repo_owner.trim().trim_matches('/');
    let repo = repo_name.trim().trim_matches('/');
    if host.is_empty() || owner.is_empty() || repo.is_empty() {
        return Err(SymphonyError::TriageError(
            "automatic publication requires a pinned forge host, repo owner, and repo name".into(),
        ));
    }
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let canonical = format!("https://{host}/{owner}/{repo}.git");

    if let Some(url) = desired_effects
        .get("remote_url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        if crate::repo_url::repo_is_remote(url) {
            let parsed = reqwest::Url::parse(url).map_err(|_| {
                SymphonyError::TriageError(
                    "automatic publication remote_url must be a valid HTTPS URL".into(),
                )
            })?;
            if parsed.scheme() != "https" {
                return Err(SymphonyError::TriageError(
                    "automatic publication remote_url must use HTTPS".into(),
                ));
            }
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(SymphonyError::TriageError(
                    "automatic publication remote_url must not contain credentials".into(),
                ));
            }
            let remote_repo = parsed
                .path()
                .trim_matches('/')
                .strip_suffix(".git")
                .unwrap_or_else(|| parsed.path().trim_matches('/'));
            let expected_repo = format!("{owner}/{repo}");
            if parsed
                .host_str()
                .is_none_or(|remote_host| !remote_host.eq_ignore_ascii_case(host))
                || remote_repo != expected_repo
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(SymphonyError::TriageError(
                    "automatic publication remote_url does not match the pinned forge repository"
                        .into(),
                ));
            }
            return Ok(canonical);
        }
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::{GithubIssueComment, GithubPullRequestRef, GithubUser};
    use crate::implementation::domain::{
        AcceptanceCriterionClaim, CriterionStatus, EvidenceKind, ExecutionProfile,
        ImplementationEvidence, ManifestStatus,
    };
    use crate::spec::domain::{SpecArtifact, SpecPublicationKind};
    use crate::triage::store::{
        ClaimAttemptRequest, StoreBundleArtifactRequest, StoreImplementationArtifactRequest,
        StoreSpecArtifactRequest,
    };
    use chrono::Utc;
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
            values.sort_by_key(|c| c.id);
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

    struct FakeRouting {
        labels: Mutex<Vec<String>>,
        state: Mutex<Option<String>>,
        label_removes: Mutex<Vec<String>>,
        draft_pr_gate: Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    }

    impl FakeRouting {
        fn new(labels: Vec<String>) -> Arc<Self> {
            Arc::new(Self {
                labels: Mutex::new(labels),
                state: Mutex::new(None),
                label_removes: Mutex::new(Vec::new()),
                draft_pr_gate: Mutex::new(None),
            })
        }
    }

    #[async_trait]
    impl TriageRoutingPort for Arc<FakeRouting> {
        async fn list_issue_labels(&self, _issue_number: u64) -> Result<Vec<String>> {
            Ok(self.labels.lock().unwrap().clone())
        }
        async fn issue_project_state(&self, _issue_number: u64) -> Result<Option<String>> {
            Ok(self.state.lock().unwrap().clone())
        }
        async fn add_issue_label(&self, _issue_number: u64, label: &str) -> Result<()> {
            self.labels.lock().unwrap().push(label.to_string());
            Ok(())
        }
        async fn remove_issue_label(&self, _issue_number: u64, label: &str) -> Result<()> {
            if let Some(gate) = self.draft_pr_gate.lock().unwrap().as_ref() {
                assert!(gate(), "label removed before draft-PR artifact existed");
            }
            self.label_removes.lock().unwrap().push(label.to_string());
            self.labels
                .lock()
                .unwrap()
                .retain(|existing| !existing.eq_ignore_ascii_case(label));
            Ok(())
        }
        async fn set_issue_project_state(&self, _issue_number: u64, state: &str) -> Result<()> {
            if let Some(gate) = self.draft_pr_gate.lock().unwrap().as_ref() {
                assert!(gate(), "state changed before draft-PR artifact existed");
            }
            *self.state.lock().unwrap() = Some(state.to_string());
            Ok(())
        }
    }

    struct FakePulls {
        prs: Mutex<Vec<GithubPullRequest>>,
        next_number: Mutex<u64>,
        create_calls: Mutex<u32>,
    }

    impl FakePulls {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                prs: Mutex::new(Vec::new()),
                next_number: Mutex::new(100),
                create_calls: Mutex::new(0),
            })
        }
    }

    #[async_trait]
    impl ImplementationPullRequestPort for Arc<FakePulls> {
        async fn list_pull_requests(
            &self,
            _state: &str,
            head: Option<&str>,
            base: Option<&str>,
            _max_pages: u32,
        ) -> Result<Vec<GithubPullRequest>> {
            let head_branch = head.and_then(|h| h.split(':').nth(1));
            Ok(self
                .prs
                .lock()
                .unwrap()
                .iter()
                .filter(|pr| {
                    head_branch.is_none_or(|b| pr.head.ref_name == b)
                        && base.is_none_or(|b| pr.base.ref_name == b)
                })
                .cloned()
                .collect())
        }

        async fn create_pull_request(
            &self,
            title: &str,
            body: &str,
            head: &str,
            base: &str,
            draft: bool,
        ) -> Result<GithubPullRequest> {
            *self.create_calls.lock().unwrap() += 1;
            let mut next = self.next_number.lock().unwrap();
            let number = *next;
            *next += 1;
            let head_sha = body
                .split("→ `")
                .nth(1)
                .and_then(|rest| rest.split('`').next())
                .unwrap_or("deadbeef")
                .to_string();
            let pr = GithubPullRequest {
                number,
                html_url: format!("https://github.com/acme/repo/pull/{number}"),
                draft,
                state: "open".into(),
                title: title.to_string(),
                head: GithubPullRequestRef {
                    ref_name: head.to_string(),
                    sha: head_sha,
                },
                base: GithubPullRequestRef {
                    ref_name: base.to_string(),
                    sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                },
                user: Some(GithubUser {
                    login: "symphony-bot".into(),
                }),
                body: Some(body.to_string()),
            };
            self.prs.lock().unwrap().push(pr.clone());
            Ok(pr)
        }
    }

    fn open_store() -> (tempfile::TempDir, SharedFactoryStore) {
        let dir = tempdir().unwrap();
        let store = SharedFactoryStore::open(&dir.path().join("factory.db"), 5_000).unwrap();
        (dir, store)
    }

    async fn setup_automatic_fixture(
        store: &SharedFactoryStore,
    ) -> (
        ImplementationPublicationIntent,
        ImplementationArtifactRecord,
        BundleArtifactRecord,
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
                &serde_json::json!({"route_label": "ready-for-agent"}),
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
        let head = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
        let manifest = ImplementationManifest {
            schema_version: 1,
            status: ManifestStatus::Completed,
            head_commit: Some(head.into()),
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
                head_commit: Some(head.into()),
                approved_spec_path: "specs/KATA-42/APPROVED-v1.md".into(),
                validation_cycles: 1,
                execution_profile: ExecutionProfile::Local,
                bytes_len: 128,
                usage: Default::default(),
            })
            .unwrap();
        let bundle = store
            .store_bundle_artifact(StoreBundleArtifactRequest {
                stage_run_id: stage.stage_run_id,
                implementation_artifact_id: artifact.artifact_id.clone(),
                base_commit: artifact.base_commit.clone(),
                head_commit: head.into(),
                tree_sha: "tree".into(),
                sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                bytes_len: 10,
                changed_file_count: 1,
                changed_paths: vec!["src/x.rs".into()],
                approved_spec_path: artifact.approved_spec_path.clone(),
                approved_spec_blob_sha: "cccc".into(),
                harness: "pi".into(),
                model: None,
                execution_profile: ExecutionProfile::Local,
                created_at: Utc::now(),
                verified_at: Utc::now(),
            })
            .unwrap();
        let intent = store
            .create_implementation_publication_intent(
                &artifact.run_id,
                Some(&artifact.artifact_id),
                ImplementationPublicationKind::Automatic,
                &serde_json::json!({
                    "mode": "automatic",
                    "approval_route_label": "ready-for-agent",
                    "completion_route_state": "Agent Review",
                }),
            )
            .unwrap();
        (intent, artifact, bundle, manifest)
    }

    fn skip_branch(store: &SharedFactoryStore, intent_id: &str, head: &str) {
        store
            .record_implementation_publication_step(
                intent_id,
                COMMENT_PENDING_STEP,
                PublicationStatus::Pending,
                &serde_json::json!({}),
            )
            .unwrap();
        store
            .record_implementation_publication_step(
                intent_id,
                BRANCH_PUSHED_STEP,
                PublicationStatus::Pending,
                &serde_json::json!({"remote_sha": head}),
            )
            .unwrap();
    }

    #[tokio::test]
    async fn foreign_pr_rejected_then_create_and_route_after_draft_artifact() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(vec!["ready-for-agent".into()]);
        let pulls = FakePulls::new();
        let (intent, artifact, bundle, manifest) = setup_automatic_fixture(&store).await;
        skip_branch(&store, &intent.intent_id, &bundle.head_commit);

        pulls.prs.lock().unwrap().push(GithubPullRequest {
            number: 1,
            html_url: "https://github.com/acme/repo/pull/1".into(),
            draft: true,
            state: "open".into(),
            title: "human".into(),
            head: GithubPullRequestRef {
                ref_name: "symphony/_42".into(),
                sha: bundle.head_commit.clone(),
            },
            base: GithubPullRequestRef {
                ref_name: "main".into(),
                sha: bundle.base_commit.clone(),
            },
            user: Some(GithubUser {
                login: "human".into(),
            }),
            body: Some("no marker".into()),
        });

        let intent = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        let publisher =
            AutomaticImplementationPublisher::new(comments.clone(), routing.clone(), pulls.clone());
        let err = publisher
            .publish_automatic(
                &store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    issue_title: "Retry",
                    current_issue_revision: "rev",
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &manifest,
                    validation: &[],
                    branch: "symphony/_42",
                    base_branch: "main",
                    remote_url: "/tmp/unused",
                    github_token: None,
                    repo_owner: "acme",
                    approval_route_label: "ready-for-agent",
                    completion_route_state: "Agent Review",
                    storage_path: Path::new("/tmp/unused-db"),
                    max_pages: 2,
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("ownership marker"));

        pulls.prs.lock().unwrap().clear();
        store
            .record_implementation_publication_step(
                &intent.intent_id,
                BRANCH_PUSHED_STEP,
                PublicationStatus::Pending,
                &serde_json::json!({"remote_sha": bundle.head_commit}),
            )
            .unwrap();

        let store_gate = store.clone();
        let artifact_id = artifact.artifact_id.clone();
        *routing.draft_pr_gate.lock().unwrap() = Some(Arc::new(move || {
            store_gate
                .get_draft_pr_for_implementation_artifact(&artifact_id)
                .ok()
                .flatten()
                .is_some()
        }));

        let intent = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        let draft = publisher
            .publish_automatic(
                &store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    issue_title: "Retry",
                    current_issue_revision: "rev",
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &manifest,
                    validation: &[],
                    branch: "symphony/_42",
                    base_branch: "main",
                    remote_url: "/tmp/unused",
                    github_token: None,
                    repo_owner: "acme",
                    approval_route_label: "ready-for-agent",
                    completion_route_state: "Agent Review",
                    storage_path: Path::new("/tmp/unused-db"),
                    max_pages: 2,
                },
            )
            .await
            .unwrap();
        assert!(draft.draft);
        assert_eq!(*pulls.create_calls.lock().unwrap(), 1);
        assert!(routing
            .label_removes
            .lock()
            .unwrap()
            .iter()
            .any(|l| l == "ready-for-agent"));
        assert_eq!(
            routing.state.lock().unwrap().as_deref(),
            Some("Agent Review")
        );
        let final_intent = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(final_intent.status, PublicationStatus::Pending);
        assert!(final_intent
            .completed_steps
            .iter()
            .any(|step| step == COMMENT_FINAL_STEP));
    }

    #[tokio::test]
    async fn create_before_record_recovers_owned_draft_pr() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(vec!["ready-for-agent".into()]);
        let pulls = FakePulls::new();
        let (intent, artifact, bundle, manifest) = setup_automatic_fixture(&store).await;
        skip_branch(&store, &intent.intent_id, &bundle.head_commit);

        let body = render_draft_pr_body(ImplementationDraftPrBody {
            intent_id: &intent.intent_id,
            issue_number: 42,
            run_id: &artifact.run_id,
            stage_run_id: &artifact.stage_run_id,
            artifact_id: &artifact.artifact_id,
            bundle_artifact_id: &bundle.artifact_id,
            approved_artifact_id: &artifact.approved_artifact_id,
            approved_version: 1,
            approved_spec_path: &artifact.approved_spec_path,
            base_commit: &bundle.base_commit,
            head_commit: &bundle.head_commit,
            manifest: &manifest,
            validation: &[],
        });
        pulls.prs.lock().unwrap().push(GithubPullRequest {
            number: 55,
            html_url: "https://github.com/acme/repo/pull/55".into(),
            draft: true,
            state: "open".into(),
            title: "Symphony implementation: Retry".into(),
            head: GithubPullRequestRef {
                ref_name: "symphony/_42".into(),
                sha: bundle.head_commit.clone(),
            },
            base: GithubPullRequestRef {
                ref_name: "main".into(),
                sha: bundle.base_commit.clone(),
            },
            user: Some(GithubUser {
                login: "symphony-bot".into(),
            }),
            body: Some(body),
        });

        let intent = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        let publisher =
            AutomaticImplementationPublisher::new(comments, routing.clone(), pulls.clone());
        let draft = publisher
            .publish_automatic(
                &store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    issue_title: "Retry",
                    current_issue_revision: "rev",
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &manifest,
                    validation: &[],
                    branch: "symphony/_42",
                    base_branch: "main",
                    remote_url: "/tmp/unused",
                    github_token: None,
                    repo_owner: "acme",
                    approval_route_label: "ready-for-agent",
                    completion_route_state: "Agent Review",
                    storage_path: Path::new("/tmp/unused-db"),
                    max_pages: 2,
                },
            )
            .await
            .unwrap();
        assert_eq!(draft.number, 55);
        assert_eq!(*pulls.create_calls.lock().unwrap(), 0);
        let final_intent = store
            .get_implementation_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(final_intent.status, PublicationStatus::Pending);
        assert!(final_intent
            .completed_steps
            .iter()
            .any(|step| step == COMMENT_FINAL_STEP));
    }

    #[tokio::test]
    async fn copied_marker_from_another_author_is_rejected() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(vec!["ready-for-agent".into()]);
        let pulls = FakePulls::new();
        let (intent, artifact, bundle, manifest) = setup_automatic_fixture(&store).await;
        skip_branch(&store, &intent.intent_id, &bundle.head_commit);

        pulls.prs.lock().unwrap().push(GithubPullRequest {
            number: 55,
            html_url: "https://github.com/acme/repo/pull/55".into(),
            draft: true,
            state: "open".into(),
            title: "copied marker".into(),
            head: GithubPullRequestRef {
                ref_name: "symphony/_42".into(),
                sha: bundle.head_commit.clone(),
            },
            base: GithubPullRequestRef {
                ref_name: "main".into(),
                sha: bundle.base_commit.clone(),
            },
            user: Some(GithubUser {
                login: "human".into(),
            }),
            body: Some(format!("{}\n", pr_marker(&intent.intent_id))),
        });

        let intent = reload_intent(&store, &intent.intent_id).unwrap();
        let publisher = AutomaticImplementationPublisher::new(comments, routing, pulls.clone());
        let err = publisher
            .publish_automatic(
                &store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    issue_title: "Retry",
                    current_issue_revision: "rev",
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &manifest,
                    validation: &[],
                    branch: "symphony/_42",
                    base_branch: "main",
                    remote_url: "/tmp/unused",
                    github_token: None,
                    repo_owner: "acme",
                    approval_route_label: "ready-for-agent",
                    completion_route_state: "Agent Review",
                    storage_path: Path::new("/tmp/unused-db"),
                    max_pages: 2,
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("authenticated publisher"));
        assert_eq!(*pulls.create_calls.lock().unwrap(), 0);
        assert_eq!(
            reload_intent(&store, &intent.intent_id).unwrap().status,
            PublicationStatus::Conflict
        );
    }

    #[tokio::test]
    async fn closed_owned_pr_blocks_create_before_record_recovery() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(vec!["ready-for-agent".into()]);
        let pulls = FakePulls::new();
        let (intent, artifact, bundle, manifest) = setup_automatic_fixture(&store).await;
        skip_branch(&store, &intent.intent_id, &bundle.head_commit);

        pulls.prs.lock().unwrap().push(GithubPullRequest {
            number: 55,
            html_url: "https://github.com/acme/repo/pull/55".into(),
            draft: true,
            state: "closed".into(),
            title: "closed owned".into(),
            head: GithubPullRequestRef {
                ref_name: "symphony/_42".into(),
                sha: bundle.head_commit.clone(),
            },
            base: GithubPullRequestRef {
                ref_name: "main".into(),
                sha: bundle.base_commit.clone(),
            },
            user: Some(GithubUser {
                login: "symphony-bot".into(),
            }),
            body: Some(format!("{}\n", pr_marker(&intent.intent_id))),
        });

        let intent = reload_intent(&store, &intent.intent_id).unwrap();
        let publisher = AutomaticImplementationPublisher::new(comments, routing, pulls.clone());
        let err = publisher
            .publish_automatic(
                &store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    issue_title: "Retry",
                    current_issue_revision: "rev",
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &manifest,
                    validation: &[],
                    branch: "symphony/_42",
                    base_branch: "main",
                    remote_url: "/tmp/unused",
                    github_token: None,
                    repo_owner: "acme",
                    approval_route_label: "ready-for-agent",
                    completion_route_state: "Agent Review",
                    storage_path: Path::new("/tmp/unused-db"),
                    max_pages: 2,
                },
            )
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("refusing to replace maintainer divergence"));
        assert_eq!(*pulls.create_calls.lock().unwrap(), 0);
        assert_eq!(
            reload_intent(&store, &intent.intent_id).unwrap().status,
            PublicationStatus::Conflict
        );
    }

    #[tokio::test]
    async fn persisted_draft_pr_is_reverified_after_route_handoff() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(vec!["ready-for-agent".into()]);
        let pulls = FakePulls::new();
        let (intent, artifact, bundle, manifest) = setup_automatic_fixture(&store).await;
        skip_branch(&store, &intent.intent_id, &bundle.head_commit);

        let live_pr = GithubPullRequest {
            number: 55,
            html_url: "https://github.com/acme/repo/pull/55".into(),
            draft: true,
            state: "open".into(),
            title: "owned".into(),
            head: GithubPullRequestRef {
                ref_name: "symphony/_42".into(),
                sha: bundle.head_commit.clone(),
            },
            base: GithubPullRequestRef {
                ref_name: "main".into(),
                sha: bundle.base_commit.clone(),
            },
            user: Some(GithubUser {
                login: "symphony-bot".into(),
            }),
            body: Some(format!("{}\n", pr_marker(&intent.intent_id))),
        };
        pulls.prs.lock().unwrap().push(live_pr.clone());
        store
            .store_draft_pr_artifact(StoreDraftPrArtifactRequest {
                run_id: artifact.run_id.clone(),
                implementation_artifact_id: artifact.artifact_id.clone(),
                intent_id: intent.intent_id.clone(),
                number: live_pr.number,
                url: live_pr.html_url.clone(),
                draft: live_pr.draft,
                head: live_pr.head.ref_name.clone(),
                base: live_pr.base.ref_name.clone(),
                head_sha: live_pr.head.sha.clone(),
                marker: pr_marker(&intent.intent_id),
            })
            .unwrap();
        store
            .record_implementation_publication_step(
                &intent.intent_id,
                PR_VERIFIED_STEP,
                PublicationStatus::Pending,
                &serde_json::json!({"pr_number": 55}),
            )
            .unwrap();
        store
            .record_implementation_publication_step(
                &intent.intent_id,
                ROUTE_APPLIED_STEP,
                PublicationStatus::Pending,
                &serde_json::json!({"pr_number": 55}),
            )
            .unwrap();
        pulls.prs.lock().unwrap()[0].state = "closed".into();

        let intent = reload_intent(&store, &intent.intent_id).unwrap();
        let publisher =
            AutomaticImplementationPublisher::new(comments.clone(), routing.clone(), pulls);
        let err = publisher
            .publish_automatic(
                &store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    issue_title: "Retry",
                    current_issue_revision: "rev",
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &manifest,
                    validation: &[],
                    branch: "symphony/_42",
                    base_branch: "main",
                    remote_url: "/tmp/unused",
                    github_token: None,
                    repo_owner: "acme",
                    approval_route_label: "ready-for-agent",
                    completion_route_state: "Agent Review",
                    storage_path: Path::new("/tmp/unused-db"),
                    max_pages: 2,
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not open"));
        assert!(routing.label_removes.lock().unwrap().is_empty());
        assert!(routing.state.lock().unwrap().is_none());
        assert!(comments.comments.lock().unwrap().is_empty());
        assert_eq!(
            reload_intent(&store, &intent.intent_id).unwrap().status,
            PublicationStatus::Conflict
        );
    }

    #[tokio::test]
    async fn persisted_draft_pr_missing_from_bounded_listing_remains_pending() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(vec!["ready-for-agent".into()]);
        let pulls = FakePulls::new();
        let (intent, artifact, bundle, manifest) = setup_automatic_fixture(&store).await;
        skip_branch(&store, &intent.intent_id, &bundle.head_commit);

        store
            .store_draft_pr_artifact(StoreDraftPrArtifactRequest {
                run_id: artifact.run_id.clone(),
                implementation_artifact_id: artifact.artifact_id.clone(),
                intent_id: intent.intent_id.clone(),
                number: 55,
                url: "https://github.com/acme/repo/pull/55".into(),
                draft: true,
                head: "symphony/_42".into(),
                base: "main".into(),
                head_sha: bundle.head_commit.clone(),
                marker: pr_marker(&intent.intent_id),
            })
            .unwrap();
        store
            .record_implementation_publication_step(
                &intent.intent_id,
                PR_VERIFIED_STEP,
                PublicationStatus::Pending,
                &serde_json::json!({"pr_number": 55}),
            )
            .unwrap();

        let intent = reload_intent(&store, &intent.intent_id).unwrap();
        let publisher = AutomaticImplementationPublisher::new(comments, routing, pulls);
        let err = publisher
            .publish_automatic(
                &store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    issue_title: "Retry",
                    current_issue_revision: "rev",
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &manifest,
                    validation: &[],
                    branch: "symphony/_42",
                    base_branch: "main",
                    remote_url: "/tmp/unused",
                    github_token: None,
                    repo_owner: "acme",
                    approval_route_label: "ready-for-agent",
                    completion_route_state: "Agent Review",
                    storage_path: Path::new("/tmp/unused-db"),
                    max_pages: 2,
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not visible"));

        let recovered = reload_intent(&store, &intent.intent_id).unwrap();
        assert_eq!(recovered.status, PublicationStatus::Pending);
        let last_error = recovered.last_error.unwrap();
        assert_eq!(last_error.code, "pr_not_visible");
        assert!(last_error.retryable);
    }

    #[tokio::test]
    async fn persisted_draft_pr_recovery_records_missing_verified_step() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(vec!["ready-for-agent".into()]);
        let pulls = FakePulls::new();
        let (intent, artifact, bundle, manifest) = setup_automatic_fixture(&store).await;
        skip_branch(&store, &intent.intent_id, &bundle.head_commit);

        let live_pr = GithubPullRequest {
            number: 55,
            html_url: "https://github.com/acme/repo/pull/55".into(),
            draft: true,
            state: "open".into(),
            title: "owned".into(),
            head: GithubPullRequestRef {
                ref_name: "symphony/_42".into(),
                sha: bundle.head_commit.clone(),
            },
            base: GithubPullRequestRef {
                ref_name: "main".into(),
                sha: bundle.base_commit.clone(),
            },
            user: Some(GithubUser {
                login: "symphony-bot".into(),
            }),
            body: Some(format!("{}\n", pr_marker(&intent.intent_id))),
        };
        pulls.prs.lock().unwrap().push(live_pr.clone());
        store
            .store_draft_pr_artifact(StoreDraftPrArtifactRequest {
                run_id: artifact.run_id.clone(),
                implementation_artifact_id: artifact.artifact_id.clone(),
                intent_id: intent.intent_id.clone(),
                number: live_pr.number,
                url: live_pr.html_url,
                draft: live_pr.draft,
                head: live_pr.head.ref_name,
                base: live_pr.base.ref_name,
                head_sha: live_pr.head.sha,
                marker: pr_marker(&intent.intent_id),
            })
            .unwrap();

        let publisher =
            AutomaticImplementationPublisher::new(comments, routing.clone(), pulls.clone());
        publisher
            .publish_automatic(
                &store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 42,
                    issue_title: "Retry",
                    current_issue_revision: "rev",
                    run_id: &artifact.run_id,
                    stage_run_id: &artifact.stage_run_id,
                    artifact: &artifact,
                    bundle: &bundle,
                    manifest: &manifest,
                    validation: &[],
                    branch: "symphony/_42",
                    base_branch: "main",
                    remote_url: "/tmp/unused",
                    github_token: None,
                    repo_owner: "acme",
                    approval_route_label: "ready-for-agent",
                    completion_route_state: "Agent Review",
                    storage_path: Path::new("/tmp/unused-db"),
                    max_pages: 2,
                },
            )
            .await
            .unwrap();

        let recovered = reload_intent(&store, &intent.intent_id).unwrap();
        assert!(recovered
            .completed_steps
            .iter()
            .any(|step| step == PR_VERIFIED_STEP));
        assert_eq!(
            routing.state.lock().unwrap().as_deref(),
            Some("Agent Review")
        );
    }

    #[tokio::test]
    async fn retryable_revision_drift_remains_pending() {
        let (_dir, store) = open_store();
        let comments = FakeComments::new("symphony-bot");
        let routing = FakeRouting::new(vec![]);
        let pulls = FakePulls::new();
        let (intent, artifact, bundle, manifest) = setup_automatic_fixture(&store).await;
        let publisher = AutomaticImplementationPublisher::new(comments, routing, pulls);

        // Drift persists until a human re-approves the issue, which can outlast
        // any retry ceiling. Poll well past that ceiling and assert the intent
        // never consumes retry budget, so it cannot be terminalized as blocked
        // and stranded once the revision is remediated.
        for _ in 0..10 {
            let err = publisher
                .publish_automatic(
                    &store,
                    AutomaticPublishRequest {
                        intent: &intent,
                        issue_number: 42,
                        issue_title: "Retry",
                        current_issue_revision: "other-rev",
                        run_id: &artifact.run_id,
                        stage_run_id: &artifact.stage_run_id,
                        artifact: &artifact,
                        bundle: &bundle,
                        manifest: &manifest,
                        validation: &[],
                        branch: "symphony/_42",
                        base_branch: "main",
                        remote_url: "/tmp/unused",
                        github_token: None,
                        repo_owner: "acme",
                        approval_route_label: "ready-for-agent",
                        completion_route_state: "Agent Review",
                        storage_path: Path::new("/tmp/unused-db"),
                        max_pages: 2,
                    },
                )
                .await
                .unwrap_err();
            assert!(err.to_string().contains("drift"));

            let recovered = store
                .get_implementation_publication_intent(&intent.intent_id)
                .unwrap()
                .unwrap();
            assert_eq!(recovered.status, PublicationStatus::Pending);
            assert_eq!(recovered.retry_count, 0);
            assert_eq!(
                recovered
                    .last_error
                    .as_ref()
                    .map(|error| error.code.as_str()),
                Some("issue_revision_drift")
            );
        }
    }

    #[test]
    fn publication_remote_ignores_persisted_local_workspace_path() {
        let remote = resolve_publication_remote(
            &serde_json::json!({"remote_url": "/workspace/local-checkout"}),
            "github.com",
            "acme",
            "repo",
        )
        .unwrap();
        assert_eq!(remote, "https://github.com/acme/repo.git");
    }

    #[test]
    fn publication_remote_accepts_matching_pinned_remote_url() {
        let remote = resolve_publication_remote(
            &serde_json::json!({"remote_url": "https://github.example/acme/repo.git"}),
            "github.example",
            "acme",
            "repo",
        )
        .unwrap();
        assert_eq!(remote, "https://github.example/acme/repo.git");
    }

    #[test]
    fn publication_remote_rejects_cross_forge_token_target() {
        let err = resolve_publication_remote(
            &serde_json::json!({"remote_url": "https://attacker.example/acme/repo.git"}),
            "github.example",
            "acme",
            "repo",
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not match"));
        assert!(!err.to_string().contains("attacker.example"));
    }

    #[test]
    fn publication_remote_rejects_non_https_transport() {
        let err = resolve_publication_remote(
            &serde_json::json!({"remote_url": "git@github.example:acme/repo.git"}),
            "github.example",
            "acme",
            "repo",
        )
        .unwrap_err();
        assert!(err.to_string().contains("valid HTTPS"));
    }

    #[test]
    fn publication_remote_rejects_embedded_credentials() {
        let err = resolve_publication_remote(
            &serde_json::json!({
                "remote_url": "https://user:secret@github.example/acme/repo.git"
            }),
            "github.com",
            "acme",
            "repo",
        )
        .unwrap_err();
        assert!(err.to_string().contains("must not contain credentials"));
        assert!(!err.to_string().contains("secret"));
    }
}
