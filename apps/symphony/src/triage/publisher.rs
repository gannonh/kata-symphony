use async_trait::async_trait;

use crate::error::{Result, SymphonyError};
use crate::github::client::{GithubClient, GithubIssueComment};
use crate::triage::comment::{
    self, extract_intent_id_from_marker, AutomaticCommentInput, AutomaticCommentPhase,
    IneligibleCommentInput, PreviewCommentInput,
};
use crate::triage::domain::{
    managed_label_set, FactoryError, ManagedProjection, PublicationIntentRecord, PublicationMode,
    PublicationStatus, TriageArtifact,
};
use crate::triage::store::FactoryRunStore;

pub const PREVIEW_COMMENT_STEP: &str = "preview_comment";
pub const DIAGNOSTIC_COMMENT_STEP: &str = "ineligible_diagnostic";
pub const COMMENT_PENDING_STEP: &str = "comment_pending";
pub const ROUTE_LABEL_STEP: &str = "route_label";
pub const PROJECT_STATE_STEP: &str = "project_state";
pub const CONFLICT_CLEANUP_STEP: &str = "conflict_cleanup";
pub const COMMENT_ROUTE_EFFECTS_STEP: &str = "comment_route_effects";
pub const INTAKE_REMOVED_STEP: &str = "intake_removed";
pub const COMMENT_APPLIED_STEP: &str = "comment_applied";

