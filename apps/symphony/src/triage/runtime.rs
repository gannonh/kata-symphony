//! Runtime wiring for triage: shared store, HTTP query, and event emission.

use chrono::Utc;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::domain::{
    EventKind, EventSeverity, FactorySessionInfo, FactorySessionRegistry, FactorySnapshot,
    ServiceConfig,
};
use crate::error::{Result, SymphonyError};
use crate::event_stream::EventHub;
use crate::github::client::GithubClient;
use crate::github::projects_v2::ProjectsV2Client;
use crate::http_server::{
    attach_implementation_http_response, attach_review_http_response_with_attempts,
    attach_spec_http_response, factory_run_http_response, factory_run_metrics_http_response,
    implementation_run_metrics_http_response, review_run_metrics_http_response,
    spec_run_metrics_http_response, FactoryArtifactHttpResponse, FactoryRunHttpResponse,
    FactoryRunMetricsHttpResponse, FactoryRunQuery, ImplementationRunMetricsHttpResponse,
    ReviewRunMetricsHttpResponse, SpecRunMetricsHttpResponse,
};
use crate::implementation::coordinator::{
    ImplementationCoordinator, ImplementationCoordinatorConfig,
};
use crate::implementation::runner::LiveImplementationHarness;
use crate::review::coordinator::{ReviewCoordinator, ReviewCoordinatorConfig};
use crate::review::worker::LiveReviewWorker;
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
    StoredCommentIdentity, UpdateReviewAttemptRequest, UpsertFactoryRunRequest,
    UpsertImplementationStateRequest,
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

    pub fn list_blocked_implementation_publications(
        &self,
    ) -> Result<Vec<crate::implementation::domain::ImplementationPublicationIntent>> {
        self.with_store(|store| store.list_blocked_implementation_publications())
    }

    /// Operator recovery for a blocked publication intent. Records the
    /// intervention on the run timeline so the reset is auditable alongside the
    /// failures that caused it.
    pub fn reset_blocked_implementation_publication(
        &self,
        intent_id: &str,
        operator: &str,
    ) -> Result<crate::implementation::domain::ImplementationPublicationIntent> {
        self.with_store_mut(|store| {
            store.reset_blocked_implementation_publication(intent_id, operator)
        })
    }

    pub fn set_implementation_publication_waiting(
        &self,
        intent_id: &str,
        status: crate::triage::domain::PublicationStatus,
        error: crate::triage::domain::FactoryError,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.set_implementation_publication_waiting(intent_id, status, error)
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

    pub fn claim_review_attempt(&self, request: ClaimAttemptRequest) -> Result<StageRunRecord> {
        self.with_store_mut(|store| {
            store.claim_stage_attempt(crate::review::domain::REVIEW_STAGE_NAME, request)
        })
    }

    pub fn list_a4_eligible_review_runs(
        &self,
        max_attempts: u32,
    ) -> Result<Vec<crate::triage::store::A4EligibleReviewRun>> {
        self.with_store(|store| store.list_a4_eligible_review_runs(max_attempts))
    }

    pub fn review_artifact_exists(
        &self,
        run_id: &str,
        head_sha: &str,
        base_sha: &str,
    ) -> Result<bool> {
        self.with_store(|store| store.review_artifact_exists(run_id, head_sha, base_sha))
    }

    pub fn count_review_attempt_failures_for_head(
        &self,
        run_id: &str,
        reviewed_head_sha: &str,
        base_sha: &str,
    ) -> Result<u32> {
        self.with_store(|store| {
            store.count_review_attempt_failures_for_head(run_id, reviewed_head_sha, base_sha)
        })
    }

    pub fn get_orphaned_review_artifact(
        &self,
        run_id: &str,
        head_sha: &str,
        base_sha: &str,
    ) -> Result<Option<crate::review::domain::ReviewFindingsArtifactRecord>> {
        self.with_store(|store| store.get_orphaned_review_artifact(run_id, head_sha, base_sha))
    }

    pub fn store_review_attempt_inputs(
        &self,
        request: crate::triage::store::StoreReviewAttemptRequest,
    ) -> Result<crate::review::domain::ReviewAttemptRecord> {
        self.with_store_mut(|store| store.store_review_attempt_inputs(request))
    }

    pub fn update_review_attempt(&self, request: UpdateReviewAttemptRequest<'_>) -> Result<()> {
        self.with_store_mut(|store| store.update_review_attempt(request))
    }

    pub fn interrupt_review_attempt(
        &self,
        stage_run_id: &str,
        owner_instance: &str,
    ) -> Result<bool> {
        self.with_store_mut(|store| store.interrupt_attempt(stage_run_id, owner_instance))
    }

    pub fn store_review_artifact(
        &self,
        request: crate::triage::store::StoreReviewArtifactRequest,
    ) -> Result<crate::review::domain::ReviewFindingsArtifactRecord> {
        self.with_store_mut(|store| store.store_review_artifact(request))
    }

    pub fn get_review_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<crate::review::domain::ReviewFindingsArtifactRecord>> {
        self.with_store(|store| store.get_review_artifact(artifact_id))
    }

    // ── A5 verification wrappers ──────────────────────────────────────────

    pub fn claim_verification_attempt(&self, request: ClaimAttemptRequest) -> Result<StageRunRecord> {
        self.with_store_mut(|store| {
            store.claim_stage_attempt(crate::verification::domain::VERIFICATION_STAGE_NAME, request)
        })
    }

    pub fn list_a5_eligible_verification_runs(
        &self,
    ) -> Result<Vec<crate::triage::store::A5EligibleVerificationRun>> {
        self.with_store(|store| store.list_a5_eligible_verification_runs())
    }

    pub fn store_verification_attempt_inputs(
        &self,
        request: crate::triage::store::StoreVerificationAttemptRequest,
    ) -> Result<crate::verification::domain::VerificationAttemptRecord> {
        self.with_store_mut(|store| store.store_verification_attempt_inputs(request))
    }

    pub fn get_verification_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<Option<crate::verification::domain::VerificationAttemptRecord>> {
        self.with_store(|store| store.get_verification_attempt(attempt_id))
    }

    pub fn list_verification_attempts(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::verification::domain::VerificationAttemptRecord>> {
        self.with_store(|store| store.list_verification_attempts(run_id))
    }

    pub fn list_running_verification_attempts(
        &self,
    ) -> Result<Vec<crate::verification::domain::VerificationAttemptRecord>> {
        self.with_store(|store| store.list_running_verification_attempts())
    }

    pub fn update_verification_attempt(
        &self,
        request: crate::triage::store::UpdateVerificationAttemptRequest<'_>,
    ) -> Result<()> {
        self.with_store_mut(|store| store.update_verification_attempt(request))
    }

    pub fn record_verification_command_launch(
        &self,
        run_id: &str,
        attempt_id: &str,
        ordinal: u32,
        name: &str,
        kind: crate::verification::domain::VerificationCommandKind,
        configuration_revision: &str,
        command_sha256: &str,
        execution_profile: &str,
        launch_nonce: &str,
    ) -> Result<String> {
        self.with_store_mut(|store| {
            store.record_verification_command_launch(
                run_id,
                attempt_id,
                ordinal,
                name,
                kind,
                configuration_revision,
                command_sha256,
                execution_profile,
                launch_nonce,
            )
        })
    }

    pub fn cas_verification_launch_identity(
        &self,
        command_run_id: &str,
        launch_nonce: &str,
        identity: &crate::triage::process_identity::ProcessIdentity,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.cas_verification_launch_identity(command_run_id, launch_nonce, identity)
        })
    }

    pub fn cas_verification_container(
        &self,
        command_run_id: &str,
        launch_nonce: &str,
        container_id: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.cas_verification_container(command_run_id, launch_nonce, container_id)
        })
    }

    pub fn complete_verification_command(
        &self,
        request: crate::triage::store::CompleteVerificationCommandRequest<'_>,
    ) -> Result<()> {
        self.with_store_mut(|store| store.complete_verification_command(request))
    }

    pub fn mark_verification_command_not_run(&self, command_run_id: &str) -> Result<()> {
        self.with_store_mut(|store| store.mark_verification_command_not_run(command_run_id))
    }

    pub fn list_verification_command_runs(
        &self,
        attempt_id: &str,
    ) -> Result<Vec<crate::verification::domain::VerificationCommandRunRecord>> {
        self.with_store(|store| store.list_verification_command_runs(attempt_id))
    }

    pub fn store_verification_evidence(
        &self,
        records: &[crate::verification::domain::VerificationEvidenceRecord],
    ) -> Result<()> {
        self.with_store_mut(|store| store.store_verification_evidence(records))
    }

    pub fn list_verification_evidence(
        &self,
        attempt_id: &str,
    ) -> Result<Vec<crate::verification::domain::VerificationEvidenceRecord>> {
        self.with_store(|store| store.list_verification_evidence(attempt_id))
    }

    pub fn store_verification_gate(
        &self,
        record: &crate::verification::domain::VerificationGateRecord,
    ) -> Result<()> {
        self.with_store_mut(|store| store.store_verification_gate(record))
    }

    pub fn get_verification_gate(
        &self,
        attempt_id: &str,
    ) -> Result<Option<crate::verification::domain::VerificationGateRecord>> {
        self.with_store(|store| store.get_verification_gate(attempt_id))
    }

    pub fn create_verification_publication_intent(
        &self,
        run_id: &str,
        attempt_id: &str,
        kind: &str,
    ) -> Result<crate::verification::domain::VerificationPublicationIntent> {
        self.with_store_mut(|store| {
            store.create_verification_publication_intent(run_id, attempt_id, kind)
        })
    }

    pub fn list_verification_publications_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::verification::domain::VerificationPublicationIntent>> {
        self.with_store(|store| store.list_verification_publications_for_run(run_id))
    }

    pub fn mark_verification_publication_applied(
        &self,
        intent_id: &str,
        comment_id: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.mark_verification_publication_applied(intent_id, comment_id)
        })
    }

    pub fn complete_verification_stage_run(
        &self,
        stage_run_id: &str,
        usage: crate::triage::domain::StageUsage,
    ) -> Result<()> {
        self.with_store_mut(|store| store.complete_verification_stage_run(stage_run_id, usage))
    }

    pub fn interrupt_verification_stage_run(
        &self,
        stage_run_id: &str,
        error: &FactoryError,
    ) -> Result<()> {
        self.with_store_mut(|store| store.interrupt_verification_stage_run(stage_run_id, error))
    }

    pub fn verification_metrics(
        &self,
    ) -> Result<crate::verification::domain::VerificationMetricsAggregate> {
        self.with_store(|store| store.verification_metrics())
    }

    pub fn list_review_finding_records_for_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<crate::review::domain::ReviewFindingRecord>> {
        self.with_store(|store| store.list_review_finding_records_for_artifact(artifact_id))
    }

    pub fn create_review_publication_intent(
        &self,
        run_id: &str,
        artifact_id: &str,
        kind: &str,
        desired_effects: &serde_json::Value,
    ) -> Result<crate::review::domain::ReviewPublicationIntent> {
        self.with_store_mut(|store| {
            store.create_review_publication_intent(run_id, artifact_id, kind, desired_effects)
        })
    }

    pub fn claim_review_publication(
        &self,
        intent_id: &str,
        owner: &str,
        lease_seconds: i64,
    ) -> Result<bool> {
        self.with_store_mut(|store| store.claim_review_publication(intent_id, owner, lease_seconds))
    }

    pub fn list_pending_review_publications(
        &self,
    ) -> Result<Vec<crate::review::domain::ReviewPublicationIntent>> {
        self.with_store(|store| store.list_pending_review_publications())
    }

    pub fn list_blocked_review_publications(
        &self,
    ) -> Result<Vec<crate::review::domain::ReviewPublicationIntent>> {
        self.with_store(|store| store.list_blocked_review_publications())
    }

    pub fn reset_blocked_review_publication(
        &self,
        intent_id: &str,
        operator: &str,
    ) -> Result<crate::review::domain::ReviewPublicationIntent> {
        self.with_store_mut(|store| store.reset_blocked_review_publication(intent_id, operator))
    }

    pub fn get_review_publication_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<crate::review::domain::ReviewPublicationIntent>> {
        self.with_store(|store| store.get_review_publication_intent(intent_id))
    }

    pub fn list_review_publications_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::review::domain::ReviewPublicationIntent>> {
        self.with_store(|store| store.list_review_publications_for_run(run_id))
    }

    #[cfg(test)]
    pub fn complete_review_publication(&self, intent_id: &str, step: &str) -> Result<()> {
        self.with_store_mut(|store| store.complete_review_publication(intent_id, step))
    }

    pub fn complete_review_publication_owned(
        &self,
        intent_id: &str,
        owner: &str,
        step: &str,
    ) -> Result<bool> {
        self.with_store_mut(|store| store.complete_review_publication_owned(intent_id, owner, step))
    }

    #[cfg(test)]
    pub fn record_review_publication_step(
        &self,
        intent_id: &str,
        step: &str,
        status: PublicationStatus,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.record_review_publication_step(intent_id, step, status, expected_projection)
        })
    }

    pub fn record_review_publication_step_owned(
        &self,
        intent_id: &str,
        owner: &str,
        step: &str,
        status: PublicationStatus,
        expected_projection: &serde_json::Value,
    ) -> Result<bool> {
        self.with_store_mut(|store| {
            store.record_review_publication_step_owned(
                intent_id,
                owner,
                step,
                status,
                expected_projection,
            )
        })
    }

    #[cfg(test)]
    pub fn bind_review_publication_review(
        &self,
        intent_id: &str,
        review_id: &str,
        review_url: &str,
        publisher_login: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.bind_review_publication_review(intent_id, review_id, review_url, publisher_login)
        })
    }

    pub fn bind_review_publication_review_owned(
        &self,
        intent_id: &str,
        owner: &str,
        review_id: &str,
        review_url: &str,
        publisher_login: &str,
    ) -> Result<bool> {
        self.with_store_mut(|store| {
            store.bind_review_publication_review_owned(
                intent_id,
                owner,
                review_id,
                review_url,
                publisher_login,
            )
        })
    }

    #[cfg(test)]
    pub fn set_review_publication_route_state(
        &self,
        intent_id: &str,
        route_state: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.set_review_publication_route_state(intent_id, route_state)
        })
    }

    pub fn set_review_publication_route_state_owned(
        &self,
        intent_id: &str,
        owner: &str,
        route_state: &str,
    ) -> Result<bool> {
        self.with_store_mut(|store| {
            store.set_review_publication_route_state_owned(intent_id, owner, route_state)
        })
    }

    #[cfg(test)]
    pub fn bind_review_publication_comment(
        &self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()> {
        self.with_store_mut(|store| {
            store.bind_review_publication_comment(intent_id, comment_id, publisher_login)
        })
    }

    pub fn bind_review_publication_comment_owned(
        &self,
        intent_id: &str,
        owner: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<bool> {
        self.with_store_mut(|store| {
            store.bind_review_publication_comment_owned(
                intent_id,
                owner,
                comment_id,
                publisher_login,
            )
        })
    }

    #[cfg(test)]
    pub fn clear_review_publication_comment(&self, intent_id: &str) -> Result<()> {
        self.with_store_mut(|store| store.clear_review_publication_comment(intent_id))
    }

    pub fn clear_review_publication_comment_owned(
        &self,
        intent_id: &str,
        owner: &str,
    ) -> Result<bool> {
        self.with_store_mut(|store| store.clear_review_publication_comment_owned(intent_id, owner))
    }

    pub fn clear_review_publication_lease(&self, intent_id: &str, owner: &str) -> Result<()> {
        self.with_store_mut(|store| store.clear_review_publication_lease(intent_id, owner))
    }

    pub fn renew_review_publication_lease(
        &self,
        intent_id: &str,
        owner: &str,
        lease_seconds: i64,
    ) -> Result<bool> {
        self.with_store_mut(|store| {
            store.renew_review_publication_lease(intent_id, owner, lease_seconds)
        })
    }

    #[cfg(test)]
    pub fn set_review_publication_error(
        &self,
        intent_id: &str,
        status: PublicationStatus,
        error: FactoryError,
    ) -> Result<()> {
        self.with_store_mut(|store| store.set_review_publication_error(intent_id, status, error))
    }

    pub fn set_review_publication_error_owned(
        &self,
        intent_id: &str,
        owner: &str,
        status: PublicationStatus,
        error: FactoryError,
    ) -> Result<bool> {
        self.with_store_mut(|store| {
            store.set_review_publication_error_owned(intent_id, owner, status, error)
        })
    }

    pub fn supersede_review_publication_owned(
        &self,
        intent_id: &str,
        owner: &str,
        error: FactoryError,
    ) -> Result<bool> {
        self.with_store_mut(|store| {
            store.supersede_review_publication_owned(intent_id, owner, error)
        })
    }

    #[cfg(test)]
    pub fn supersede_review_publication(
        &self,
        intent_id: &str,
        error: FactoryError,
    ) -> Result<bool> {
        self.with_store_mut(|store| store.supersede_review_publication(intent_id, error))
    }

    pub fn review_metrics(&self) -> Result<crate::review::domain::ReviewMetricsAggregate> {
        self.with_store(|store| store.review_metrics())
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
            let review_artifacts = store.list_review_artifacts(&run.run_id)?;
            let review_artifact = review_artifacts.first();
            let review_publication = store
                .list_review_publications_for_run(&run.run_id)?
                .into_iter()
                .next();
            attach_review_http_response_with_attempts(
                &mut response,
                review_artifact,
                review_publication.as_ref(),
                &attempts,
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

    fn review_metrics(&self) -> std::result::Result<ReviewRunMetricsHttpResponse, String> {
        self.with_store(|store| store.review_metrics())
            .map(review_run_metrics_http_response)
            .map_err(|err| err.to_string())
    }

    fn blocked_publications(
        &self,
    ) -> std::result::Result<Vec<crate::http_server::BlockedPublicationHttpResponse>, String> {
        let mut intents = self
            .list_blocked_implementation_publications()
            .map(|intents| {
                intents
                    .into_iter()
                    .map(|intent| {
                        (
                            intent.created_at,
                            crate::http_server::BlockedPublicationHttpResponse {
                                intent_id: intent.intent_id,
                                run_id: intent.run_id,
                                kind: intent.kind.as_str().to_string(),
                                retry_count: intent.retry_count,
                                last_step: intent.completed_steps.last().cloned(),
                                error_code: intent
                                    .last_error
                                    .as_ref()
                                    .map(|error| error.code.clone()),
                                error_remediation: intent
                                    .last_error
                                    .as_ref()
                                    .map(|error| error.remediation.clone()),
                                updated_at: intent.updated_at,
                            },
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .map_err(|err| err.to_string())?;
        let review_intents = self
            .list_blocked_review_publications()
            .map(|intents| {
                intents
                    .into_iter()
                    .map(|intent| {
                        (
                            intent.created_at,
                            crate::http_server::BlockedPublicationHttpResponse {
                                intent_id: intent.intent_id,
                                run_id: intent.run_id,
                                kind: intent.kind,
                                retry_count: intent.retry_count,
                                last_step: intent.completed_steps.last().cloned(),
                                error_code: intent
                                    .last_error
                                    .as_ref()
                                    .map(|error| error.code.clone()),
                                error_remediation: intent
                                    .last_error
                                    .as_ref()
                                    .map(|error| error.remediation.clone()),
                                updated_at: intent.updated_at,
                            },
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .map_err(|err| err.to_string())?;
        intents.extend(review_intents);
        intents.sort_by_key(|(created_at, _)| *created_at);
        Ok(intents.into_iter().map(|(_, intent)| intent).collect())
    }

    fn reset_blocked_publication(
        &self,
        intent_id: &str,
        operator: &str,
    ) -> std::result::Result<crate::http_server::BlockedPublicationResetHttpResponse, String> {
        // Intent ids are globally unique, so locating the durable record selects
        // the publication family without changing the HTTP contract.
        if self
            .get_implementation_publication_intent(intent_id)
            .map_err(|err| err.to_string())?
            .is_some()
        {
            return self
                .reset_blocked_implementation_publication(intent_id, operator)
                .map(
                    |intent| crate::http_server::BlockedPublicationResetHttpResponse {
                        intent_id: intent.intent_id,
                        run_id: intent.run_id,
                        status: intent.status.as_str().to_string(),
                        completed_steps: intent.completed_steps,
                    },
                )
                .map_err(|err| err.to_string());
        }
        if self
            .get_review_publication_intent(intent_id)
            .map_err(|err| err.to_string())?
            .is_some()
        {
            return self
                .reset_blocked_review_publication(intent_id, operator)
                .map(
                    |intent| crate::http_server::BlockedPublicationResetHttpResponse {
                        intent_id: intent.intent_id,
                        run_id: intent.run_id,
                        status: intent.status.as_str().to_string(),
                        completed_steps: intent.completed_steps,
                    },
                )
                .map_err(|err| err.to_string());
        }
        Err(format!("publication intent {intent_id} not found"))
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
    store: SharedFactoryStore,
    factory_sessions: Arc<Mutex<FactorySessionRegistry>>,
}

impl EventHubEmitter {
    pub fn new(
        hub: EventHub,
        store: SharedFactoryStore,
        factory_sessions: Arc<Mutex<FactorySessionRegistry>>,
    ) -> Self {
        Self {
            hub,
            store,
            factory_sessions,
        }
    }

    fn update_factory_session(
        &self,
        event_name: &str,
        issue: Option<&str>,
        run_id: Option<&str>,
        stage_run_id: Option<&str>,
        payload: &serde_json::Value,
    ) {
        let Some(stage_run_id) = stage_run_id else {
            return;
        };
        let Ok(Some(stage)) = self.store.get_stage_run(stage_run_id) else {
            return;
        };
        if stage.stage == crate::triage::domain::TRIAGE_STAGE_NAME {
            return;
        }

        let issue_identifier = issue
            .map(str::to_string)
            .or_else(|| {
                run_id.and_then(|id| {
                    self.store
                        .get_run_by_id(id)
                        .ok()
                        .flatten()
                        .map(|run| run.issue_identifier)
                })
            })
            .unwrap_or_else(|| run_id.unwrap_or("-").to_string());
        let message = event_message(payload);
        let total_tokens = event_total_tokens(payload);
        let is_start = matches!(
            event_name,
            "spec_started" | "implementation_started" | "review_started"
        );
        let is_terminal = matches!(
            (stage.stage.as_str(), event_name),
            ("spec", "spec_published" | "spec_failed")
                | (
                    "implementation",
                    "implementation_completed"
                        | "implementation_failed"
                        | "implementation_preview_published"
                )
                | ("review", "review_published" | "review_blocked")
        );
        if is_start && stage.status.is_terminal() {
            return;
        }

        let Ok(mut registry) = self.factory_sessions.lock() else {
            return;
        };
        if registry.has_completed(stage_run_id) {
            return;
        }
        if is_start || !registry.has_session(stage_run_id) {
            registry.begin(FactorySessionInfo {
                stage: stage.stage.clone(),
                issue_identifier,
                run_id: stage.run_id.clone(),
                stage_run_id: stage.stage_run_id.clone(),
                attempt: stage.attempt,
                harness: stage.harness.clone(),
                model: stage.model.clone(),
                started_at: stage.started_at.unwrap_or_else(Utc::now),
                last_activity_at: Some(Utc::now()),
                last_event: Some(event_name.to_string()),
                last_event_message: message.clone(),
                session_id: None,
                turn_count: 0,
                total_tokens: total_tokens.unwrap_or(0),
            });
        }
        registry.update_event(stage_run_id, event_name, message, total_tokens);

        if is_terminal {
            registry.finish(
                stage_run_id,
                stage.status.as_str(),
                stage.usage.input_tokens,
                stage.usage.output_tokens,
                stage.usage.total_tokens,
                stage.error.map(|error| error.remediation),
            );
        }
    }
}

impl EventEmitter for EventHubEmitter {
    fn emit_triage_event(
        &self,
        event_name: &str,
        issue: Option<&str>,
        run_id: Option<&str>,
        stage_run_id: Option<&str>,
        payload: serde_json::Value,
    ) {
        self.update_factory_session(event_name, issue, run_id, stage_run_id, &payload);
        let severity = if event_name.contains("failed")
            || event_name.contains("conflict")
            || event_name.contains("blocked")
        {
            EventSeverity::Warn
        } else {
            EventSeverity::Info
        };
        let display_issue = issue.map(str::to_string).or_else(|| {
            run_id.and_then(|id| {
                self.store
                    .get_run_by_id(id)
                    .ok()
                    .flatten()
                    .map(|run| run.issue_identifier)
            })
        });
        self.hub.publish(
            EventKind::Triage,
            severity,
            display_issue,
            event_name,
            payload,
        );
    }
}

fn event_message(payload: &serde_json::Value) -> Option<String> {
    ["summary", "error", "error_preview", "message", "status"]
        .into_iter()
        .find_map(|key| {
            payload
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

fn event_total_tokens(payload: &serde_json::Value) -> Option<u64> {
    payload
        .get("total_tokens")
        .and_then(|value| value.as_u64())
        .or_else(|| {
            payload
                .get("usage")
                .and_then(|usage| usage.get("total_tokens"))
                .and_then(|value| value.as_u64())
        })
}

/// Owns the GitHub-backed triage coordinator for the orchestrator poll loop.
pub struct TriageRuntime {
    coordinator: Option<TriageCoordinator<SharedFactoryStore, GithubTriageIntake, GithubClient>>,
    spec_coordinator: Option<SpecCoordinator<GithubTriageIntake, GithubClient>>,
    implementation_coordinator:
        Option<ImplementationCoordinator<GithubClient, GithubClient, LiveImplementationHarness>>,
    review_coordinator: Option<ReviewCoordinator<GithubClient, LiveReviewWorker>>,
    verification_coordinator:
        Option<crate::verification::coordinator::VerificationCoordinator<
            GithubClient,
            crate::verification::worker::LiveVerificationWorker,
        >>,
    store: SharedFactoryStore,
    sessions: Arc<Mutex<crate::domain::TriageSessionRegistry>>,
    factory_sessions: Arc<Mutex<FactorySessionRegistry>>,
}

impl TriageRuntime {
    /// Open the durable factory store for dispatch-guard reads when triage intake
    /// is disabled. Returns `Ok(None)` when triage is enabled (the full runtime
    /// owns the store), the tracker is not GitHub, or no existing DB is present.
    pub fn try_open_dispatch_guard_store(
        config: &ServiceConfig,
    ) -> Result<Option<SharedFactoryStore>> {
        if config.triage.enabled
            || config.spec.enabled
            || config.implementation.enabled
            || config.review.enabled
        {
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
        if !config.triage.enabled
            && !config.spec.enabled
            && !config.implementation.enabled
            && !config.review.enabled
        {
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
        let review_projects = projects.clone();
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
        let factory_sessions = Arc::new(Mutex::new(FactorySessionRegistry::default()));
        let emitter = event_hub.map(|hub| {
            Arc::new(EventHubEmitter::new(
                hub,
                store.clone(),
                factory_sessions.clone(),
            )) as Arc<dyn EventEmitter>
        });

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
                forge_host: forge_host.clone(),
                repository: repository.clone(),
                owner_instance: owner_instance.clone(),
                workflow_dir: workflow_dir.clone(),
                storage_path: storage_path.clone(),
            },
        )
        .with_routing(routing.clone())
        .with_pulls(client.clone());
        if let Some(events) = emitter.clone() {
            implementation_coordinator = implementation_coordinator.with_events(events);
        }

        let mut review_coordinator = ReviewCoordinator::new(
            store.clone(),
            client.clone(),
            client.clone(),
            review_projects,
            LiveReviewWorker,
            ReviewCoordinatorConfig {
                forge_host: forge_host.clone(),
                repository: repository.clone(),
                owner_instance: owner_instance.clone(),
                workflow_dir: workflow_dir.clone(),
                project_owner: owner.to_string(),
                project_number,
                max_pages: config
                    .triage
                    .max_intake_pages
                    .max(config.spec.max_intake_pages)
                    .max(1),
            },
        );
        if let Some(events) = emitter.clone() {
            review_coordinator = review_coordinator.with_events(events);
        }

        let workspace_root = crate::verification::coordinator::resolve_workspace_root(config)?;
        let mut verification_coordinator =
            crate::verification::coordinator::VerificationCoordinator::new(
                store.clone(),
                client.clone(),
                client.clone(),
                crate::github::projects_v2::ProjectsV2Client::new(client.clone()),
                crate::verification::worker::LiveVerificationWorker,
                crate::verification::coordinator::VerificationCoordinatorConfig {
                    forge_host: forge_host.clone(),
                    repository: repository.clone(),
                    owner_instance: owner_instance.clone(),
                    workflow_dir: workflow_dir.clone(),
                    project_owner: owner.to_string(),
                    project_number,
                    max_pages: config
                        .triage
                        .max_intake_pages
                        .max(config.spec.max_intake_pages)
                        .max(1),
                    workspace_root,
                },
            );
        if let Some(events) = emitter.clone() {
            verification_coordinator = verification_coordinator.with_events(events);
        }

        Ok(Some(Self {
            coordinator,
            spec_coordinator,
            implementation_coordinator: Some(implementation_coordinator),
            review_coordinator: Some(review_coordinator),
            verification_coordinator: Some(verification_coordinator),
            store,
            sessions,
            factory_sessions,
        }))
    }

    pub fn store(&self) -> SharedFactoryStore {
        self.store.clone()
    }

    pub fn sessions(&self) -> Arc<Mutex<crate::domain::TriageSessionRegistry>> {
        self.sessions.clone()
    }

    pub fn factory_sessions(&self) -> Arc<Mutex<FactorySessionRegistry>> {
        self.factory_sessions.clone()
    }

    pub fn factory_snapshot(&self) -> FactorySnapshot {
        self.factory_sessions
            .lock()
            .map(|registry| registry.snapshot())
            .unwrap_or_default()
    }

    fn reconcile_factory_sessions(&self) {
        let active_stage_runs = self
            .factory_sessions
            .lock()
            .map(|registry| registry.active_stage_run_ids())
            .unwrap_or_default();
        for stage_run_id in active_stage_runs {
            let Ok(Some(stage)) = self.store.get_stage_run(&stage_run_id) else {
                continue;
            };
            if !stage.status.is_terminal() {
                continue;
            }
            if let Ok(mut registry) = self.factory_sessions.lock() {
                registry.finish(
                    &stage_run_id,
                    stage.status.as_str(),
                    stage.usage.input_tokens,
                    stage.usage.output_tokens,
                    stage.usage.total_tokens,
                    stage.error.map(|error| error.remediation),
                );
            }
        }
    }

    /// Issue IDs / intake labels that must not enter implementation dispatch
    /// while automatic publication is still nonterminal.
    pub fn pending_automatic_dispatch_guards(&self) -> Result<Vec<PendingAutomaticDispatchGuard>> {
        self.store.pending_automatic_dispatch_guards()
    }

    pub async fn poll(&mut self, config: &ServiceConfig) -> Result<TriagePollSummary> {
        self.reconcile_factory_sessions();
        // A5 claim/reconciliation runs before legacy dispatch so verification
        // recovery can terminate owned processes before any new dispatch.
        if let Some(coordinator) = self.verification_coordinator.as_mut() {
            match coordinator.poll_once(config).await {
                Ok(summary) => {
                    if summary.verification_enabled
                        || summary.candidates_seen > 0
                        || summary.recovered > 0
                    {
                        tracing::info!(
                            event = "verification_poll_completed",
                            enabled = summary.verification_enabled,
                            candidates_seen = summary.candidates_seen,
                            attempts_started = summary.attempts_started,
                            attempts_completed = summary.attempts_completed,
                            attempts_failed = summary.attempts_failed,
                            waiting = summary.waiting,
                            recovered = summary.recovered,
                            preview_published = summary.preview_published,
                            "verification poll completed"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        event = "verification_poll_failed",
                        error = %error,
                        "verification poll failed; continuing orchestrator loop"
                    );
                }
            }
        }
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
                            automatic_published = summary.automatic_published,
                            automatic_pending = summary.automatic_pending,
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
        if let Some(coordinator) = self.review_coordinator.as_mut() {
            match coordinator.poll_once(config).await {
                Ok(summary) => {
                    if summary.review_enabled
                        || summary.candidates_seen > 0
                        || summary.attempts_started > 0
                    {
                        tracing::info!(
                            event = "review_poll_completed",
                            enabled = summary.review_enabled,
                            candidates_seen = summary.candidates_seen,
                            attempts_started = summary.attempts_started,
                            attempts_completed = summary.attempts_completed,
                            attempts_failed = summary.attempts_failed,
                            waiting = summary.waiting,
                            preview_published = summary.preview_published,
                            automatic_published = summary.automatic_published,
                            blocked = summary.blocked,
                            "review poll completed"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        event = "review_poll_failed",
                        error = %error,
                        "review poll failed; continuing orchestrator loop"
                    );
                }
            }
        }
        self.reconcile_factory_sessions();
        Ok(triage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_server::FactoryRunQuery;
    use rusqlite::params;

    #[test]
    fn factory_query_lists_and_resets_review_blocked_publications() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("factory.db");
        let store = SharedFactoryStore::open(&path, 5_000).unwrap();
        let mut db = store.inner.lock().unwrap();
        db.connection_for_test()
            .execute_batch("PRAGMA foreign_keys = OFF")
            .unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let error = serde_json::json!({
            "code": "review_publication_retry_exhausted",
            "component": "review_publication",
            "remediation": "restore forge access",
            "retryable": false
        })
        .to_string();
        db.connection_for_test()
            .execute(
                "INSERT INTO review_publication_intents (
                    intent_id, run_id, artifact_id, kind, status,
                    completed_steps_json, retry_count, last_error_json,
                    desired_effects_json, observed_baseline_json,
                    expected_projection_json, created_at, updated_at
                 ) VALUES ('review-blocked', 'run-review', 'artifact-review',
                    'formal', 'blocked', '[\"review_created\"]', 3, ?1,
                    '{}', '{}', '{}', ?2, ?2)",
                params![error, now],
            )
            .unwrap();
        db.connection_for_test()
            .execute(
                "INSERT INTO review_publication_intents (
                    intent_id, run_id, artifact_id, kind, status,
                    completed_steps_json, retry_count, last_error_json,
                    desired_effects_json, observed_baseline_json,
                    expected_projection_json, created_at, updated_at
                 ) VALUES ('review-conflict', 'run-review', 'artifact-conflict',
                    'formal', 'conflict', '[\"review_created\"]', 2, ?1,
                    '{}', '{}', '{}', ?2, ?2)",
                params![error, now],
            )
            .unwrap();
        drop(db);

        let blocked = FactoryRunQuery::blocked_publications(&store).unwrap();
        assert_eq!(blocked.len(), 2);
        assert!(blocked
            .iter()
            .any(|intent| intent.intent_id == "review-blocked"));
        assert!(blocked
            .iter()
            .any(|intent| intent.intent_id == "review-conflict"));

        let reset =
            FactoryRunQuery::reset_blocked_publication(&store, "review-blocked", "ada").unwrap();
        assert_eq!(reset.status, "pending");
        assert_eq!(reset.completed_steps, vec!["review_created"]);

        let conflict_reset =
            FactoryRunQuery::reset_blocked_publication(&store, "review-conflict", "ada").unwrap();
        assert_eq!(conflict_reset.status, "pending");
        assert!(FactoryRunQuery::blocked_publications(&store)
            .unwrap()
            .is_empty());
    }
}
