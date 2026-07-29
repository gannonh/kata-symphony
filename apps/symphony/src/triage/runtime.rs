//! Runtime wiring for triage: shared store, HTTP query, and event emission.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::domain::{EventKind, EventSeverity, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::event_stream::EventHub;
use crate::github::client::GithubClient;
use crate::github::projects_v2::ProjectsV2Client;
use crate::http_server::{
    attach_implementation_http_response, attach_spec_http_response, factory_run_http_response,
    factory_run_metrics_http_response, implementation_run_metrics_http_response,
    spec_run_metrics_http_response, FactoryArtifactHttpResponse, FactoryRunHttpResponse,
    FactoryRunMetricsHttpResponse, FactoryRunQuery, ImplementationRunMetricsHttpResponse,
    SpecRunMetricsHttpResponse,
};
use crate::implementation::coordinator::{
    ImplementationCoordinator, ImplementationCoordinatorConfig,
};
use crate::implementation::runner::LiveImplementationHarness;
use crate::spec::coordinator::{SpecCoordinator, SpecCoordinatorConfig};
use crate::triage::coordinator::{
    EventEmitter, TriageCoordinator, TriageCoordinatorConfig, TriagePollSummary,
};
use crate::triage::domain::{
    ArtifactRecord, FactoryError, FactoryEventRecord, FactoryRunRecord, FactoryRunStatus,
    PublicationIntentRecord, PublicationStatus, StageRunRecord,
};
use crate::triage::intake::GithubTriageIntake;
use crate::triage::routing::GithubTriageRouting;
use crate::triage::storage_path::{
    forge_host_from_endpoint, resolve_storage_path, storage_path_for_log,
};
use crate::triage::store::{
    A3EligibleApprovedRun, ClaimAttemptRequest, CreatePublicationIntentRequest, FactoryRunStore,
    PendingAutomaticDispatchGuard, SqliteFactoryStore, StoreArtifactRequest,
    StoreBundleArtifactRequest, StoreImplementationArtifactRequest, StoreImplementationTurnRequest,
    StoreSpecArtifactRequest, StoreSpecTurnRequest, StoreValidationCycleRequest,
    StoredCommentIdentity, UpsertFactoryRunRequest, UpsertImplementationStateRequest,
};

/// Shared SQLite factory store for coordinator + HTTP reads.
#[derive(Clone)]
pub struct SharedFactoryStore {
    inner: Arc<Mutex<SqliteFactoryStore>>,
}