const AUTOMATIC_STEPS: &[&str] = &[
    COMMENT_PENDING_STEP,
    ROUTE_LABEL_STEP,
    PROJECT_STATE_STEP,
    CONFLICT_CLEANUP_STEP,
    COMMENT_ROUTE_EFFECTS_STEP,
    INTAKE_REMOVED_STEP,
    COMMENT_APPLIED_STEP,
];

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
                let _ = (issue_number, max_pages);
                Err(SymphonyError::TriageError(
                    "automatic publication requires AutomaticPublisher".to_string(),
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

#[derive(Debug, Clone)]
pub struct AutomaticPublishRequest<'a> {
    pub intent: &'a PublicationIntentRecord,
    pub issue_number: u64,
    pub run_id: &'a str,
    pub stage_run_id: &'a str,
    pub attempt: u32,
    pub artifact: &'a TriageArtifact,
    pub managed_labels: &'a [String],
    pub max_pages: u32,
}

#[async_trait]
pub trait TriageRoutingPort: Send + Sync {
    async fn list_issue_labels(&self, issue_number: u64) -> Result<Vec<String>>;
    async fn issue_project_state(&self, issue_number: u64) -> Result<Option<String>>;
    async fn add_issue_label(&self, issue_number: u64, label: &str) -> Result<()>;
    async fn remove_issue_label(&self, issue_number: u64, label: &str) -> Result<()>;
    async fn set_issue_project_state(&self, issue_number: u64, state: &str) -> Result<()>;
}

#[async_trait]
impl TriageRoutingPort for std::sync::Arc<dyn TriageRoutingPort> {
    async fn list_issue_labels(&self, issue_number: u64) -> Result<Vec<String>> {
        (**self).list_issue_labels(issue_number).await
    }

    async fn issue_project_state(&self, issue_number: u64) -> Result<Option<String>> {
        (**self).issue_project_state(issue_number).await
    }

    async fn add_issue_label(&self, issue_number: u64, label: &str) -> Result<()> {
        (**self).add_issue_label(issue_number, label).await
    }

    async fn remove_issue_label(&self, issue_number: u64, label: &str) -> Result<()> {
        (**self).remove_issue_label(issue_number, label).await
    }

    async fn set_issue_project_state(&self, issue_number: u64, state: &str) -> Result<()> {
        (**self).set_issue_project_state(issue_number, state).await
    }
}

/// Automatic-mode publisher: marked comments plus deterministic label/state effects.
pub struct AutomaticPublisher<C, R> {
    comments: C,
    routing: R,
}

impl<C, R> AutomaticPublisher<C, R>
where
    C: TriageCommentPort,
    R: TriageRoutingPort,
{
    pub fn new(comments: C, routing: R) -> Self {
        Self { comments, routing }
    }

    pub async fn reconcile_automatic(
        &self,
        store: &mut dyn FactoryRunStore,
        request: AutomaticPublishRequest<'_>,
    ) -> Result<PublicationStatus> {
        if request.intent.mode != PublicationMode::Automatic {
            return Err(SymphonyError::TriageError(format!(
                "automatic publisher cannot reconcile mode={}",
                request.intent.mode.as_str()
            )));
        }

        let mut intent = store
            .get_publication_intent(&request.intent.intent_id)?
            .ok_or_else(|| {
                SymphonyError::TriageError(format!(
                    "publication intent {} missing",
                    request.intent.intent_id
                ))
            })?;

        if matches!(
            intent.status,
            PublicationStatus::Applied | PublicationStatus::Conflict | PublicationStatus::Blocked
        ) {
            return Ok(intent.status);
        }

        let vocabulary = publication_vocabulary(request.managed_labels, &intent.intake_label);
        self.ensure_baseline(store, &intent, request.issue_number, &vocabulary)
            .await?;
        intent = reload_intent(store, &intent.intent_id)?;

        for step in AUTOMATIC_STEPS {
            if intent.completed_steps.iter().any(|done| done == *step) {
                continue;
            }
            match self
                .reconcile_step(store, &mut intent, &request, &vocabulary, step)
                .await?
            {
                PublicationStatus::Pending => {}
                terminal => return Ok(terminal),
            }
            intent = reload_intent(store, &intent.intent_id)?;
        }

        Ok(PublicationStatus::Applied)
    }

    async fn ensure_baseline(
        &self,
        store: &mut dyn FactoryRunStore,
        intent: &PublicationIntentRecord,
        issue_number: u64,
        vocabulary: &std::collections::BTreeSet<String>,
    ) -> Result<()> {
        if ManagedProjection::from_json(&intent.observed_baseline).is_some() {
            return Ok(());
        }
        let current = self.observe_current(issue_number, vocabulary).await?;
        let json = current.to_json();
        store.set_publication_baseline(&intent.intent_id, &json, &json)
    }

    async fn reconcile_step(
        &self,
        store: &mut dyn FactoryRunStore,
        intent: &mut PublicationIntentRecord,
        request: &AutomaticPublishRequest<'_>,
        vocabulary: &std::collections::BTreeSet<String>,
        step: &str,
    ) -> Result<PublicationStatus> {
        let expected =
            ManagedProjection::from_json(&intent.expected_projection).unwrap_or_default();
        match step {
            COMMENT_PENDING_STEP => {
                let body = automatic_body(request, intent, AutomaticCommentPhase::Pending);
                self.upsert_marked_comment(
                    store,
                    intent,
                    request.issue_number,
                    &body,
                    request.max_pages,
                )
                .await?;
                store.record_publication_step(
                    &intent.intent_id,
                    COMMENT_PENDING_STEP,
                    PublicationStatus::Pending,
                    &expected.to_json(),
                )?;
                Ok(PublicationStatus::Pending)
            }
            ROUTE_LABEL_STEP => {
                let desired = expected.with_label(&intent.route_label);
                match self
                    .reconcile_mutation(
                        store,
                        intent,
                        request.issue_number,
                        vocabulary,
                        &expected,
                        &desired,
                        ROUTE_LABEL_STEP,
                        MutationKind::AddLabel(intent.route_label.clone()),
                    )
                    .await?
                {
                    StepOutcome::Conflict => Ok(PublicationStatus::Conflict),
                    StepOutcome::Done => Ok(PublicationStatus::Pending),
                }
            }
            PROJECT_STATE_STEP => {
                let Some(state) = intent.project_state.clone() else {
                    store.record_publication_step(
                        &intent.intent_id,
                        PROJECT_STATE_STEP,
                        PublicationStatus::Pending,
                        &expected.to_json(),
                    )?;
                    return Ok(PublicationStatus::Pending);
                };
                let desired = expected.with_project_state(&state);
                match self
                    .reconcile_mutation(
                        store,
                        intent,
                        request.issue_number,
                        vocabulary,
                        &expected,
                        &desired,
                        PROJECT_STATE_STEP,
                        MutationKind::SetState(state),
                    )
                    .await?
                {
                    StepOutcome::Conflict => Ok(PublicationStatus::Conflict),
                    StepOutcome::Done => Ok(PublicationStatus::Pending),
                }
            }
            CONFLICT_CLEANUP_STEP => {
                let desired = conflict_cleanup_desired(
                    &expected,
                    request.managed_labels,
                    &intent.route_label,
                );
                let to_remove = conflicting_route_labels(
                    &expected,
                    request.managed_labels,
                    &intent.route_label,
                );
                match self
                    .reconcile_mutation(
                        store,
                        intent,
                        request.issue_number,
                        vocabulary,
                        &expected,
                        &desired,
                        CONFLICT_CLEANUP_STEP,
                        MutationKind::RemoveLabels(to_remove),
                    )
                    .await?
                {
                    StepOutcome::Conflict => Ok(PublicationStatus::Conflict),
                    StepOutcome::Done => Ok(PublicationStatus::Pending),
                }
            }
            COMMENT_ROUTE_EFFECTS_STEP => {
                let body =
                    automatic_body(request, intent, AutomaticCommentPhase::RouteEffectsApplied);
                self.upsert_marked_comment(
                    store,
                    intent,
                    request.issue_number,
                    &body,
                    request.max_pages,
                )
                .await?;
                store.record_publication_step(
                    &intent.intent_id,
                    COMMENT_ROUTE_EFFECTS_STEP,
                    PublicationStatus::Pending,
                    &expected.to_json(),
                )?;
                Ok(PublicationStatus::Pending)
            }
            INTAKE_REMOVED_STEP => {
                let desired = expected.without_label(&intent.intake_label);
                match self
                    .reconcile_mutation(
                        store,
                        intent,
                        request.issue_number,
                        vocabulary,
                        &expected,
                        &desired,
                        INTAKE_REMOVED_STEP,
                        MutationKind::RemoveLabels(vec![intent.intake_label.clone()]),
                    )
                    .await?
                {
                    StepOutcome::Conflict => Ok(PublicationStatus::Conflict),
                    StepOutcome::Done => Ok(PublicationStatus::Pending),
                }
            }
            COMMENT_APPLIED_STEP => {
                let body = automatic_body(request, intent, AutomaticCommentPhase::Applied);
                self.upsert_marked_comment(
                    store,
                    intent,
                    request.issue_number,
                    &body,
                    request.max_pages,
                )
                .await?;
                store.record_publication_step(
                    &intent.intent_id,
                    COMMENT_APPLIED_STEP,
                    PublicationStatus::Applied,
                    &expected.to_json(),
                )?;
                Ok(PublicationStatus::Applied)
            }
            other => Err(SymphonyError::TriageError(format!(
                "unknown automatic publication step {other}"
            ))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn reconcile_mutation(
        &self,
        store: &mut dyn FactoryRunStore,
        intent: &PublicationIntentRecord,
        issue_number: u64,
        vocabulary: &std::collections::BTreeSet<String>,
        expected: &ManagedProjection,
        desired: &ManagedProjection,
        step: &str,
        mutation: MutationKind,
    ) -> Result<StepOutcome> {
        let current = self.observe_current(issue_number, vocabulary).await?;
        if current == *desired {
            store.record_publication_step(
                &intent.intent_id,
                step,
                PublicationStatus::Pending,
                &desired.to_json(),
            )?;
            return Ok(StepOutcome::Done);
        }
        if current != *expected {
            let error = FactoryError::new(
                "human_conflict",
                "publisher",
                format!(
                    "managed GitHub state differs from both expected and desired projections at step {step}"
                ),
                false,
                Some(step.to_string()),
            );
            store.update_publication_step(
                &intent.intent_id,
                "",
                PublicationStatus::Conflict,
                Some(error),
            )?;
            return Ok(StepOutcome::Conflict);
        }

        match mutation {
            MutationKind::AddLabel(label) => {
                self.routing.add_issue_label(issue_number, &label).await?;
            }
            MutationKind::SetState(state) => {
                self.routing
                    .set_issue_project_state(issue_number, &state)
                    .await?;
            }
            MutationKind::RemoveLabels(labels) => {
                // Persist each successful removal so a crash mid-cleanup leaves
                // expected projection on a publisher-owned intermediate state
                // instead of looking like a human conflict on retry.
                let mut progressive = current.clone();
                for label in labels {
                    if progressive.contains_label(&label) {
                        self.routing
                            .remove_issue_label(issue_number, &label)
                            .await?;
                        progressive = progressive.without_label(&label);
                        store.record_publication_step(
                            &intent.intent_id,
                            "",
                            PublicationStatus::Pending,
                            &progressive.to_json(),
                        )?;
                    }
                }
            }
        }

        store.record_publication_step(
            &intent.intent_id,
            step,
            PublicationStatus::Pending,
            &desired.to_json(),
        )?;
        Ok(StepOutcome::Done)
    }

    async fn observe_current(
        &self,
        issue_number: u64,
        vocabulary: &std::collections::BTreeSet<String>,
    ) -> Result<ManagedProjection> {
        let labels = self.routing.list_issue_labels(issue_number).await?;
        let state = self.routing.issue_project_state(issue_number).await?;
        Ok(ManagedProjection::observe(
            labels.iter().map(String::as_str),
            vocabulary,
            state.as_deref(),
        ))
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
        }
        Ok(None)
    }
}

enum MutationKind {
    AddLabel(String),
    SetState(String),
    RemoveLabels(Vec<String>),
}

enum StepOutcome {
    Done,
    Conflict,
}

fn publication_vocabulary(
    route_labels: &[String],
    intake_label: &str,
) -> std::collections::BTreeSet<String> {
    let mut vocabulary = managed_label_set(route_labels.iter().map(String::as_str));
    let intake = intake_label.trim().to_ascii_lowercase();
    if !intake.is_empty() {
        vocabulary.insert(intake);
    }
    vocabulary
}

fn conflicting_route_labels(
    expected: &ManagedProjection,
    managed_labels: &[String],
    desired_route_label: &str,
) -> Vec<String> {
    let desired = desired_route_label.trim().to_ascii_lowercase();
    managed_labels
        .iter()
        .map(|label| label.trim().to_ascii_lowercase())
        .filter(|label| !label.is_empty() && *label != desired && expected.contains_label(label))
        .collect()
}

fn conflict_cleanup_desired(
    expected: &ManagedProjection,
    managed_labels: &[String],
    desired_route_label: &str,
) -> ManagedProjection {
    let mut next = expected.clone();
    for label in conflicting_route_labels(expected, managed_labels, desired_route_label) {
        next = next.without_label(&label);
    }
    next
}

fn automatic_body(
    request: &AutomaticPublishRequest<'_>,
    intent: &PublicationIntentRecord,
    phase: AutomaticCommentPhase,
) -> String {
    comment::render_automatic_comment(&AutomaticCommentInput {
        intent_id: &intent.intent_id,
        run_id: request.run_id,
        stage_run_id: request.stage_run_id,
        attempt: request.attempt,
        artifact: request.artifact,
        route_label: &intent.route_label,
        project_state: intent.project_state.as_deref(),
        phase,
    })
}

fn reload_intent(
    store: &mut dyn FactoryRunStore,
    intent_id: &str,
) -> Result<PublicationIntentRecord> {
    store.get_publication_intent(intent_id)?.ok_or_else(|| {
        SymphonyError::TriageError(format!("publication intent {intent_id} missing after step"))
    })
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
        EvidenceKind, RiskClass, TriageEvidence, TriageRoute, TriageRoutesConfig,
        TRIAGE_SCHEMA_VERSION,
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
        history: Mutex<Vec<String>>,
    }

    impl MockComments {
        fn new(login: &str) -> Arc<Self> {
            Arc::new(Self {
                login: login.to_string(),
                comments: Mutex::new(HashMap::new()),
                next_id: Mutex::new(100),
                create_count: Mutex::new(0),
                update_count: Mutex::new(0),
                history: Mutex::new(Vec::new()),
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
            self.history.lock().unwrap().push(body.to_string());
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
            self.history.lock().unwrap().push(body.to_string());
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

    #[derive(Default)]
    struct MockRouting {
        labels: Mutex<Vec<String>>,
        project_state: Mutex<Option<String>>,
        calls: Mutex<Vec<String>>,
    }

    impl MockRouting {
        fn new(labels: &[&str], project_state: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                labels: Mutex::new(labels.iter().map(|label| label.to_string()).collect()),
                project_state: Mutex::new(project_state.map(str::to_string)),
                calls: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn labels_now(&self) -> Vec<String> {
            self.labels.lock().unwrap().clone()
        }

        fn state_now(&self) -> Option<String> {
            self.project_state.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TriageRoutingPort for Arc<MockRouting> {
        async fn list_issue_labels(&self, _issue_number: u64) -> Result<Vec<String>> {
            Ok(self.labels.lock().unwrap().clone())
        }

        async fn issue_project_state(&self, _issue_number: u64) -> Result<Option<String>> {
            Ok(self.project_state.lock().unwrap().clone())
        }

        async fn add_issue_label(&self, _issue_number: u64, label: &str) -> Result<()> {
            self.calls.lock().unwrap().push(format!("add:{label}"));
            let mut labels = self.labels.lock().unwrap();
            if !labels.iter().any(|value| value == label) {
                labels.push(label.to_string());
            }
            Ok(())
        }

        async fn remove_issue_label(&self, _issue_number: u64, label: &str) -> Result<()> {
            self.calls.lock().unwrap().push(format!("remove:{label}"));
            self.labels.lock().unwrap().retain(|value| value != label);
            Ok(())
        }

        async fn set_issue_project_state(&self, _issue_number: u64, state: &str) -> Result<()> {
            self.calls.lock().unwrap().push(format!("state:{state}"));
            *self.project_state.lock().unwrap() = Some(state.to_string());
            Ok(())
        }
    }

    fn artifact() -> TriageArtifact {
        artifact_for(TriageRoute::Implement)
    }

    fn artifact_for(route: TriageRoute) -> TriageArtifact {
        TriageArtifact {
            schema_version: TRIAGE_SCHEMA_VERSION,
            route,
            risk_class: RiskClass::Low,
            rationale: "Bounded fix.".to_string(),
            evidence: vec![TriageEvidence {
                kind: EvidenceKind::Issue,
                reference: "body".to_string(),
                summary: "Exact replacement named.".to_string(),
            }],
            next_action: "Apply the fix.".to_string(),
            clarification_question: (route == TriageRoute::NeedsInformation)
                .then(|| "Which fallback should Symphony use?".to_string()),
            reproduction: None,
        }
    }

    fn managed_labels() -> Vec<String> {
        TriageRoutesConfig::default().managed_labels()
    }

    fn store_with_automatic_intent(
        route: TriageRoute,
    ) -> (
        tempfile::TempDir,
        SqliteFactoryStore,
        PublicationIntentRecord,
    ) {
        let routes = TriageRoutesConfig::default();
        let mapping = routes.mapping_for(route);
        store_with_intent(
            PublicationMode::Automatic,
            artifact_for(route),
            &mapping.label,
            mapping.state.clone(),
        )
    }

    fn store_with_preview_intent() -> (
        tempfile::TempDir,
        SqliteFactoryStore,
        PublicationIntentRecord,
    ) {
        store_with_intent(
            PublicationMode::Preview,
            artifact(),
            "ready-for-agent",
            None,
        )
    }

    fn store_with_intent(
        mode: PublicationMode,
        artifact: TriageArtifact,
        route_label: &str,
        project_state: Option<String>,
    ) -> (
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
                artifact,
                bytes_len: 100,
                usage: Default::default(),
            })
            .unwrap();
        let desired_kind = match mode {
            PublicationMode::Preview => "preview_comment",
            PublicationMode::Automatic => "automatic_route",
        };
        let intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact_record.run_id,
                artifact_id: Some(artifact_record.artifact_id),
                mode,
                intake_label: "needs-triage".to_string(),
                route_label: route_label.to_string(),
                project_state,
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": desired_kind}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();
        (temp, store, intent)
    }

    #[tokio::test]
    async fn automatic_implement_route_applies_label_state_then_removes_intake_last() {
        let (_temp, mut store, intent) = store_with_automatic_intent(TriageRoute::Implement);
        let comments = MockComments::new("symphony-bot");
        let routing = MockRouting::new(&["needs-triage", "docs"], Some("Backlog"));
        let publisher = AutomaticPublisher::new(comments.clone(), routing.clone());
        let artifact = artifact_for(TriageRoute::Implement);

        let status = publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact,
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(status, PublicationStatus::Applied);
        assert_eq!(
            routing.calls(),
            vec![
                "add:ready-for-agent".to_string(),
                "state:Todo".to_string(),
                "remove:needs-triage".to_string(),
            ]
        );
        assert_eq!(
            routing.labels_now(),
            vec!["docs".to_string(), "ready-for-agent".to_string()]
        );
        assert_eq!(routing.state_now().as_deref(), Some("Todo"));

        let reloaded = store
            .get_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.status, PublicationStatus::Applied);
        assert_eq!(
            reloaded.completed_steps,
            vec![
                COMMENT_PENDING_STEP.to_string(),
                ROUTE_LABEL_STEP.to_string(),
                PROJECT_STATE_STEP.to_string(),
                CONFLICT_CLEANUP_STEP.to_string(),
                COMMENT_ROUTE_EFFECTS_STEP.to_string(),
                INTAKE_REMOVED_STEP.to_string(),
                COMMENT_APPLIED_STEP.to_string(),
            ]
        );

        assert_eq!(*comments.create_count.lock().unwrap(), 1);
        let history = comments.history.lock().unwrap().clone();
        assert!(history[0].contains("Publication: `pending`"));
        assert!(history
            .iter()
            .any(|body| body.contains("Route effects: `applied`; publication: `pending`")));
        assert!(history.last().unwrap().contains("Publication: `applied`"));
        assert!(history.last().unwrap().contains("`ready-for-agent`"));
        assert!(history.last().unwrap().contains("`Todo`"));
    }

    #[tokio::test]
    async fn automatic_spec_route_skips_project_state_and_stays_ineligible_for_implement() {
        let (_temp, mut store, intent) = store_with_automatic_intent(TriageRoute::Spec);
        let comments = MockComments::new("symphony-bot");
        let routing = MockRouting::new(&["needs-triage"], Some("Backlog"));
        let publisher = AutomaticPublisher::new(comments.clone(), routing.clone());
        let artifact = artifact_for(TriageRoute::Spec);

        let status = publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact,
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(status, PublicationStatus::Applied);
        assert_eq!(
            routing.calls(),
            vec![
                "add:ready-to-spec".to_string(),
                "remove:needs-triage".to_string(),
            ]
        );
        assert_eq!(routing.state_now().as_deref(), Some("Backlog"));
        let reloaded = store
            .get_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert!(reloaded
            .completed_steps
            .iter()
            .any(|step| step == PROJECT_STATE_STEP));
    }

    #[tokio::test]
    async fn automatic_conflict_cleanup_removes_other_managed_route_labels() {
        let (_temp, mut store, intent) = store_with_automatic_intent(TriageRoute::Implement);
        let comments = MockComments::new("symphony-bot");
        let routing = MockRouting::new(&["needs-triage", "ready-to-spec", "docs"], Some("Backlog"));
        let publisher = AutomaticPublisher::new(comments.clone(), routing.clone());

        publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact_for(TriageRoute::Implement),
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert!(routing
            .calls()
            .contains(&"remove:ready-to-spec".to_string()));
        assert_eq!(
            routing.labels_now(),
            vec!["docs".to_string(), "ready-for-agent".to_string()]
        );
    }

    #[tokio::test]
    async fn automatic_crash_mid_conflict_cleanup_resumes_without_false_conflict() {
        let (_temp, mut store, intent) = store_with_automatic_intent(TriageRoute::Implement);
        let comments = MockComments::new("symphony-bot");
        let routing = MockRouting::new(
            &["needs-triage", "ready-to-spec", "needs-info"],
            Some("Backlog"),
        );
        let publisher = AutomaticPublisher::new(comments.clone(), routing.clone());

        publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact_for(TriageRoute::Implement),
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        // Simulate crash after removing one conflicting label during cleanup:
        // rewind before conflict_cleanup completes, keep GitHub intermediate state,
        // and leave expected projection on that publisher-owned intermediate.
        let intermediate = ManagedProjection::observe(
            ["needs-triage", "ready-for-agent", "needs-info"],
            &publication_vocabulary(&managed_labels(), "needs-triage"),
            Some("Todo"),
        );
        store
            .debug_rewind_publication(
                &intent.intent_id,
                &[COMMENT_PENDING_STEP, ROUTE_LABEL_STEP, PROJECT_STATE_STEP],
                &intermediate.to_json(),
            )
            .unwrap();
        *routing.labels.lock().unwrap() = vec![
            "needs-triage".to_string(),
            "ready-for-agent".to_string(),
            "needs-info".to_string(),
        ];
        *routing.project_state.lock().unwrap() = Some("Todo".to_string());
        *routing.calls.lock().unwrap() = Vec::new();

        let pending = store
            .get_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        let status = publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &pending,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact_for(TriageRoute::Implement),
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(status, PublicationStatus::Applied);
        assert!(!routing.labels_now().contains(&"needs-info".to_string()));
        assert!(!routing.labels_now().contains(&"needs-triage".to_string()));
    }

    #[tokio::test]
    async fn automatic_human_conflict_stops_without_intake_removal() {
        let (_temp, mut store, intent) = store_with_automatic_intent(TriageRoute::Implement);
        let comments = MockComments::new("symphony-bot");
        let routing = MockRouting::new(&["needs-triage"], Some("Backlog"));
        let publisher = AutomaticPublisher::new(comments.clone(), routing.clone());

        publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact_for(TriageRoute::Implement),
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        let applied = store
            .get_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        let baseline = applied.observed_baseline.clone();
        store
            .debug_rewind_publication(&intent.intent_id, &[COMMENT_PENDING_STEP], &baseline)
            .unwrap();
        // Human changed managed labels away from the recorded expected projection.
        *routing.labels.lock().unwrap() =
            vec!["needs-triage".to_string(), "ready-to-spec".to_string()];
        *routing.calls.lock().unwrap() = Vec::new();

        let pending = store
            .get_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        let status = publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &pending,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact_for(TriageRoute::Implement),
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(status, PublicationStatus::Conflict);
        assert!(routing.labels_now().contains(&"needs-triage".to_string()));
        assert!(!routing
            .calls()
            .iter()
            .any(|call| call == "remove:needs-triage"));
    }

    #[tokio::test]
    async fn automatic_crash_after_route_label_resumes_without_duplicate_add() {
        let (_temp, mut store, intent) = store_with_automatic_intent(TriageRoute::Implement);
        let comments = MockComments::new("symphony-bot");
        let routing = MockRouting::new(&["needs-triage"], Some("Backlog"));
        let publisher = AutomaticPublisher::new(comments.clone(), routing.clone());

        publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact_for(TriageRoute::Implement),
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        let applied = store
            .get_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        let baseline = applied.observed_baseline.clone();
        store
            .debug_rewind_publication(&intent.intent_id, &[COMMENT_PENDING_STEP], &baseline)
            .unwrap();
        // GitHub already has the route label from the "lost" successful mutation.
        *routing.labels.lock().unwrap() =
            vec!["needs-triage".to_string(), "ready-for-agent".to_string()];
        *routing.calls.lock().unwrap() = vec!["add:ready-for-agent".to_string()];
        *routing.project_state.lock().unwrap() = Some("Backlog".to_string());

        let pending = store
            .get_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &pending,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact_for(TriageRoute::Implement),
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        let add_calls = routing
            .calls()
            .into_iter()
            .filter(|call| call == "add:ready-for-agent")
            .count();
        assert_eq!(add_calls, 1);
        let reloaded = store
            .get_publication_intent(&intent.intent_id)
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.status, PublicationStatus::Applied);
    }

    #[tokio::test]
    async fn automatic_needs_information_comment_includes_clarification_question() {
        let (_temp, mut store, intent) = store_with_automatic_intent(TriageRoute::NeedsInformation);
        let comments = MockComments::new("symphony-bot");
        let routing = MockRouting::new(&["needs-triage"], None);
        let publisher = AutomaticPublisher::new(comments.clone(), routing.clone());
        let artifact = artifact_for(TriageRoute::NeedsInformation);

        publisher
            .reconcile_automatic(
                &mut store,
                AutomaticPublishRequest {
                    intent: &intent,
                    issue_number: 12,
                    run_id: "run",
                    stage_run_id: "stage",
                    attempt: 1,
                    artifact: &artifact,
                    managed_labels: &managed_labels(),
                    max_pages: 10,
                },
            )
            .await
            .unwrap();

        let history = comments.history.lock().unwrap().clone();
        assert!(history
            .iter()
            .any(|body| body.contains("Which fallback should Symphony use?")));
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