impl SharedFactoryStore {
    pub fn open(path: &Path, busy_timeout_ms: u64) -> Result<Self> {
        let store = SqliteFactoryStore::acquire_lock_and_migrate(path, busy_timeout_ms)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(store)),
        })
    }

    /// Read durable nonterminal automatic publication guards for dispatch.
    pub fn pending_automatic_dispatch_guards(&self) -> Result<Vec<PendingAutomaticDispatchGuard>> {
        self.with_store(|store| store.list_pending_automatic_dispatch_guards())
    }

    pub fn claim_spec_attempt(&self, request: ClaimAttemptRequest) -> Result<StageRunRecord> {
        self.with_store_mut(|store| {
            store.claim_stage_attempt(crate::spec::domain::SPEC_STAGE_NAME, request)
        })
    }

    pub fn store_spec_turn(
        &self,
        request: StoreSpecTurnRequest,
    ) -> Result<crate::spec::domain::SpecTurnRecord> {
        self.with_store_mut(|store| store.store_spec_turn(request))
    }

    pub fn store_spec_artifact(
        &self,
        request: StoreSpecArtifactRequest,
    ) -> Result<crate::spec::domain::SpecArtifactRecord> {
        self.with_store_mut(|store| store.store_spec_artifact(request))
    }

    pub fn get_spec_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<crate::spec::domain::SpecArtifactRecord>> {
        self.with_store(|store| store.get_spec_artifact(artifact_id))
    }

    pub fn list_spec_artifacts(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::spec::domain::SpecArtifactRecord>> {
        self.with_store(|store| store.list_spec_artifacts(run_id))
    }

    pub fn list_spec_turns(
        &self,
        stage_run_id: &str,
    ) -> Result<Vec<crate::spec::domain::SpecTurnRecord>> {
        self.with_store(|store| store.list_spec_turns(stage_run_id))
    }

    pub fn get_spec_state(
        &self,
        run_id: &str,
    ) -> Result<Option<crate::spec::domain::SpecRunState>> {
        self.with_store(|store| store.get_spec_state(run_id))
    }

    pub fn create_spec_publication_intent(
        &self,
        run_id: &str,
        artifact_id: Option<&str>,
        kind: crate::spec::domain::SpecPublicationKind,
        desired_effects: &serde_json::Value,
    ) -> Result<crate::spec::domain::SpecPublicationIntent> {
        self.with_store_mut(|store| {
            store.create_spec_publication_intent(run_id, artifact_id, kind, desired_effects)
        })
    }

    pub fn get_latest_spec_publication(
        &self,
        run_id: &str,
    ) -> Result<Option<crate::spec::domain::SpecPublicationIntent>> {
        self.with_store(|store| store.get_latest_spec_publication(run_id))
    }

    pub fn find_spec_publication_by_kind(
        &self,
        run_id: &str,
        kind: crate::spec::domain::SpecPublicationKind,
    ) -> Result<Option<crate::spec::domain::SpecPublicationIntent>> {
        self.with_store(|store| store.find_spec_publication_by_kind(run_id, kind))
    }

    pub fn latest_spec_publication_of_kind(
        &self,
        run_id: &str,
        kind: crate::spec::domain::SpecPublicationKind,
    ) -> Result<Option<crate::spec::domain::SpecPublicationIntent>> {
        self.with_store(|store| store.latest_spec_publication_of_kind(run_id, kind))
    }

    pub fn list_pending_spec_publications(
        &self,
    ) -> Result<Vec<crate::spec::domain::SpecPublicationIntent>> {
        self.with_store(|store| store.list_pending_spec_publications())
    }

    pub fn set_spec_publication_comment(
        &self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.set_spec_publication_comment(intent_id, comment_id, publisher_login)
        })
    }

    pub fn complete_spec_publication(&self, intent_id: &str, step: &str) -> Result<()> {
        self.with_store_mut(|store| store.complete_spec_publication(intent_id, step))
    }

    pub fn update_spec_publication_step(
        &self,
        intent_id: &str,
        completed_step: &str,
        status: crate::triage::domain::PublicationStatus,
        error: Option<crate::triage::domain::FactoryError>,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.update_spec_publication_step(intent_id, completed_step, status, error)
        })
    }

    pub fn update_spec_diagnostic_message(
        &self,
        intent_id: &str,
        desired_effects: &serde_json::Value,
    ) -> Result<crate::spec::domain::SpecPublicationIntent> {
        self.with_store_mut(|store| {
            store.update_spec_diagnostic_message(intent_id, desired_effects)
        })
    }

    pub fn increment_spec_revision_requests(&self, run_id: &str, max: u32) -> Result<u32> {
        self.with_store_mut(|store| store.increment_spec_revision_requests(run_id, max))
    }

    pub fn set_pending_spec_approval(&self, run_id: &str, version: u32) -> Result<()> {
        self.with_store_mut(|store| store.set_pending_spec_approval(run_id, version))
    }

    pub fn pin_spec_approval(&self, run_id: &str, artifact_id: &str) -> Result<()> {
        self.with_store_mut(|store| store.pin_spec_approval(run_id, artifact_id))
    }

    pub fn finalize_spec_approval(
        &self,
        run_id: &str,
        intent_id: &str,
        completed_step: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| store.finalize_spec_approval(run_id, intent_id, completed_step))
    }

    pub fn claim_implementation_attempt(
        &self,
        request: ClaimAttemptRequest,
    ) -> Result<StageRunRecord> {
        self.with_store_mut(|store| {
            store.claim_stage_attempt(
                crate::implementation::domain::IMPLEMENTATION_STAGE_NAME,
                request,
            )
        })
    }

    pub fn store_implementation_turn(
        &self,
        request: StoreImplementationTurnRequest,
    ) -> Result<crate::implementation::domain::ImplementationTurnRecord> {
        self.with_store_mut(|store| store.store_implementation_turn(request))
    }

    pub fn list_implementation_turns(
        &self,
        stage_run_id: &str,
    ) -> Result<Vec<crate::implementation::domain::ImplementationTurnRecord>> {
        self.with_store(|store| store.list_implementation_turns(stage_run_id))
    }

    pub fn store_validation_cycle(
        &self,
        request: StoreValidationCycleRequest,
    ) -> Result<crate::implementation::domain::ValidationCycleRecord> {
        self.with_store_mut(|store| store.store_validation_cycle(request))
    }

    pub fn list_validation_cycles(
        &self,
        stage_run_id: &str,
    ) -> Result<Vec<crate::implementation::domain::ValidationCycleRecord>> {
        self.with_store(|store| store.list_validation_cycles(stage_run_id))
    }

    pub fn store_implementation_artifact(
        &self,
        request: StoreImplementationArtifactRequest,
    ) -> Result<crate::implementation::domain::ImplementationArtifactRecord> {
        self.with_store_mut(|store| store.store_implementation_artifact(request))
    }

    pub fn get_implementation_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<crate::implementation::domain::ImplementationArtifactRecord>> {
        self.with_store(|store| store.get_implementation_artifact(artifact_id))
    }

    pub fn list_implementation_artifacts(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::implementation::domain::ImplementationArtifactRecord>> {
        self.with_store(|store| store.list_implementation_artifacts(run_id))
    }

    pub fn store_bundle_artifact(
        &self,
        request: StoreBundleArtifactRequest,
    ) -> Result<crate::implementation::domain::BundleArtifactRecord> {
        self.with_store_mut(|store| store.store_bundle_artifact(request))
    }

    pub fn get_bundle_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<crate::implementation::domain::BundleArtifactRecord>> {
        self.with_store(|store| store.get_bundle_artifact(artifact_id))
    }

    pub fn get_implementation_state(
        &self,
        run_id: &str,
    ) -> Result<Option<crate::implementation::domain::ImplementationRunState>> {
        self.with_store(|store| store.get_implementation_state(run_id))
    }

    pub fn upsert_implementation_state(
        &self,
        request: UpsertImplementationStateRequest,
    ) -> Result<crate::implementation::domain::ImplementationRunState> {
        self.with_store_mut(|store| store.upsert_implementation_state(request))
    }

    pub fn set_implementation_decision(
        &self,
        run_id: &str,
        decision: crate::implementation::domain::ImplementationDecision,
        blocker: Option<crate::implementation::domain::ImplementationBlocker>,
    ) -> Result<()> {
        self.with_store_mut(|store| store.set_implementation_decision(run_id, decision, blocker))
    }

    pub fn create_implementation_publication_intent(
        &self,
        run_id: &str,
        artifact_id: Option<&str>,
        kind: crate::implementation::domain::ImplementationPublicationKind,
        desired_effects: &serde_json::Value,
    ) -> Result<crate::implementation::domain::ImplementationPublicationIntent> {
        self.with_store_mut(|store| {
            store.create_implementation_publication_intent(
                run_id,
                artifact_id,
                kind,
                desired_effects,
            )
        })
    }

    pub fn get_implementation_publication_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<crate::implementation::domain::ImplementationPublicationIntent>> {
        self.with_store(|store| store.get_implementation_publication_intent(intent_id))
    }

    pub fn list_pending_implementation_publications(
        &self,
    ) -> Result<Vec<crate::implementation::domain::ImplementationPublicationIntent>> {
        self.with_store(|store| store.list_pending_implementation_publications())
    }

    pub fn list_implementation_publications_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::implementation::domain::ImplementationPublicationIntent>> {
        self.with_store(|store| store.list_implementation_publications_for_run(run_id))
    }

    pub fn complete_implementation_publication(&self, intent_id: &str, step: &str) -> Result<()> {
        self.with_store_mut(|store| store.complete_implementation_publication(intent_id, step))
    }

    pub fn record_implementation_publication_step(
        &self,
        intent_id: &str,
        step: &str,
        status: crate::triage::domain::PublicationStatus,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.record_implementation_publication_step(
                intent_id,
                step,
                status,
                expected_projection,
            )
        })
    }

    pub fn set_implementation_publication_baseline(
        &self,
        intent_id: &str,
        observed_baseline: &serde_json::Value,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.set_implementation_publication_baseline(
                intent_id,
                observed_baseline,
                expected_projection,
            )
        })
    }

    pub fn set_implementation_publication_error(
        &self,
        intent_id: &str,
        status: crate::triage::domain::PublicationStatus,
        error: crate::triage::domain::FactoryError,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.set_implementation_publication_error(intent_id, status, error)
        })
    }

    pub fn store_draft_pr_artifact(
        &self,
        request: crate::triage::store::StoreDraftPrArtifactRequest,
    ) -> Result<crate::implementation::domain::DraftPrArtifactRecord> {
        self.with_store_mut(|store| store.store_draft_pr_artifact(request))
    }

    pub fn get_draft_pr_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<crate::implementation::domain::DraftPrArtifactRecord>> {
        self.with_store(|store| store.get_draft_pr_artifact(artifact_id))
    }

    pub fn get_draft_pr_for_implementation_artifact(
        &self,
        implementation_artifact_id: &str,
    ) -> Result<Option<crate::implementation::domain::DraftPrArtifactRecord>> {
        self.with_store(|store| {
            store.get_draft_pr_for_implementation_artifact(implementation_artifact_id)
        })
    }

    pub fn get_draft_pr_for_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<crate::implementation::domain::DraftPrArtifactRecord>> {
        self.with_store(|store| store.get_draft_pr_for_intent(intent_id))
    }

    pub fn bind_implementation_publication_comment(
        &self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.bind_implementation_publication_comment(intent_id, comment_id, publisher_login)
        })
    }

    pub fn store_implementation_attempt_inputs(
        &self,
        stage_run_id: &str,
        inputs: &crate::implementation::domain::ImplementationAttemptInputs,
    ) -> Result<()> {
        self.with_store_mut(|store| store.store_implementation_attempt_inputs(stage_run_id, inputs))
    }

    pub fn get_implementation_attempt_inputs(
        &self,
        stage_run_id: &str,
    ) -> Result<Option<crate::implementation::domain::ImplementationAttemptInputs>> {
        self.with_store(|store| store.get_implementation_attempt_inputs(stage_run_id))
    }

    pub fn list_a3_eligible_approved_runs(
        &self,
        configuration_revision: &str,
    ) -> Result<Vec<A3EligibleApprovedRun>> {
        self.with_store(|store| store.list_a3_eligible_approved_runs(configuration_revision))
    }

    fn with_store<T>(&self, f: impl FnOnce(&SqliteFactoryStore) -> Result<T>) -> Result<T> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| SymphonyError::StorageError("factory store lock poisoned".to_string()))?;
        f(&guard)
    }

    fn with_store_mut<T>(&self, f: impl FnOnce(&mut SqliteFactoryStore) -> Result<T>) -> Result<T> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| SymphonyError::StorageError("factory store lock poisoned".to_string()))?;
        f(&mut guard)
    }

    fn load_run_response(
        &self,
        lookup: impl FnOnce(&SqliteFactoryStore) -> Result<Option<FactoryRunRecord>>,
    ) -> std::result::Result<Option<FactoryRunHttpResponse>, String> {
        self.with_store(|store| {
            let Some(run) = lookup(store)? else {
                return Ok(None);
            };
            let attempts = store.list_stage_runs(&run.run_id)?;
            let artifact = store.get_latest_artifact(&run.run_id)?;
            let publication = store.get_latest_publication(&run.run_id)?;
            let spec_artifacts = store.list_spec_artifacts(&run.run_id)?;
            let spec_state = store.get_spec_state(&run.run_id)?;
            let spec_publication = store.get_latest_spec_publication(&run.run_id)?;
            let mut turns = std::collections::HashMap::new();
            for attempt in attempts
                .iter()
                .filter(|attempt| attempt.stage == crate::spec::domain::SPEC_STAGE_NAME)
            {
                turns.insert(
                    attempt.stage_run_id.clone(),
                    store.list_spec_turns(&attempt.stage_run_id)?,
                );
            }
            let mut response =
                factory_run_http_response(&run, &attempts, artifact.as_ref(), publication.as_ref());
            attach_spec_http_response(
                &mut response,
                &spec_artifacts,
                spec_state.as_ref(),
                spec_publication.as_ref(),
                &turns,
            );
            let implementation_state = store.get_implementation_state(&run.run_id)?;
            let implementation_artifacts = store.list_implementation_artifacts(&run.run_id)?;
            let publication = store
                .list_implementation_publications_for_run(&run.run_id)?
                .into_iter()
                .next();
            let bundle = implementation_state
                .as_ref()
                .and_then(|state| state.bundle_artifact_id.as_deref())
                .and_then(|id| store.get_bundle_artifact(id).ok().flatten());
            let draft_pr = implementation_artifacts
                .first()
                .and_then(|artifact| {
                    store
                        .get_draft_pr_for_implementation_artifact(&artifact.artifact_id)
                        .ok()
                        .flatten()
                })
                .or_else(|| {
                    publication.as_ref().and_then(|intent| {
                        store
                            .get_draft_pr_for_intent(&intent.intent_id)
                            .ok()
                            .flatten()
                    })
                });
            attach_implementation_http_response(
                &mut response,
                implementation_state.as_ref(),
                implementation_artifacts.first(),
                publication.as_ref(),
                bundle.as_ref(),
                draft_pr.as_ref(),
            );
            Ok(Some(response))
        })
        .map_err(|err| err.to_string())
    }
}

impl FactoryRunStore for SharedFactoryStore {
    fn claim_attempt(&mut self, request: ClaimAttemptRequest) -> Result<StageRunRecord> {
        self.with_store_mut(|store| store.claim_attempt(request))
    }

    fn store_artifact(&mut self, request: StoreArtifactRequest) -> Result<ArtifactRecord> {
        self.with_store_mut(|store| store.store_artifact(request))
    }

    fn create_publication_intent(
        &mut self,
        request: CreatePublicationIntentRequest,
    ) -> Result<PublicationIntentRecord> {
        self.with_store_mut(|store| store.create_publication_intent(request))
    }

    fn upsert_factory_run(&mut self, request: UpsertFactoryRunRequest) -> Result<FactoryRunRecord> {
        self.with_store_mut(|store| store.upsert_factory_run(request))
    }

    fn mark_run_status(&mut self, run_id: &str, status: FactoryRunStatus) -> Result<()> {
        self.with_store_mut(|store| store.mark_run_status(run_id, status))
    }

    fn fail_attempt(&mut self, stage_run_id: &str, error: FactoryError) -> Result<StageRunRecord> {
        self.with_store_mut(|store| store.fail_attempt(stage_run_id, error))
    }

    fn get_run_by_id(&self, run_id: &str) -> Result<Option<FactoryRunRecord>> {
        self.with_store(|store| store.get_run_by_id(run_id))
    }

    fn get_run_by_issue(
        &self,
        forge_host: &str,
        repository: &str,
        issue_id: &str,
    ) -> Result<Option<FactoryRunRecord>> {
        self.with_store(|store| store.get_run_by_issue(forge_host, repository, issue_id))
    }

    fn get_run_by_issue_identifier(
        &self,
        issue_identifier: &str,
    ) -> Result<Option<FactoryRunRecord>> {
        self.with_store(|store| store.get_run_by_issue_identifier(issue_identifier))
    }

    fn get_stage_run(&self, stage_run_id: &str) -> Result<Option<StageRunRecord>> {
        self.with_store(|store| store.get_stage_run(stage_run_id))
    }

    fn list_stage_runs(&self, run_id: &str) -> Result<Vec<StageRunRecord>> {
        self.with_store(|store| store.list_stage_runs(run_id))
    }

    fn get_artifact_by_id(&self, artifact_id: &str) -> Result<Option<ArtifactRecord>> {
        self.with_store(|store| store.get_artifact_by_id(artifact_id))
    }

    fn get_artifact_for_revision(
        &self,
        run_id: &str,
        issue_revision: &str,
        configuration_revision: &str,
    ) -> Result<Option<ArtifactRecord>> {
        self.with_store(|store| {
            store.get_artifact_for_revision(run_id, issue_revision, configuration_revision)
        })
    }

    fn get_latest_artifact(&self, run_id: &str) -> Result<Option<ArtifactRecord>> {
        self.with_store(|store| store.get_latest_artifact(run_id))
    }

    fn get_publication_intent(&self, intent_id: &str) -> Result<Option<PublicationIntentRecord>> {
        self.with_store(|store| store.get_publication_intent(intent_id))
    }

    fn get_latest_publication(&self, run_id: &str) -> Result<Option<PublicationIntentRecord>> {
        self.with_store(|store| store.get_latest_publication(run_id))
    }

    fn list_pending_intents(&self, limit: usize) -> Result<Vec<PublicationIntentRecord>> {
        self.with_store(|store| store.list_pending_intents(limit))
    }

    fn list_pending_automatic_dispatch_guards(&self) -> Result<Vec<PendingAutomaticDispatchGuard>> {
        self.with_store(|store| store.list_pending_automatic_dispatch_guards())
    }

    fn list_intents_for_run(&self, run_id: &str) -> Result<Vec<PublicationIntentRecord>> {
        self.with_store(|store| store.list_intents_for_run(run_id))
    }

    fn list_verified_comment_identities(&self, run_id: &str) -> Result<Vec<StoredCommentIdentity>> {
        self.with_store(|store| store.list_verified_comment_identities(run_id))
    }

    fn list_stage_attempts_for_revision(
        &self,
        stage: &str,
        run_id: &str,
        issue_revision: &str,
        configuration_revision: &str,
    ) -> Result<Vec<StageRunRecord>> {
        self.with_store(|store| {
            store.list_stage_attempts_for_revision(
                stage,
                run_id,
                issue_revision,
                configuration_revision,
            )
        })
    }

    fn update_publication_step(
        &mut self,
        intent_id: &str,
        completed_step: &str,
        status: PublicationStatus,
        error: Option<FactoryError>,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.update_publication_step(intent_id, completed_step, status, error)
        })
    }

    fn record_publication_step(
        &mut self,
        intent_id: &str,
        completed_step: &str,
        status: PublicationStatus,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.record_publication_step(intent_id, completed_step, status, expected_projection)
        })
    }

    fn set_publication_baseline(
        &mut self,
        intent_id: &str,
        observed_baseline: &serde_json::Value,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.set_publication_baseline(intent_id, observed_baseline, expected_projection)
        })
    }

    fn set_publication_comment(
        &mut self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.set_publication_comment(intent_id, comment_id, publisher_login)
        })
    }

    fn record_event(&mut self, event: FactoryEventRecord) -> Result<()> {
        self.with_store_mut(|store| store.record_event(event))
    }

    fn renew_lease(&mut self, stage_run_id: &str, owner_instance: &str) -> Result<bool> {
        self.with_store_mut(|store| store.renew_lease(stage_run_id, owner_instance))
    }

    fn interrupt_stale_attempts(&mut self) -> Result<u64> {
        self.with_store_mut(|store| store.interrupt_stale_attempts())
    }

    fn interrupt_attempt(&mut self, stage_run_id: &str, owner_instance: &str) -> Result<bool> {
        self.with_store_mut(|store| store.interrupt_attempt(stage_run_id, owner_instance))
    }

    fn record_attempt_process(
        &mut self,
        request: crate::triage::store::RecordAttemptProcessRequest,
    ) -> Result<()> {
        self.with_store_mut(|store| store.record_attempt_process(request.clone()))
    }

    fn list_recoverable_attempts(&self) -> Result<Vec<crate::triage::store::RecoverableAttempt>> {
        self.with_store(|store| store.list_recoverable_attempts())
    }

    fn clear_attempt_process(&mut self, stage_run_id: &str) -> Result<()> {
        self.with_store_mut(|store| store.clear_attempt_process(stage_run_id))
    }

    fn list_correction_candidates(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::triage::store::CorrectionCandidate>> {
        self.with_store(|store| store.list_correction_candidates(limit))
    }

    fn record_route_observation(
        &mut self,
        artifact_id: &str,
        kind: crate::triage::store::RouteObservationKind,
        value: &str,
    ) -> Result<bool> {
        self.with_store_mut(|store| store.record_route_observation(artifact_id, kind, value))
    }

    fn triage_metrics(&self) -> Result<crate::triage::domain::TriageMetricsAggregate> {
        self.with_store(|store| store.triage_metrics())
    }
}

impl FactoryRunQuery for SharedFactoryStore {
    fn get_run(&self, run_id: &str) -> std::result::Result<Option<FactoryRunHttpResponse>, String> {
        self.load_run_response(|store| store.get_run_by_id(run_id))
    }

    fn get_run_by_issue(
        &self,
        issue_identifier: &str,
    ) -> std::result::Result<Option<FactoryRunHttpResponse>, String> {
        self.load_run_response(|store| store.get_run_by_issue_identifier(issue_identifier))
    }

    fn triage_metrics(&self) -> std::result::Result<FactoryRunMetricsHttpResponse, String> {
        self.with_store(|store| store.triage_metrics())
            .map(factory_run_metrics_http_response)
            .map_err(|err| err.to_string())
    }

    fn spec_metrics(&self) -> std::result::Result<SpecRunMetricsHttpResponse, String> {
        self.with_store(|store| store.spec_metrics())
            .map(spec_run_metrics_http_response)
            .map_err(|err| err.to_string())
    }

    fn implementation_metrics(
        &self,
    ) -> std::result::Result<ImplementationRunMetricsHttpResponse, String> {
        self.with_store(|store| store.implementation_metrics())
            .map(implementation_run_metrics_http_response)
            .map_err(|err| err.to_string())
    }

    fn get_artifact(
        &self,
        run_id: &str,
        artifact_id: &str,
    ) -> std::result::Result<Option<FactoryArtifactHttpResponse>, String> {
        self.with_store(|store| {
            let Some(artifact) = store.get_spec_artifact(artifact_id)? else {
                return Ok(None);
            };
            if artifact.run_id != run_id {
                return Ok(None);
            }
            let attempt = store
                .get_stage_run(&artifact.stage_run_id)?
                .ok_or_else(|| {
                    SymphonyError::StorageError(format!(
                        "stage run {} for spec artifact is missing",
                        artifact.stage_run_id
                    ))
                })?;
            Ok(Some(FactoryArtifactHttpResponse {
                artifact_id: artifact.artifact_id,
                run_id: artifact.run_id,
                stage_run_id: artifact.stage_run_id,
                kind: "spec".to_string(),
                version: Some(artifact.version),
                attempt: attempt.attempt,
                received_at: artifact.received_at,
                artifact: serde_json::to_value(artifact.artifact).map_err(|error| {
                    SymphonyError::StorageError(format!(
                        "could not serialize spec artifact: {error}"
                    ))
                })?,
            }))
        })
        .map_err(|err| err.to_string())
    }
}

pub struct EventHubEmitter {
    hub: EventHub,
}

impl EventHubEmitter {
    pub fn new(hub: EventHub) -> Self {
        Self { hub }
    }
}

impl EventEmitter for EventHubEmitter {
    fn emit_triage_event(&self, event_name: &str, issue: Option<&str>, payload: serde_json::Value) {
        let severity = if event_name.contains("failed")
            || event_name.contains("conflict")
            || event_name.contains("blocked")
        {
            EventSeverity::Warn
        } else {
            EventSeverity::Info
        };
        self.hub.publish(
            EventKind::Triage,
            severity,
            issue.map(str::to_string),
            event_name,
            payload,
        );
    }
}

/// Owns the GitHub-backed triage coordinator for the orchestrator poll loop.
pub struct TriageRuntime {
    coordinator: Option<TriageCoordinator<SharedFactoryStore, GithubTriageIntake, GithubClient>>,
    spec_coordinator: Option<SpecCoordinator<GithubTriageIntake, GithubClient>>,
    implementation_coordinator:
        Option<ImplementationCoordinator<GithubClient, GithubClient, LiveImplementationHarness>>,
    store: SharedFactoryStore,
    sessions: Arc<Mutex<crate::domain::TriageSessionRegistry>>,
}

impl TriageRuntime {
    /// Open the durable factory store for dispatch-guard reads when triage intake
    /// is disabled. Returns `Ok(None)` when triage is enabled (the full runtime
    /// owns the store), the tracker is not GitHub, or no existing DB is present.
    pub fn try_open_dispatch_guard_store(
        config: &ServiceConfig,
    ) -> Result<Option<SharedFactoryStore>> {
        if config.triage.enabled || config.spec.enabled || config.implementation.enabled {
            return Ok(None);
        }
        if !matches!(config.tracker.kind.as_deref(), Some("github")) {
            return Ok(None);
        }
        let Some(owner) = config
            .tracker
            .repo_owner
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        let Some(repo) = config
            .tracker
            .repo_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        let forge_host = forge_host_from_endpoint(&config.tracker.endpoint);
        let storage_path = resolve_storage_path(&config.storage, &forge_host, owner, repo);
        if !storage_path.exists() {
            return Ok(None);
        }
        tracing::info!(
            event = "triage_dispatch_guard_store_opened",
            path = %storage_path_for_log(&storage_path),
            "opened durable triage store for dispatch guards while triage.enabled=false"
        );
        Ok(Some(SharedFactoryStore::open(
            &storage_path,
            config.storage.busy_timeout_ms,
        )?))
    }

    /// Start triage when `triage.enabled` is true. Returns `Ok(None)` when disabled.
    pub fn try_start(
        config: &ServiceConfig,
        workflow_path: &Path,
        event_hub: Option<EventHub>,
    ) -> Result<Option<Self>> {
        if !config.triage.enabled && !config.spec.enabled && !config.implementation.enabled {
            return Ok(None);
        }

        if !matches!(config.tracker.kind.as_deref(), Some("github")) {
            return Err(SymphonyError::TriageError(
                "factory stages require tracker.kind=github".to_string(),
            ));
        }

        let owner = config
            .tracker
            .repo_owner
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                SymphonyError::InvalidWorkflowConfig(
                    "tracker.repo_owner is required for factory stages".to_string(),
                )
            })?;
        let repo = config
            .tracker
            .repo_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                SymphonyError::InvalidWorkflowConfig(
                    "tracker.repo_name is required for factory stages".to_string(),
                )
            })?;
        let project_number = config.tracker.github_project_number.ok_or_else(|| {
            SymphonyError::InvalidWorkflowConfig(
                "tracker.github_project_number is required for factory stages".to_string(),
            )
        })?;

        let resolved = crate::github::auth::resolve_github_token(&config.tracker)
            .ok_or(SymphonyError::MissingGithubApiToken)?;
        let token = resolved.token;

        let forge_host = forge_host_from_endpoint(&config.tracker.endpoint);
        let repository = format!("{owner}/{repo}");
        let storage_path = resolve_storage_path(&config.storage, &forge_host, owner, repo);
        tracing::info!(
            event = "factory_storage_resolved",
            path = %storage_path_for_log(&storage_path),
            triage_enabled = config.triage.enabled,
            spec_enabled = config.spec.enabled,
            implementation_enabled = config.implementation.enabled,
            "resolved factory SQLite storage path"
        );

        let store = SharedFactoryStore::open(&storage_path, config.storage.busy_timeout_ms)?;
        let label_prefix = config
            .tracker
            .label_prefix
            .clone()
            .unwrap_or_else(|| "kata".to_string());
        let endpoint = config.tracker.endpoint.trim();
        let client = if endpoint.is_empty() || endpoint == "https://api.github.com" {
            GithubClient::new(token, owner, repo, label_prefix)
        } else {
            GithubClient::with_base_url(token, owner, repo, label_prefix, endpoint)
        };

        let projects = ProjectsV2Client::new(client.clone());
        let managed_labels = config
            .triage
            .routes
            .managed_labels()
            .into_iter()
            .chain(config.spec.managed_labels())
            .collect();
        let intake = GithubTriageIntake::new(
            client.clone(),
            projects.clone(),
            owner,
            repo,
            project_number,
            forge_host.clone(),
            managed_labels,
        );
        let routing = GithubTriageRouting::new(
            client.clone(),
            projects,
            owner,
            repo,
            project_number,
            config
                .triage
                .max_intake_pages
                .max(config.spec.max_intake_pages),
        );

        let workflow_dir = workflow_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| Path::new(".").to_path_buf());
        let owner_instance = format!("symphony-{}-{}", std::process::id(), uuid::Uuid::new_v4());
        let project_display_name = format!("#{project_number}");
        let sessions = Arc::new(Mutex::new(crate::domain::TriageSessionRegistry::default()));
        let emitter =
            event_hub.map(|hub| Arc::new(EventHubEmitter::new(hub)) as Arc<dyn EventEmitter>);

        let coordinator = if config.triage.enabled {
            let mut coordinator = TriageCoordinator::new(
                store.clone(),
                intake.clone(),
                client.clone(),
                TriageCoordinatorConfig {
                    forge_host: forge_host.clone(),
                    repository: repository.clone(),
                    owner_instance: owner_instance.clone(),
                    workflow_dir: workflow_dir.clone(),
                    project_display_name,
                },
            )
            .with_routing(routing.clone())
            .with_session_registry(sessions.clone());
            if let Some(events) = emitter.clone() {
                coordinator = coordinator.with_events(events);
            }
            Some(coordinator)
        } else {
            None
        };

        let spec_coordinator = if config.spec.enabled {
            let mut coordinator = SpecCoordinator::new(
                store.clone(),
                intake,
                client.clone(),
                routing.clone(),
                SpecCoordinatorConfig {
                    forge_host: forge_host.clone(),
                    repository: repository.clone(),
                    owner_instance: owner_instance.clone(),
                    workflow_dir: workflow_dir.clone(),
                },
            );
            if let Some(events) = emitter.clone() {
                coordinator = coordinator.with_events(events);
            }
            Some(coordinator)
        } else {
            None
        };

        // Always construct when we have a factory store so reconciliation continues
        // even if implementation.enabled is false (disabling stops new claims only).
        let mut implementation_coordinator = ImplementationCoordinator::new(
            store.clone(),
            client.clone(),
            client.clone(),
            ImplementationCoordinatorConfig {
                forge_host,
                repository,
                owner_instance,
                workflow_dir,
                storage_path: storage_path.clone(),
            },
        )
        .with_routing(routing.clone())
        .with_pulls(client);
        if let Some(events) = emitter {
            implementation_coordinator = implementation_coordinator.with_events(events);
        }

        Ok(Some(Self {
            coordinator,
            spec_coordinator,
            implementation_coordinator: Some(implementation_coordinator),
            store,
            sessions,
        }))
    }

    pub fn store(&self) -> SharedFactoryStore {
        self.store.clone()
    }

    pub fn sessions(&self) -> Arc<Mutex<crate::domain::TriageSessionRegistry>> {
        self.sessions.clone()
    }

    /// Issue IDs / intake labels that must not enter implementation dispatch
    /// while automatic publication is still nonterminal.
    pub fn pending_automatic_dispatch_guards(&self) -> Result<Vec<PendingAutomaticDispatchGuard>> {
        self.store.pending_automatic_dispatch_guards()
    }

    pub async fn poll(&mut self, config: &ServiceConfig) -> Result<TriagePollSummary> {
        let triage = if let Some(coordinator) = self.coordinator.as_mut() {
            coordinator.poll_once(config).await?
        } else {
            TriagePollSummary::default()
        };
        if let Some(coordinator) = self.spec_coordinator.as_mut() {
            // A spec-stage failure must not discard the triage summary or surface as
            // `triage_poll_failed`. Report it under its own event so the failing stage
            // is identifiable and the other stage keeps making progress.
            match coordinator.poll_once(config).await {
                Ok(summary) => {
                    if summary.spec_enabled || summary.issues_seen > 0 {
                        tracing::info!(
                            event = "spec_poll_completed",
                            enabled = summary.spec_enabled,
                            issues_seen = summary.issues_seen,
                            attempts_started = summary.attempts_started,
                            attempts_completed = summary.attempts_completed,
                            attempts_failed = summary.attempts_failed,
                            published = summary.published,
                            approved = summary.approved,
                            revisions_requested = summary.revisions_requested,
                            "spec poll completed"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        event = "spec_poll_failed",
                        error = %error,
                        "spec poll failed; continuing orchestrator loop"
                    );
                }
            }
        }
        if let Some(coordinator) = self.implementation_coordinator.as_mut() {
            match coordinator.poll_once(config).await {
                Ok(summary) => {
                    if summary.implementation_enabled
                        || summary.candidates_seen > 0
                        || summary.attempts_started > 0
                    {
                        tracing::info!(
                            event = "implementation_poll_completed",
                            enabled = summary.implementation_enabled,
                            candidates_seen = summary.candidates_seen,
                            attempts_started = summary.attempts_started,
                            attempts_completed = summary.attempts_completed,
                            attempts_failed = summary.attempts_failed,
                            stale_skipped = summary.stale_skipped,
                            preview_published = summary.preview_published,
                            awaiting_human = summary.awaiting_human,
                            "implementation poll completed"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        event = "implementation_poll_failed",
                        error = %error,
                        "implementation poll failed; continuing orchestrator loop"
                    );
                }
            }
        }
        Ok(triage)
    }
}
