use crate::error::{Result, SymphonyError};
use crate::implementation::domain::{
    BundleArtifactRecord, DraftPrArtifactRecord, ExecutionProfile, ImplementationArtifactRecord,
    ImplementationAttemptInputs, ImplementationBlocker, ImplementationDecision,
    ImplementationManifest, ImplementationMetricsAggregate, ImplementationPublicationIntent,
    ImplementationPublicationKind, ImplementationRunState, ImplementationTurnKind,
    ImplementationTurnRecord, ImplementationTurnStatus, ValidationCommandResult,
    ValidationCycleRecord, IMPLEMENTATION_STAGE_NAME,
};
use crate::spec::artifact::validate_spec;
use crate::spec::domain::{
    ReviewFinding, SpecArtifact, SpecArtifactRecord, SpecDecision, SpecPublicationIntent,
    SpecPublicationKind, SpecRunState, SpecTurnKind, SpecTurnRecord, SpecTurnStatus,
    SPEC_STAGE_NAME,
};
use crate::review::domain::{
    ReviewAttemptRecord, ReviewFindingsArtifactRecord, ReviewMetricsAggregate,
    ReviewPublicationIntent, REVIEW_STAGE_NAME,
};
use crate::review::manifest::ReviewFindingsManifest;
use crate::triage::domain::{
    truncate_utf8_bytes, ArtifactRecord, FactoryError, FactoryEventRecord, FactoryRunRecord,
    FactoryRunStatus, PublicationIntentRecord, PublicationMode, PublicationStatus, StageRunRecord,
    StageStatus, StageUsage, TriageArtifact, TriageMetricsAggregate,
    FACTORY_ERROR_STRING_MAX_BYTES, FACTORY_EVENT_PAYLOAD_MAX_BYTES, TRIAGE_LEASE_STALE_AFTER_MS,
    TRIAGE_STAGE_NAME,
};
use crate::triage::process_identity::ProcessIdentity;
use crate::triage::storage_path::lock_path_for_storage;
use chrono::{DateTime, Duration, Utc};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::Duration as StdDuration;
use uuid::Uuid;

/// Embedded migrations, applied in order while the exclusive store lock is held.
const MIGRATIONS: [&str; 6] = [
    include_str!("migrations/001_init.sql"),
    include_str!("migrations/002_route_observations.sql"),
    include_str!("migrations/003_spec_stage.sql"),
    include_str!("migrations/004_implementation_stage.sql"),
    include_str!("migrations/005_implementation_draft_pr.sql"),
    include_str!("migrations/006_review_stage.sql"),
];

#[derive(Debug, Clone)]
pub struct ClaimAttemptRequest {
    pub forge_host: String,
    pub repository: String,
    pub issue_id: String,
    pub issue_identifier: String,
    pub issue_revision: String,
    pub configuration_revision: String,
    pub owner_instance: String,
    pub harness: String,
    pub model: Option<String>,
    pub workspace_path: Option<String>,
    pub output_path: Option<String>,
    pub pid: Option<i64>,
    pub process_group_id: Option<i64>,
    pub process_start_token: Option<String>,
    pub executable_identity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoreArtifactRequest {
    pub stage_run_id: String,
    pub issue_revision: String,
    pub configuration_revision: String,
    pub route_mapping_hash: String,
    pub artifact: TriageArtifact,
    pub bytes_len: u64,
    pub usage: StageUsage,
}

#[derive(Debug, Clone)]
pub struct StoreSpecTurnRequest {
    pub turn_id: String,
    pub stage_run_id: String,
    pub ordinal: u32,
    pub kind: SpecTurnKind,
    pub status: SpecTurnStatus,
    pub harness: String,
    pub model: Option<String>,
    pub usage: StageUsage,
    pub output_json: Option<serde_json::Value>,
    pub error: Option<FactoryError>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct StoreSpecArtifactRequest {
    pub stage_run_id: String,
    pub issue_revision: String,
    pub configuration_revision: String,
    pub artifact: SpecArtifact,
    pub review_cycles: u32,
    pub unresolved_blocking_findings: Vec<ReviewFinding>,
    pub bytes_len: u64,
    pub usage: StageUsage,
}

#[derive(Debug, Clone)]
pub struct StoreImplementationTurnRequest {
    pub turn_id: String,
    pub stage_run_id: String,
    pub ordinal: u32,
    pub kind: ImplementationTurnKind,
    pub status: ImplementationTurnStatus,
    pub harness: String,
    pub model: Option<String>,
    pub execution_profile: ExecutionProfile,
    pub usage: StageUsage,
    pub output_json: Option<serde_json::Value>,
    pub error: Option<FactoryError>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct StoreValidationCycleRequest {
    pub cycle_id: String,
    pub stage_run_id: String,
    pub cycle: u32,
    pub passed: bool,
    pub commands: Vec<ValidationCommandResult>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoreImplementationArtifactRequest {
    pub stage_run_id: String,
    pub approved_artifact_id: String,
    pub approved_version: u32,
    pub issue_revision: String,
    pub configuration_revision: String,
    pub manifest: ImplementationManifest,
    pub base_commit: String,
    pub head_commit: Option<String>,
    pub approved_spec_path: String,
    pub validation_cycles: u32,
    pub execution_profile: ExecutionProfile,
    pub bytes_len: u64,
    pub usage: StageUsage,
}

#[derive(Debug, Clone)]
pub struct StoreBundleArtifactRequest {
    pub stage_run_id: String,
    pub implementation_artifact_id: String,
    pub base_commit: String,
    pub head_commit: String,
    pub tree_sha: String,
    pub sha256: String,
    pub bytes_len: u64,
    pub changed_file_count: u32,
    pub changed_paths: Vec<String>,
    pub approved_spec_path: String,
    pub approved_spec_blob_sha: String,
    pub harness: String,
    pub model: Option<String>,
    pub execution_profile: ExecutionProfile,
    pub created_at: DateTime<Utc>,
    pub verified_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoreDraftPrArtifactRequest {
    pub run_id: String,
    pub implementation_artifact_id: String,
    pub intent_id: String,
    pub number: u64,
    pub url: String,
    pub draft: bool,
    pub head: String,
    pub base: String,
    pub head_sha: String,
    pub marker: String,
}

#[derive(Debug, Clone)]
pub struct UpsertImplementationStateRequest {
    pub run_id: String,
    pub approved_artifact_id: String,
    pub approved_version: u32,
    pub configuration_revision: String,
    pub decision: ImplementationDecision,
    pub successful_artifact_id: Option<String>,
    pub bundle_artifact_id: Option<String>,
    pub blocker: Option<ImplementationBlocker>,
}

/// Factory run with a terminal A2 approval that is eligible for A3 claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A3EligibleApprovedRun {
    pub run_id: String,
    pub forge_host: String,
    pub repository: String,
    pub issue_id: String,
    pub issue_identifier: String,
    pub issue_revision: Option<String>,
    pub approved_artifact_id: String,
    pub approved_version: u32,
}

/// Factory run with a terminal A3 draft-PR publication that may be claimed by A4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A4EligibleReviewRun {
    pub run_id: String,
    pub forge_host: String,
    pub repository: String,
    pub issue_id: String,
    pub issue_identifier: String,
    pub draft_pr_artifact_id: String,
    pub implementation_artifact_id: String,
    pub approved_artifact_id: String,
    pub pr_number: u64,
    pub pr_url: String,
    pub draft: bool,
    pub head: String,
    pub base: String,
    pub head_sha: String,
}

#[derive(Debug, Clone)]
pub struct StoreReviewAttemptRequest {
    pub attempt_id: String,
    pub stage_run_id: String,
    pub draft_pr_artifact_id: String,
    pub implementation_artifact_id: String,
    pub spec_artifact_id: String,
    pub pr_number: u64,
    pub reviewed_head_sha: String,
    pub base_sha: String,
}

#[derive(Debug, Clone)]
pub struct StoreReviewArtifactRequest {
    pub stage_run_id: String,
    pub attempt_id: String,
    pub draft_pr_artifact_id: String,
    pub implementation_artifact_id: String,
    pub spec_artifact_id: String,
    pub reviewed_head_sha: String,
    pub base_sha: String,
    pub manifest: ReviewFindingsManifest,
    pub bytes_len: u64,
    pub usage: StageUsage,
}

#[derive(Debug, Clone)]
pub struct CreatePublicationIntentRequest {
    pub run_id: String,
    pub artifact_id: Option<String>,
    pub mode: PublicationMode,
    pub intake_label: String,
    pub route_label: String,
    pub project_state: Option<String>,
    pub route_mapping_hash: String,
    pub desired_effects: serde_json::Value,
    pub observed_baseline: serde_json::Value,
    pub expected_projection: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct UpsertFactoryRunRequest {
    pub forge_host: String,
    pub repository: String,
    pub issue_id: String,
    pub issue_identifier: String,
    pub issue_revision: Option<String>,
    pub status: FactoryRunStatus,
    pub current_stage: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCommentIdentity {
    pub comment_id: String,
    pub intent_id: String,
    pub publisher_login: String,
}

#[derive(Debug, Clone)]
pub struct RecordAttemptProcessRequest {
    pub stage_run_id: String,
    pub owner_instance: String,
    pub identity: ProcessIdentity,
    pub workspace_path: Option<String>,
    pub output_path: Option<String>,
}

/// An interrupted attempt whose child process and disposable directories may
/// still be around after a crash or restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableAttempt {
    pub stage_run_id: String,
    pub run_id: String,
    pub owner_instance: Option<String>,
    /// Present only when every OS identity field was recorded.
    pub identity: Option<ProcessIdentity>,
    pub workspace_path: Option<String>,
    pub output_path: Option<String>,
}

/// A published route whose issue can be re-read to measure human agreement.
///
/// Carries the route mapping recorded at publication time so a later config
/// reload cannot reinterpret this observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionCandidate {
    pub run_id: String,
    pub stage_run_id: Option<String>,
    pub artifact_id: String,
    pub intent_id: String,
    /// Durable forge issue id, used to re-read the issue independently of the
    /// intake and implementation-candidate queries.
    pub issue_id: String,
    pub issue_identifier: String,
    pub intake_label: String,
    pub published_route: String,
    pub desired_effects: serde_json::Value,
}

/// Kind of post-publication observation recorded against an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteObservationKind {
    Corrected,
    Inconsistent,
}

impl RouteObservationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Corrected => "corrected",
            Self::Inconsistent => "inconsistent",
        }
    }
}

/// Durable guard used by the implementation scheduler while automatic
/// publication is still nonterminal (`pending`, `conflict`, or `blocked`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAutomaticDispatchGuard {
    pub forge_host: String,
    pub repository: String,
    pub issue_id: String,
    pub intake_label: String,
}

pub trait FactoryRunStore {
    fn claim_attempt(&mut self, request: ClaimAttemptRequest) -> Result<StageRunRecord>;
    fn store_artifact(&mut self, request: StoreArtifactRequest) -> Result<ArtifactRecord>;
    fn create_publication_intent(
        &mut self,
        request: CreatePublicationIntentRequest,
    ) -> Result<PublicationIntentRecord>;
    fn upsert_factory_run(&mut self, request: UpsertFactoryRunRequest) -> Result<FactoryRunRecord>;
    fn mark_run_status(&mut self, run_id: &str, status: FactoryRunStatus) -> Result<()>;
    fn fail_attempt(&mut self, stage_run_id: &str, error: FactoryError) -> Result<StageRunRecord>;
    fn get_run_by_id(&self, run_id: &str) -> Result<Option<FactoryRunRecord>>;
    fn get_run_by_issue(
        &self,
        forge_host: &str,
        repository: &str,
        issue_id: &str,
    ) -> Result<Option<FactoryRunRecord>>;
    fn get_run_by_issue_identifier(
        &self,
        issue_identifier: &str,
    ) -> Result<Option<FactoryRunRecord>>;
    fn get_stage_run(&self, stage_run_id: &str) -> Result<Option<StageRunRecord>>;
    fn list_stage_runs(&self, run_id: &str) -> Result<Vec<StageRunRecord>>;
    fn get_artifact_by_id(&self, artifact_id: &str) -> Result<Option<ArtifactRecord>>;
    fn get_artifact_for_revision(
        &self,
        run_id: &str,
        issue_revision: &str,
        configuration_revision: &str,
    ) -> Result<Option<ArtifactRecord>>;
    fn get_latest_artifact(&self, run_id: &str) -> Result<Option<ArtifactRecord>>;
    fn get_publication_intent(&self, intent_id: &str) -> Result<Option<PublicationIntentRecord>>;
    fn get_latest_publication(&self, run_id: &str) -> Result<Option<PublicationIntentRecord>>;
    fn list_pending_intents(&self, limit: usize) -> Result<Vec<PublicationIntentRecord>>;
    fn list_pending_automatic_dispatch_guards(&self) -> Result<Vec<PendingAutomaticDispatchGuard>>;
    fn list_intents_for_run(&self, run_id: &str) -> Result<Vec<PublicationIntentRecord>>;
    fn list_verified_comment_identities(&self, run_id: &str) -> Result<Vec<StoredCommentIdentity>>;
    fn list_stage_attempts_for_revision(
        &self,
        stage: &str,
        run_id: &str,
        issue_revision: &str,
        configuration_revision: &str,
    ) -> Result<Vec<StageRunRecord>>;
    fn update_publication_step(
        &mut self,
        intent_id: &str,
        completed_step: &str,
        status: PublicationStatus,
        error: Option<FactoryError>,
    ) -> Result<()>;
    /// Record a completed publication step and the expected managed projection
    /// it produced in one write, so a crash can never leave the two disagreeing.
    fn record_publication_step(
        &mut self,
        intent_id: &str,
        completed_step: &str,
        status: PublicationStatus,
        expected_projection: &serde_json::Value,
    ) -> Result<()>;
    /// Persist the managed baseline observed before any GitHub effect together
    /// with the initial expected projection.
    fn set_publication_baseline(
        &mut self,
        intent_id: &str,
        observed_baseline: &serde_json::Value,
        expected_projection: &serde_json::Value,
    ) -> Result<()>;
    fn set_publication_comment(
        &mut self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()>;
    fn record_event(&mut self, event: FactoryEventRecord) -> Result<()>;
    fn renew_lease(&mut self, stage_run_id: &str, owner_instance: &str) -> Result<bool>;
    fn interrupt_stale_attempts(&mut self) -> Result<u64>;
    /// Mark one running attempt interrupted. Used by restart recovery once the
    /// prior owner's lease is stale; returns `false` when the attempt is already
    /// terminal or owned by someone else.
    fn interrupt_attempt(&mut self, stage_run_id: &str, owner_instance: &str) -> Result<bool>;
    /// Record the spawned child's OS identity and disposable paths so a later
    /// process can decide whether the recorded process group is still this
    /// attempt, and which directories to reclaim.
    fn record_attempt_process(&mut self, request: RecordAttemptProcessRequest) -> Result<()>;
    /// Attempts a restart must clean up: interrupted, and still holding
    /// disposable paths or a recorded process group.
    fn list_recoverable_attempts(&self) -> Result<Vec<RecoverableAttempt>>;
    /// Clear the recorded process and paths once recovery has reclaimed them,
    /// so the same attempt is not swept twice.
    fn clear_attempt_process(&mut self, stage_run_id: &str) -> Result<()>;
    /// Applied automatic publications whose issues can be re-read to measure
    /// whether a human later changed the route.
    fn list_correction_candidates(&self, limit: usize) -> Result<Vec<CorrectionCandidate>>;
    /// Record one post-publication observation. Returns `false` when this
    /// artifact already recorded the same observation, which keeps the
    /// reconciler idempotent across polls.
    fn record_route_observation(
        &mut self,
        artifact_id: &str,
        kind: RouteObservationKind,
        value: &str,
    ) -> Result<bool>;
    fn triage_metrics(&self) -> Result<TriageMetricsAggregate>;
}

pub struct SqliteFactoryStore {
    conn: Connection,
    _lock_file: File,
}

impl SqliteFactoryStore {
    pub fn acquire_lock_and_migrate(
        path: &Path,
        busy_timeout_ms: u64,
    ) -> Result<SqliteFactoryStore> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let lock_path = lock_path_for_storage(path);
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        lock_file.try_lock_exclusive().map_err(|err| {
            SymphonyError::StorageError(format!(
                "could not acquire exclusive triage store lock {}: {err}",
                lock_path.display()
            ))
        })?;

        let conn = Connection::open(path).map_err(storage_error)?;
        conn.busy_timeout(StdDuration::from_millis(busy_timeout_ms))
            .map_err(storage_error)?;
        for migration in MIGRATIONS {
            conn.execute_batch(migration).map_err(storage_error)?;
        }

        Ok(Self {
            conn,
            _lock_file: lock_file,
        })
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    /// Test-only helper to simulate crash windows by rewinding completed steps
    /// and expected projection while leaving GitHub side effects intact.
    #[cfg(test)]
    pub fn debug_rewind_publication(
        &mut self,
        intent_id: &str,
        completed_steps: &[&str],
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        let now_s = ts(Self::now());
        let changed = self
            .conn
            .execute(
                "UPDATE publication_intents
                 SET completed_steps_json = ?1, status = ?2, expected_projection_json = ?3,
                     updated_at = ?4
                 WHERE intent_id = ?5",
                params![
                    serde_json::to_string(&completed_steps).map_err(storage_error)?,
                    PublicationStatus::Pending.as_str(),
                    bounded_json(expected_projection)?,
                    now_s,
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "publication intent {intent_id} not found"
            )));
        }
        Ok(())
    }

    /// Claim an attempt for a typed factory stage. A1's trait method delegates
    /// to its original triage-specific implementation; A2 uses this method so
    /// triage and spec attempts can coexist for one revision pair.
    pub fn claim_stage_attempt(
        &mut self,
        stage: &str,
        request: ClaimAttemptRequest,
    ) -> Result<StageRunRecord> {
        if stage.trim().is_empty() {
            return Err(SymphonyError::StorageError(
                "factory stage name must be non-empty".to_string(),
            ));
        }
        let now = Self::now();
        let now_s = ts(now);
        let tx = self.conn.transaction().map_err(storage_error)?;
        let existing_run_id: Option<String> = tx
            .query_row(
                "SELECT run_id FROM factory_runs WHERE forge_host = ?1 AND repository = ?2 AND issue_id = ?3",
                params![request.forge_host, request.repository, request.issue_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let run_id = existing_run_id.unwrap_or_else(new_id);
        tx.execute(
            "INSERT INTO factory_runs (
                run_id, forge_host, repository, issue_id, issue_identifier, issue_revision,
                status, current_stage, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(forge_host, repository, issue_id) DO UPDATE SET
                issue_identifier = excluded.issue_identifier,
                issue_revision = excluded.issue_revision,
                status = excluded.status,
                current_stage = excluded.current_stage,
                updated_at = excluded.updated_at",
            params![
                run_id,
                request.forge_host,
                request.repository,
                request.issue_id,
                request.issue_identifier,
                request.issue_revision,
                FactoryRunStatus::Active.as_str(),
                stage,
                now_s,
                now_s,
            ],
        )
        .map_err(storage_error)?;
        let run_id: String = tx
            .query_row(
                "SELECT run_id FROM factory_runs WHERE forge_host = ?1 AND repository = ?2 AND issue_id = ?3",
                params![request.forge_host, request.repository, request.issue_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let attempt = tx
            .query_row(
                "SELECT COALESCE(MAX(attempt), 0) + 1 FROM stage_runs WHERE run_id = ?1 AND stage = ?2",
                params![run_id, stage],
                |row| row.get::<_, u32>(0),
            )
            .map_err(storage_error)?;
        let stage_run_id = new_id();
        tx.execute(
            "INSERT INTO stage_runs (
                stage_run_id, run_id, stage, issue_revision, configuration_revision, attempt,
                owner_instance, pid, process_group_id, process_start_token, executable_identity,
                lease_heartbeat_at, status, harness, model, workspace_path, output_path,
                started_at, completed_at, input_tokens, output_tokens, total_tokens, error_json,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, NULL, 0, 0, 0, NULL, ?19, ?20)",
            params![
                stage_run_id,
                run_id,
                stage,
                request.issue_revision,
                request.configuration_revision,
                attempt,
                request.owner_instance,
                request.pid,
                request.process_group_id,
                request.process_start_token,
                request.executable_identity,
                now_s,
                StageStatus::Running.as_str(),
                request.harness,
                request.model,
                request.workspace_path,
                request.output_path,
                now_s,
                now_s,
                now_s,
            ],
        )
        .map_err(storage_error)?;
        let record = select_stage_run(&tx, &stage_run_id)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }
}

impl FactoryRunStore for SqliteFactoryStore {
    fn claim_attempt(&mut self, request: ClaimAttemptRequest) -> Result<StageRunRecord> {
        let now = Self::now();
        let now_s = ts(now);
        let tx = self.conn.transaction().map_err(storage_error)?;

        let existing_run_id: Option<String> = tx
            .query_row(
                "SELECT run_id FROM factory_runs WHERE forge_host = ?1 AND repository = ?2 AND issue_id = ?3",
                params![request.forge_host, request.repository, request.issue_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;

        let run_id = existing_run_id.unwrap_or_else(new_id);
        tx.execute(
            "INSERT INTO factory_runs (
                run_id, forge_host, repository, issue_id, issue_identifier, issue_revision,
                status, current_stage, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(forge_host, repository, issue_id) DO UPDATE SET
                issue_identifier = excluded.issue_identifier,
                issue_revision = excluded.issue_revision,
                status = excluded.status,
                current_stage = excluded.current_stage,
                updated_at = excluded.updated_at",
            params![
                run_id,
                request.forge_host,
                request.repository,
                request.issue_id,
                request.issue_identifier,
                request.issue_revision,
                FactoryRunStatus::Active.as_str(),
                TRIAGE_STAGE_NAME,
                now_s,
                now_s,
            ],
        )
        .map_err(storage_error)?;

        let run_id: String = tx
            .query_row(
                "SELECT run_id FROM factory_runs WHERE forge_host = ?1 AND repository = ?2 AND issue_id = ?3",
                params![request.forge_host, request.repository, request.issue_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let attempt = tx
            .query_row(
                "SELECT COALESCE(MAX(attempt), 0) + 1 FROM stage_runs WHERE run_id = ?1 AND stage = ?2",
                params![run_id, TRIAGE_STAGE_NAME],
                |row| row.get::<_, u32>(0),
            )
            .map_err(storage_error)?;
        let stage_run_id = new_id();

        tx.execute(
            "INSERT INTO stage_runs (
                stage_run_id, run_id, stage, issue_revision, configuration_revision, attempt,
                owner_instance, pid, process_group_id, process_start_token, executable_identity,
                lease_heartbeat_at, status, harness, model, workspace_path, output_path,
                started_at, completed_at, input_tokens, output_tokens, total_tokens, error_json,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, NULL, 0, 0, 0, NULL, ?19, ?20)",
            params![
                stage_run_id,
                run_id,
                TRIAGE_STAGE_NAME,
                request.issue_revision,
                request.configuration_revision,
                attempt,
                request.owner_instance,
                request.pid,
                request.process_group_id,
                request.process_start_token,
                request.executable_identity,
                now_s,
                StageStatus::Running.as_str(),
                request.harness,
                request.model,
                request.workspace_path,
                request.output_path,
                now_s,
                now_s,
                now_s,
            ],
        )
        .map_err(storage_error)?;

        let record = select_stage_run(&tx, &stage_run_id)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn store_artifact(&mut self, request: StoreArtifactRequest) -> Result<ArtifactRecord> {
        let now = Self::now();
        let now_s = ts(now);
        let tx = self.conn.transaction().map_err(storage_error)?;
        let stage = select_stage_run(&tx, &request.stage_run_id)?;
        // Only a still-running attempt may complete. A restart that interrupted
        // this attempt has already released it for retry, so accepting late
        // output here would silently turn an abandoned attempt into a success.
        if stage.status != StageStatus::Running {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is {} and cannot accept triage output",
                request.stage_run_id,
                stage.status.as_str()
            )));
        }
        let artifact_id = new_id();
        let artifact_json = serde_json::to_string(&request.artifact).map_err(storage_error)?;

        tx.execute(
            "INSERT INTO triage_artifacts (
                artifact_id, run_id, stage_run_id, issue_revision, configuration_revision,
                route_mapping_hash, schema_version, route, risk_class, artifact_json,
                received_at, bytes_len
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                artifact_id,
                stage.run_id,
                request.stage_run_id,
                request.issue_revision,
                request.configuration_revision,
                request.route_mapping_hash,
                request.artifact.schema_version,
                request.artifact.route.as_str(),
                request.artifact.risk_class.as_str(),
                artifact_json,
                now_s,
                request.bytes_len,
            ],
        )
        .map_err(storage_error)?;

        tx.execute(
            "UPDATE stage_runs
             SET status = ?1, completed_at = ?2, input_tokens = ?3, output_tokens = ?4,
                 total_tokens = ?5, updated_at = ?6
             WHERE stage_run_id = ?7",
            params![
                StageStatus::Completed.as_str(),
                now_s,
                request.usage.input_tokens,
                request.usage.output_tokens,
                request.usage.total_tokens,
                now_s,
                request.stage_run_id,
            ],
        )
        .map_err(storage_error)?;

        let record = select_artifact(&tx, &artifact_id)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }

    fn create_publication_intent(
        &mut self,
        request: CreatePublicationIntentRequest,
    ) -> Result<PublicationIntentRecord> {
        let now_s = ts(Self::now());
        let intent_id = new_id();
        self.conn
            .execute(
                "INSERT INTO publication_intents (
                    intent_id, run_id, artifact_id, mode, status, intake_label, route_label,
                    project_state, route_mapping_hash, completed_steps_json, retry_count,
                    last_error_json, comment_id, publisher_login, desired_effects_json,
                    observed_baseline_json, expected_projection_json, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, NULL, NULL, NULL,
                    ?11, ?12, ?13, ?14, ?15)",
                params![
                    intent_id,
                    request.run_id,
                    request.artifact_id,
                    request.mode.as_str(),
                    PublicationStatus::Pending.as_str(),
                    request.intake_label,
                    request.route_label,
                    request.project_state,
                    request.route_mapping_hash,
                    "[]",
                    bounded_json(&request.desired_effects)?,
                    bounded_json(&request.observed_baseline)?,
                    bounded_json(&request.expected_projection)?,
                    now_s,
                    now_s,
                ],
            )
            .map_err(storage_error)?;

        select_publication_intent(&self.conn, &intent_id)
    }

    fn upsert_factory_run(&mut self, request: UpsertFactoryRunRequest) -> Result<FactoryRunRecord> {
        let now_s = ts(Self::now());
        let run_id = new_id();
        self.conn
            .execute(
                "INSERT INTO factory_runs (
                    run_id, forge_host, repository, issue_id, issue_identifier, issue_revision,
                    status, current_stage, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(forge_host, repository, issue_id) DO UPDATE SET
                    issue_identifier = excluded.issue_identifier,
                    issue_revision = excluded.issue_revision,
                    status = excluded.status,
                    current_stage = excluded.current_stage,
                    updated_at = excluded.updated_at",
                params![
                    run_id,
                    request.forge_host,
                    request.repository,
                    request.issue_id,
                    request.issue_identifier,
                    request.issue_revision,
                    request.status.as_str(),
                    request.current_stage,
                    now_s,
                    now_s,
                ],
            )
            .map_err(storage_error)?;

        self.get_run_by_issue(&request.forge_host, &request.repository, &request.issue_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError("factory run missing after upsert".to_string())
            })
    }

    fn mark_run_status(&mut self, run_id: &str, status: FactoryRunStatus) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE factory_runs SET status = ?1, updated_at = ?2 WHERE run_id = ?3",
                params![status.as_str(), ts(Self::now()), run_id],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "factory run {run_id} not found"
            )));
        }
        Ok(())
    }

    fn fail_attempt(&mut self, stage_run_id: &str, error: FactoryError) -> Result<StageRunRecord> {
        let now_s = ts(Self::now());
        let changed = self
            .conn
            .execute(
                "UPDATE stage_runs
                 SET status = ?1, completed_at = ?2, error_json = ?3, updated_at = ?2
                 WHERE stage_run_id = ?4",
                params![
                    StageStatus::Failed.as_str(),
                    now_s,
                    optional_json(&Some(error))?,
                    stage_run_id,
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "stage run {stage_run_id} not found"
            )));
        }
        select_stage_run(&self.conn, stage_run_id)
    }

    fn get_run_by_id(&self, run_id: &str) -> Result<Option<FactoryRunRecord>> {
        self.conn
            .query_row(
                "SELECT run_id, forge_host, repository, issue_id, issue_identifier,
                    issue_revision, status, current_stage, created_at, updated_at
                 FROM factory_runs WHERE run_id = ?1",
                params![run_id],
                factory_run_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn get_run_by_issue(
        &self,
        forge_host: &str,
        repository: &str,
        issue_id: &str,
    ) -> Result<Option<FactoryRunRecord>> {
        self.conn
            .query_row(
                "SELECT run_id, forge_host, repository, issue_id, issue_identifier,
                    issue_revision, status, current_stage, created_at, updated_at
                 FROM factory_runs WHERE forge_host = ?1 AND repository = ?2 AND issue_id = ?3",
                params![forge_host, repository, issue_id],
                factory_run_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn get_run_by_issue_identifier(
        &self,
        issue_identifier: &str,
    ) -> Result<Option<FactoryRunRecord>> {
        self.conn
            .query_row(
                "SELECT run_id, forge_host, repository, issue_id, issue_identifier,
                    issue_revision, status, current_stage, created_at, updated_at
                 FROM factory_runs WHERE issue_identifier = ?1
                 ORDER BY updated_at DESC LIMIT 1",
                params![issue_identifier],
                factory_run_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn list_stage_runs(&self, run_id: &str) -> Result<Vec<StageRunRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT stage_run_id, run_id, stage, issue_revision, configuration_revision, attempt,
                    owner_instance, pid, process_group_id, process_start_token, executable_identity,
                    lease_heartbeat_at, status, harness, model, workspace_path, output_path, started_at,
                    completed_at, input_tokens, output_tokens, total_tokens, error_json
                 FROM stage_runs WHERE run_id = ?1 ORDER BY attempt ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], stage_run_from_row)
            .map_err(storage_error)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(storage_error)?);
        }
        Ok(out)
    }

    fn get_latest_artifact(&self, run_id: &str) -> Result<Option<ArtifactRecord>> {
        self.conn
            .query_row(
                "SELECT artifact_id, run_id, stage_run_id, issue_revision, configuration_revision,
                    route_mapping_hash, artifact_json, received_at, bytes_len
                 FROM triage_artifacts WHERE run_id = ?1
                 ORDER BY received_at DESC LIMIT 1",
                params![run_id],
                artifact_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn get_latest_publication(&self, run_id: &str) -> Result<Option<PublicationIntentRecord>> {
        self.conn
            .query_row(
                "SELECT intent_id, run_id, artifact_id, mode, status, intake_label, route_label,
                    project_state, route_mapping_hash, completed_steps_json, retry_count,
                    last_error_json, comment_id, publisher_login, desired_effects_json,
                    observed_baseline_json, expected_projection_json, created_at, updated_at
                 FROM publication_intents WHERE run_id = ?1
                 ORDER BY updated_at DESC LIMIT 1",
                params![run_id],
                publication_intent_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn get_stage_run(&self, stage_run_id: &str) -> Result<Option<StageRunRecord>> {
        self.conn
            .query_row(
                "SELECT stage_run_id, run_id, stage, issue_revision, configuration_revision, attempt,
                    owner_instance, pid, process_group_id, process_start_token, executable_identity,
                    lease_heartbeat_at, status, harness, model, workspace_path, output_path, started_at,
                    completed_at, input_tokens, output_tokens, total_tokens, error_json
                 FROM stage_runs WHERE stage_run_id = ?1",
                params![stage_run_id],
                stage_run_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn get_artifact_by_id(&self, artifact_id: &str) -> Result<Option<ArtifactRecord>> {
        self.conn
            .query_row(
                "SELECT artifact_id, run_id, stage_run_id, issue_revision, configuration_revision,
                    route_mapping_hash, artifact_json, received_at, bytes_len
                 FROM triage_artifacts WHERE artifact_id = ?1",
                params![artifact_id],
                artifact_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn get_artifact_for_revision(
        &self,
        run_id: &str,
        issue_revision: &str,
        configuration_revision: &str,
    ) -> Result<Option<ArtifactRecord>> {
        self.conn
            .query_row(
                "SELECT artifact_id, run_id, stage_run_id, issue_revision, configuration_revision,
                    route_mapping_hash, artifact_json, received_at, bytes_len
                 FROM triage_artifacts
                 WHERE run_id = ?1 AND issue_revision = ?2 AND configuration_revision = ?3",
                params![run_id, issue_revision, configuration_revision],
                artifact_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn get_publication_intent(&self, intent_id: &str) -> Result<Option<PublicationIntentRecord>> {
        self.conn
            .query_row(
                "SELECT intent_id, run_id, artifact_id, mode, status, intake_label, route_label,
                    project_state, route_mapping_hash, completed_steps_json, retry_count, last_error_json,
                    comment_id, publisher_login, desired_effects_json, observed_baseline_json,
                    expected_projection_json, created_at, updated_at
                 FROM publication_intents WHERE intent_id = ?1",
                params![intent_id],
                publication_intent_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    fn list_intents_for_run(&self, run_id: &str) -> Result<Vec<PublicationIntentRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT intent_id, run_id, artifact_id, mode, status, intake_label, route_label,
                    project_state, route_mapping_hash, completed_steps_json, retry_count,
                    last_error_json, comment_id, publisher_login, desired_effects_json,
                    observed_baseline_json, expected_projection_json, created_at, updated_at
                 FROM publication_intents WHERE run_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], publication_intent_from_row)
            .map_err(storage_error)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(storage_error)?);
        }
        Ok(out)
    }

    fn list_verified_comment_identities(&self, run_id: &str) -> Result<Vec<StoredCommentIdentity>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT comment_id, intent_id, publisher_login
                 FROM publication_intents
                 WHERE run_id = ?1
                   AND comment_id IS NOT NULL
                   AND publisher_login IS NOT NULL
                 ORDER BY updated_at ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], |row| {
                Ok(StoredCommentIdentity {
                    comment_id: row.get(0)?,
                    intent_id: row.get(1)?,
                    publisher_login: row.get(2)?,
                })
            })
            .map_err(storage_error)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(storage_error)?);
        }
        Ok(out)
    }

    fn list_stage_attempts_for_revision(
        &self,
        stage: &str,
        run_id: &str,
        issue_revision: &str,
        configuration_revision: &str,
    ) -> Result<Vec<StageRunRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT stage_run_id, run_id, stage, issue_revision, configuration_revision, attempt,
                    owner_instance, pid, process_group_id, process_start_token, executable_identity,
                    lease_heartbeat_at, status, harness, model, workspace_path, output_path, started_at,
                    completed_at, input_tokens, output_tokens, total_tokens, error_json
                 FROM stage_runs
                 WHERE run_id = ?1
                   AND issue_revision = ?2
                   AND configuration_revision = ?3
                   AND stage = ?4
                 ORDER BY attempt ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(
                params![run_id, issue_revision, configuration_revision, stage],
                stage_run_from_row,
            )
            .map_err(storage_error)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(storage_error)?);
        }
        Ok(out)
    }

    fn list_pending_intents(&self, limit: usize) -> Result<Vec<PublicationIntentRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT intent_id, run_id, artifact_id, mode, status, intake_label, route_label,
                    project_state, route_mapping_hash, completed_steps_json, retry_count,
                    last_error_json, comment_id, publisher_login, desired_effects_json,
                    observed_baseline_json, expected_projection_json, created_at, updated_at
                 FROM publication_intents WHERE status = ?1 ORDER BY updated_at ASC LIMIT ?2",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(
                params![PublicationStatus::Pending.as_str(), limit as i64],
                publication_intent_from_row,
            )
            .map_err(storage_error)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    fn list_pending_automatic_dispatch_guards(&self) -> Result<Vec<PendingAutomaticDispatchGuard>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT r.forge_host, r.repository, r.issue_id, i.intake_label, i.updated_at
                 FROM publication_intents i
                 INNER JOIN factory_runs r ON r.run_id = i.run_id
                 WHERE i.mode = ?1 AND i.status IN (?2, ?3, ?4)
                 UNION ALL
                 SELECT r.forge_host, r.repository, r.issue_id,
                    COALESCE(json_extract(i.desired_effects_json, '$.intake_label'), 'ready-to-spec'),
                    i.updated_at
                 FROM spec_publication_intents i
                 INNER JOIN factory_runs r ON r.run_id = i.run_id
                 WHERE i.kind = 'approval' AND i.status IN (?2, ?3, ?4)
                 UNION ALL
                 SELECT r.forge_host, r.repository, r.issue_id, 'implementation', irs.updated_at
                 FROM factory_runs r
                 JOIN spec_run_state s ON s.run_id = r.run_id
                 JOIN implementation_run_state irs ON irs.run_id = r.run_id
                 WHERE s.decision = 'approved' AND s.approved_artifact_id IS NOT NULL
                 UNION ALL
                 SELECT r.forge_host, r.repository, r.issue_id, 'implementation', sr.updated_at
                 FROM factory_runs r
                 JOIN spec_run_state s ON s.run_id = r.run_id
                 JOIN stage_runs sr ON sr.run_id = r.run_id
                 WHERE s.decision = 'approved' AND s.approved_artifact_id IS NOT NULL
                   AND sr.stage = 'implementation' AND sr.status IN ('pending', 'running')
                   AND NOT EXISTS (
                     SELECT 1 FROM implementation_run_state irs WHERE irs.run_id = r.run_id
                   )
                 UNION ALL
                 SELECT r.forge_host, r.repository, r.issue_id, 'implementation', i.updated_at
                 FROM factory_runs r
                 JOIN spec_run_state s ON s.run_id = r.run_id
                 JOIN implementation_publication_intents i ON i.run_id = r.run_id
                 WHERE s.decision = 'approved' AND s.approved_artifact_id IS NOT NULL
                   AND i.status IN ('pending', 'conflict', 'blocked')
                   AND NOT EXISTS (
                     SELECT 1 FROM implementation_run_state irs WHERE irs.run_id = r.run_id
                   )
                 ORDER BY 5 ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(
                params![
                    PublicationMode::Automatic.as_str(),
                    PublicationStatus::Pending.as_str(),
                    PublicationStatus::Conflict.as_str(),
                    PublicationStatus::Blocked.as_str(),
                ],
                |row| {
                    Ok(PendingAutomaticDispatchGuard {
                        forge_host: row.get(0)?,
                        repository: row.get(1)?,
                        issue_id: row.get(2)?,
                        intake_label: row.get(3)?,
                    })
                },
            )
            .map_err(storage_error)?;
        // Deduplicate by (forge_host, repository, issue_id) while preserving
        // earliest updated_at order from the UNION query.
        let mut seen = std::collections::HashSet::new();
        let mut guards = Vec::new();
        for row in rows {
            let guard = row.map_err(storage_error)?;
            let key = (
                guard.forge_host.clone(),
                guard.repository.clone(),
                guard.issue_id.clone(),
            );
            if seen.insert(key) {
                guards.push(guard);
            }
        }
        Ok(guards)
    }

    fn update_publication_step(
        &mut self,
        intent_id: &str,
        completed_step: &str,
        status: PublicationStatus,
        error: Option<FactoryError>,
    ) -> Result<()> {
        let mut intent = select_publication_intent(&self.conn, intent_id)?;
        if !completed_step.trim().is_empty()
            && !intent
                .completed_steps
                .iter()
                .any(|step| step == completed_step.trim())
        {
            intent
                .completed_steps
                .push(completed_step.trim().to_string());
        }
        let now_s = ts(Self::now());
        self.conn
            .execute(
                "UPDATE publication_intents
                 SET completed_steps_json = ?1, status = ?2, last_error_json = ?3,
                     retry_count = retry_count + CASE WHEN ?3 IS NULL THEN 0 ELSE 1 END,
                     updated_at = ?4
                 WHERE intent_id = ?5",
                params![
                    serde_json::to_string(&intent.completed_steps).map_err(storage_error)?,
                    status.as_str(),
                    optional_json(&error)?,
                    now_s,
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn record_publication_step(
        &mut self,
        intent_id: &str,
        completed_step: &str,
        status: PublicationStatus,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        let mut intent = select_publication_intent(&self.conn, intent_id)?;
        if !completed_step.trim().is_empty()
            && !intent
                .completed_steps
                .iter()
                .any(|step| step == completed_step.trim())
        {
            intent
                .completed_steps
                .push(completed_step.trim().to_string());
        }
        let now_s = ts(Self::now());
        self.conn
            .execute(
                "UPDATE publication_intents
                 SET completed_steps_json = ?1, status = ?2, last_error_json = NULL,
                     expected_projection_json = ?3, updated_at = ?4
                 WHERE intent_id = ?5",
                params![
                    serde_json::to_string(&intent.completed_steps).map_err(storage_error)?,
                    status.as_str(),
                    bounded_json(expected_projection)?,
                    now_s,
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn set_publication_baseline(
        &mut self,
        intent_id: &str,
        observed_baseline: &serde_json::Value,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        let now_s = ts(Self::now());
        let changed = self
            .conn
            .execute(
                "UPDATE publication_intents
                 SET observed_baseline_json = ?1, expected_projection_json = ?2, updated_at = ?3
                 WHERE intent_id = ?4",
                params![
                    bounded_json(observed_baseline)?,
                    bounded_json(expected_projection)?,
                    now_s,
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "publication intent {intent_id} not found"
            )));
        }
        Ok(())
    }

    fn set_publication_comment(
        &mut self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()> {
        let now_s = ts(Self::now());
        let changed = self
            .conn
            .execute(
                "UPDATE publication_intents
                 SET comment_id = ?1, publisher_login = ?2, updated_at = ?3
                 WHERE intent_id = ?4",
                params![comment_id, publisher_login, now_s, intent_id],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "publication intent {intent_id} not found"
            )));
        }
        Ok(())
    }

    fn record_event(&mut self, event: FactoryEventRecord) -> Result<()> {
        let payload_json = bounded_json(&event.payload)?;
        self.conn
            .execute(
                "INSERT INTO factory_events (
                    event_id, run_id, stage_run_id, event_type, timestamp, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    event.event_id,
                    event.run_id,
                    event.stage_run_id,
                    event.event_type,
                    ts(event.timestamp),
                    payload_json,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn renew_lease(&mut self, stage_run_id: &str, owner_instance: &str) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE stage_runs SET lease_heartbeat_at = ?1, updated_at = ?1
                 WHERE stage_run_id = ?2 AND owner_instance = ?3 AND status = ?4",
                params![
                    ts(Self::now()),
                    stage_run_id,
                    owner_instance,
                    StageStatus::Running.as_str()
                ],
            )
            .map_err(storage_error)?;
        Ok(changed == 1)
    }

    fn interrupt_attempt(&mut self, stage_run_id: &str, owner_instance: &str) -> Result<bool> {
        let now_s = ts(Self::now());
        let changed = self
            .conn
            .execute(
                "UPDATE stage_runs
                 SET status = ?1, completed_at = ?2, updated_at = ?2
                 WHERE stage_run_id = ?3 AND owner_instance = ?4 AND status = ?5",
                params![
                    StageStatus::Interrupted.as_str(),
                    now_s,
                    stage_run_id,
                    owner_instance,
                    StageStatus::Running.as_str(),
                ],
            )
            .map_err(storage_error)?;
        Ok(changed == 1)
    }

    fn record_attempt_process(&mut self, request: RecordAttemptProcessRequest) -> Result<()> {
        let now_s = ts(Self::now());
        self.conn
            .execute(
                "UPDATE stage_runs
                 SET pid = ?1, process_group_id = ?2, process_start_token = ?3,
                     executable_identity = ?4, workspace_path = ?5, output_path = ?6,
                     updated_at = ?7
                 WHERE stage_run_id = ?8 AND owner_instance = ?9 AND status = ?10",
                params![
                    request.identity.pid,
                    request.identity.process_group_id,
                    request.identity.start_token,
                    request.identity.executable,
                    request.workspace_path,
                    request.output_path,
                    now_s,
                    request.stage_run_id,
                    request.owner_instance,
                    StageStatus::Running.as_str(),
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn list_recoverable_attempts(&self) -> Result<Vec<RecoverableAttempt>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT stage_run_id, run_id, owner_instance, pid, process_group_id,
                        process_start_token, executable_identity, workspace_path, output_path
                 FROM stage_runs
                 WHERE stage = ?1 AND status = ?2
                   AND (pid IS NOT NULL OR workspace_path IS NOT NULL OR output_path IS NOT NULL)
                 ORDER BY updated_at",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(
                params![TRIAGE_STAGE_NAME, StageStatus::Interrupted.as_str()],
                |row| {
                    let pid: Option<i64> = row.get(3)?;
                    let process_group_id: Option<i64> = row.get(4)?;
                    let start_token: Option<String> = row.get(5)?;
                    let executable: Option<String> = row.get(6)?;
                    // Only a fully recorded identity can authorize signalling.
                    let identity = match (pid, process_group_id) {
                        (Some(pid), Some(process_group_id)) => Some(ProcessIdentity {
                            pid,
                            process_group_id,
                            start_token,
                            executable,
                        }),
                        _ => None,
                    };
                    Ok(RecoverableAttempt {
                        stage_run_id: row.get(0)?,
                        run_id: row.get(1)?,
                        owner_instance: row.get(2)?,
                        identity,
                        workspace_path: row.get(7)?,
                        output_path: row.get(8)?,
                    })
                },
            )
            .map_err(storage_error)?;
        let mut attempts = Vec::new();
        for row in rows {
            attempts.push(row.map_err(storage_error)?);
        }
        Ok(attempts)
    }

    fn clear_attempt_process(&mut self, stage_run_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE stage_runs
                 SET pid = NULL, process_group_id = NULL, process_start_token = NULL,
                     executable_identity = NULL, workspace_path = NULL, output_path = NULL,
                     updated_at = ?1
                 WHERE stage_run_id = ?2",
                params![ts(Self::now()), stage_run_id],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn list_correction_candidates(&self, limit: usize) -> Result<Vec<CorrectionCandidate>> {
        // Only the latest applied automatic publication per run is measurable:
        // a later triage attempt resets the decision under observation. RANDOM
        // ordering rotates the bounded batch so older runs are not starved when
        // more than `limit` candidates exist.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT i.run_id, a.stage_run_id, i.artifact_id, i.intent_id, r.issue_id,
                        r.issue_identifier, i.intake_label, a.route, i.desired_effects_json
                 FROM publication_intents i
                 JOIN triage_artifacts a ON a.artifact_id = i.artifact_id
                 JOIN factory_runs r ON r.run_id = i.run_id
                 WHERE i.mode = ?1 AND i.status = ?2
                   AND NOT EXISTS (
                     SELECT 1 FROM publication_intents newer
                     WHERE newer.run_id = i.run_id
                       AND newer.mode = i.mode
                       AND newer.status = i.status
                       AND (
                         newer.updated_at > i.updated_at
                         OR (newer.updated_at = i.updated_at AND newer.intent_id > i.intent_id)
                       )
                   )
                 ORDER BY RANDOM()
                 LIMIT ?3",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(
                params![
                    PublicationMode::Automatic.as_str(),
                    PublicationStatus::Applied.as_str(),
                    limit as i64,
                ],
                |row| {
                    let desired: String = row.get(8)?;
                    Ok(CorrectionCandidate {
                        run_id: row.get(0)?,
                        stage_run_id: row.get(1)?,
                        artifact_id: row.get(2)?,
                        intent_id: row.get(3)?,
                        issue_id: row.get(4)?,
                        issue_identifier: row.get(5)?,
                        intake_label: row.get(6)?,
                        published_route: row.get(7)?,
                        desired_effects: serde_json::from_str(&desired)
                            .unwrap_or(serde_json::Value::Null),
                    })
                },
            )
            .map_err(storage_error)?;
        let mut candidates = Vec::new();
        for row in rows {
            candidates.push(row.map_err(storage_error)?);
        }
        Ok(candidates)
    }

    fn record_route_observation(
        &mut self,
        artifact_id: &str,
        kind: RouteObservationKind,
        value: &str,
    ) -> Result<bool> {
        let inserted = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO route_observations (artifact_id, kind, value, observed_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![artifact_id, kind.as_str(), value, ts(Self::now())],
            )
            .map_err(storage_error)?;
        Ok(inserted == 1)
    }

    fn interrupt_stale_attempts(&mut self) -> Result<u64> {
        let stale_before = ts(Self::now() - Duration::milliseconds(TRIAGE_LEASE_STALE_AFTER_MS));
        let changed = self
            .conn
            .execute(
                "UPDATE stage_runs
                 SET status = ?1, completed_at = ?2, updated_at = ?2
                 WHERE status = ?3 AND lease_heartbeat_at < ?4",
                params![
                    StageStatus::Interrupted.as_str(),
                    ts(Self::now()),
                    StageStatus::Running.as_str(),
                    stale_before,
                ],
            )
            .map_err(storage_error)?;
        Ok(changed as u64)
    }

    fn triage_metrics(&self) -> Result<TriageMetricsAggregate> {
        use crate::triage::domain::{TriageMetricsDuration, TriageMetricsTokenTotals};
        use std::collections::BTreeMap;

        let mut aggregate = TriageMetricsAggregate::default();

        let (total, completed, failed, input_tokens, output_tokens, total_tokens) = self
            .conn
            .query_row(
                "SELECT
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(total_tokens), 0)
                 FROM stage_runs WHERE stage = ?1",
                params![TRIAGE_STAGE_NAME],
                |row| {
                    Ok((
                        row.get::<_, u64>(0)?,
                        row.get::<_, u64>(1)?,
                        row.get::<_, u64>(2)?,
                        row.get::<_, u64>(3)?,
                        row.get::<_, u64>(4)?,
                        row.get::<_, u64>(5)?,
                    ))
                },
            )
            .map_err(storage_error)?;

        aggregate.total_attempts = total;
        aggregate.completed_attempts = completed;
        aggregate.failed_attempts = failed;
        aggregate.input_tokens = input_tokens;
        aggregate.output_tokens = output_tokens;
        aggregate.total_tokens = total_tokens;

        aggregate.ineligible_issues = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM factory_runs WHERE status = 'ineligible'",
                [],
                |row| row.get::<_, u64>(0),
            )
            .map_err(storage_error)?;

        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT route, COUNT(*) FROM triage_artifacts GROUP BY route ORDER BY route",
                )
                .map_err(storage_error)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
                })
                .map_err(storage_error)?;
            for row in rows {
                let (route, count) = row.map_err(storage_error)?;
                aggregate.route_counts.insert(route, count);
            }
        }

        aggregate.correction_count = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM factory_events WHERE event_type = 'triage_route_corrected'",
                [],
                |row| row.get::<_, u64>(0),
            )
            .unwrap_or(0);
        let published = aggregate.route_counts.values().copied().sum::<u64>().max(1);
        aggregate.correction_rate = aggregate.correction_count as f64 / published as f64;

        let mut durations_ms: Vec<f64> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT started_at, completed_at FROM stage_runs
                     WHERE stage = ?1 AND started_at IS NOT NULL AND completed_at IS NOT NULL",
                )
                .map_err(storage_error)?;
            let rows = stmt
                .query_map(params![TRIAGE_STAGE_NAME], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(storage_error)?;
            for row in rows {
                let (started, completed) = row.map_err(storage_error)?;
                if let (Ok(start), Ok(end)) = (
                    DateTime::parse_from_rfc3339(&started),
                    DateTime::parse_from_rfc3339(&completed),
                ) {
                    let ms = (end.with_timezone(&Utc) - start.with_timezone(&Utc))
                        .num_milliseconds()
                        .max(0) as f64;
                    durations_ms.push(ms);
                }
            }
        }
        durations_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        aggregate.duration = if durations_ms.is_empty() {
            TriageMetricsDuration {
                average_ms: None,
                p50_ms: None,
                p95_ms: None,
            }
        } else {
            let sum: f64 = durations_ms.iter().sum();
            TriageMetricsDuration {
                average_ms: Some(sum / durations_ms.len() as f64),
                p50_ms: Some(percentile(&durations_ms, 0.50)),
                p95_ms: Some(percentile(&durations_ms, 0.95)),
            }
        };

        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT harness, model,
                            COALESCE(SUM(input_tokens), 0),
                            COALESCE(SUM(output_tokens), 0),
                            COALESCE(SUM(total_tokens), 0)
                     FROM stage_runs WHERE stage = ?1
                     GROUP BY harness, model",
                )
                .map_err(storage_error)?;
            let rows = stmt
                .query_map(params![TRIAGE_STAGE_NAME], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, u64>(2)?,
                        row.get::<_, u64>(3)?,
                        row.get::<_, u64>(4)?,
                    ))
                })
                .map_err(storage_error)?;
            let mut by_harness_model: BTreeMap<String, TriageMetricsTokenTotals> = BTreeMap::new();
            for row in rows {
                let (harness, model, input, output, total) = row.map_err(storage_error)?;
                let model_key = model
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("unknown");
                let key = format!("{harness}/{model_key}");
                by_harness_model.insert(
                    key,
                    TriageMetricsTokenTotals {
                        input_tokens: input,
                        output_tokens: output,
                        total_tokens: total,
                    },
                );
            }
            aggregate.tokens_by_harness_model = by_harness_model;
        }

        Ok(aggregate)
    }
}

impl SqliteFactoryStore {
    pub fn store_spec_turn(&mut self, request: StoreSpecTurnRequest) -> Result<SpecTurnRecord> {
        let stage = select_stage_run(&self.conn, &request.stage_run_id)?;
        if stage.stage != SPEC_STAGE_NAME || stage.status != StageStatus::Running {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is not a running spec attempt",
                request.stage_run_id
            )));
        }
        let output_json = request.output_json.as_ref().map(bounded_json).transpose()?;
        self.conn
            .execute(
                "INSERT INTO spec_turns (
                    turn_id, stage_run_id, ordinal, kind, status, harness, model,
                    input_tokens, output_tokens, total_tokens, output_json, error_json,
                    started_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    request.turn_id,
                    request.stage_run_id,
                    request.ordinal,
                    request.kind.as_str(),
                    request.status.as_str(),
                    request.harness,
                    request.model,
                    request.usage.input_tokens,
                    request.usage.output_tokens,
                    request.usage.total_tokens,
                    output_json,
                    optional_json(&request.error)?,
                    ts(request.started_at),
                    request.completed_at.map(ts),
                ],
            )
            .map_err(storage_error)?;
        self.get_spec_turn(&request.turn_id)?.ok_or_else(|| {
            SymphonyError::StorageError(format!("spec turn {} was not stored", request.turn_id))
        })
    }

    pub fn get_spec_turn(&self, turn_id: &str) -> Result<Option<SpecTurnRecord>> {
        self.conn
            .query_row(
                "SELECT turn_id, stage_run_id, ordinal, kind, status, harness, model,
                    input_tokens, output_tokens, total_tokens, output_json, error_json,
                    started_at, completed_at
                 FROM spec_turns WHERE turn_id = ?1",
                params![turn_id],
                spec_turn_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_spec_turns(&self, stage_run_id: &str) -> Result<Vec<SpecTurnRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT turn_id, stage_run_id, ordinal, kind, status, harness, model,
                    input_tokens, output_tokens, total_tokens, output_json, error_json,
                    started_at, completed_at
                 FROM spec_turns WHERE stage_run_id = ?1 ORDER BY ordinal ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![stage_run_id], spec_turn_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn store_spec_artifact(
        &mut self,
        request: StoreSpecArtifactRequest,
    ) -> Result<SpecArtifactRecord> {
        validate_spec(&request.artifact).map_err(|error| {
            SymphonyError::StorageError(format!("refusing invalid spec artifact: {error}"))
        })?;
        let now = Self::now();
        let now_s = ts(now);
        let tx = self.conn.transaction().map_err(storage_error)?;
        let stage = select_stage_run(&tx, &request.stage_run_id)?;
        if stage.stage != SPEC_STAGE_NAME || stage.status != StageStatus::Running {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is not a running spec attempt",
                request.stage_run_id
            )));
        }
        let version = tx
            .query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM spec_artifacts WHERE run_id = ?1",
                params![stage.run_id],
                |row| row.get::<_, u32>(0),
            )
            .map_err(storage_error)?;
        let artifact_id = new_id();
        tx.execute(
            "INSERT INTO spec_artifacts (
                artifact_id, run_id, stage_run_id, issue_revision, configuration_revision,
                version, schema_version, artifact_json, review_cycles,
                unresolved_findings_json, received_at, bytes_len
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                artifact_id,
                stage.run_id,
                request.stage_run_id,
                request.issue_revision,
                request.configuration_revision,
                version,
                request.artifact.schema_version,
                bounded_json(&request.artifact)?,
                request.review_cycles,
                bounded_json(&request.unresolved_blocking_findings)?,
                now_s,
                request.bytes_len,
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE stage_runs SET status = ?1, completed_at = ?2,
                input_tokens = ?3, output_tokens = ?4, total_tokens = ?5, updated_at = ?6
             WHERE stage_run_id = ?7",
            params![
                StageStatus::Completed.as_str(),
                now_s,
                request.usage.input_tokens,
                request.usage.output_tokens,
                request.usage.total_tokens,
                now_s,
                request.stage_run_id,
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "INSERT INTO spec_run_state (run_id, decision, updated_at)
             VALUES (?1, 'awaiting_decision', ?2)
             ON CONFLICT(run_id) DO UPDATE SET
                pending_approval_version = NULL,
                approved_version = NULL,
                approved_artifact_id = NULL,
                decision = 'awaiting_decision', updated_at = excluded.updated_at",
            params![stage.run_id, now_s],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE factory_runs SET status = ?1, current_stage = ?2, updated_at = ?3 WHERE run_id = ?4",
            params![FactoryRunStatus::Waiting.as_str(), SPEC_STAGE_NAME, now_s, stage.run_id],
        )
        .map_err(storage_error)?;
        let record = select_spec_artifact(&tx, &artifact_id)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }

    pub fn get_spec_artifact(&self, artifact_id: &str) -> Result<Option<SpecArtifactRecord>> {
        self.conn
            .query_row(
                SPEC_ARTIFACT_SELECT,
                params![artifact_id],
                spec_artifact_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_spec_artifacts(&self, run_id: &str) -> Result<Vec<SpecArtifactRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT artifact_id, run_id, stage_run_id, issue_revision,
                    configuration_revision, version, artifact_json, review_cycles,
                    unresolved_findings_json, received_at, bytes_len
                 FROM spec_artifacts WHERE run_id = ?1 ORDER BY version DESC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], spec_artifact_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn get_spec_state(&self, run_id: &str) -> Result<Option<SpecRunState>> {
        self.conn
            .query_row(
                "SELECT run_id, revision_requests_used, pending_approval_version,
                    approved_version, approved_artifact_id, decision, updated_at
                 FROM spec_run_state WHERE run_id = ?1",
                params![run_id],
                spec_state_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn increment_spec_revision_requests(&mut self, run_id: &str, max: u32) -> Result<u32> {
        let now_s = ts(Self::now());
        let changed = self
            .conn
            .execute(
                "UPDATE spec_run_state SET revision_requests_used = revision_requests_used + 1,
                decision = 'revision_requested', updated_at = ?1
             WHERE run_id = ?2 AND revision_requests_used < ?3",
                params![now_s, run_id, max],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "spec revision request budget exhausted for run {run_id}"
            )));
        }
        Ok(self
            .get_spec_state(run_id)?
            .expect("state exists")
            .revision_requests_used)
    }

    pub fn set_pending_spec_approval(&mut self, run_id: &str, version: u32) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE spec_run_state SET pending_approval_version = ?1,
                decision = 'pending_approval', updated_at = ?2 WHERE run_id = ?3",
                params![version, ts(Self::now()), run_id],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "spec state for run {run_id} not found"
            )));
        }
        Ok(())
    }

    pub fn pin_spec_approval(&mut self, run_id: &str, artifact_id: &str) -> Result<()> {
        let artifact = self.get_spec_artifact(artifact_id)?.ok_or_else(|| {
            SymphonyError::StorageError(format!("spec artifact {artifact_id} not found"))
        })?;
        if artifact.run_id != run_id {
            return Err(SymphonyError::StorageError(
                "approved artifact does not belong to factory run".to_string(),
            ));
        }
        let changed = self
            .conn
            .execute(
                "UPDATE spec_run_state SET approved_version = ?1,
                approved_artifact_id = ?2, decision = 'pending_approval', updated_at = ?3 WHERE run_id = ?4",
                params![artifact.version, artifact_id, ts(Self::now()), run_id],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "spec state for run {run_id} not found"
            )));
        }
        Ok(())
    }

    pub fn finalize_spec_approval(
        &mut self,
        run_id: &str,
        intent_id: &str,
        completed_step: &str,
    ) -> Result<()> {
        let now = ts(Self::now());
        let intent = self
            .get_spec_publication_intent(intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "spec publication intent {intent_id} not found"
                ))
            })?;
        let mut steps = intent.completed_steps;
        if !steps.iter().any(|step| step == completed_step) {
            steps.push(completed_step.to_string());
        }
        let tx = self.conn.transaction().map_err(storage_error)?;
        let changed = tx
            .execute(
                "UPDATE spec_run_state SET pending_approval_version = NULL,
                    decision = 'approved', updated_at = ?1
                 WHERE run_id = ?2 AND approved_version IS NOT NULL",
                params![now, run_id],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "cannot finalize spec approval for run {run_id} without a pinned version"
            )));
        }
        tx.execute(
            "UPDATE factory_runs SET status = 'completed', current_stage = ?1, updated_at = ?2
             WHERE run_id = ?3",
            params![SPEC_STAGE_NAME, now, run_id],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE spec_publication_intents SET status = 'applied',
                completed_steps_json = ?1, last_error_json = NULL, updated_at = ?2
             WHERE intent_id = ?3 AND run_id = ?4",
            params![bounded_json(&steps)?, now, intent_id, run_id],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(())
    }

    pub fn create_spec_publication_intent(
        &mut self,
        run_id: &str,
        artifact_id: Option<&str>,
        kind: SpecPublicationKind,
        desired_effects: &serde_json::Value,
    ) -> Result<SpecPublicationIntent> {
        let intent_id = new_id();
        let now_s = ts(Self::now());
        self.conn
            .execute(
                "INSERT INTO spec_publication_intents (
                intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                desired_effects_json, observed_baseline_json, expected_projection_json,
                created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', ?5, '{}', '{}', ?6, ?7)",
                params![
                    intent_id,
                    run_id,
                    artifact_id,
                    kind.as_str(),
                    bounded_json(desired_effects)?,
                    now_s,
                    now_s,
                ],
            )
            .map_err(storage_error)?;
        self.get_spec_publication_intent(&intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(
                    "created spec publication intent is missing".to_string(),
                )
            })
    }

    /// Re-open an applied diagnostic intent with a new message so the owned comment
    /// is updated in place rather than accumulating a new intent per poll.
    pub fn update_spec_diagnostic_message(
        &mut self,
        intent_id: &str,
        desired_effects: &serde_json::Value,
    ) -> Result<SpecPublicationIntent> {
        self.conn
            .execute(
                "UPDATE spec_publication_intents
                 SET desired_effects_json = ?1, status = 'pending', completed_steps_json = '[]',
                     updated_at = ?2
                 WHERE intent_id = ?3",
                params![bounded_json(desired_effects)?, ts(Self::now()), intent_id],
            )
            .map_err(storage_error)?;
        self.get_spec_publication_intent(intent_id)?.ok_or_else(|| {
            SymphonyError::StorageError(format!("spec publication intent {intent_id} not found"))
        })
    }

    pub fn get_spec_publication_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<SpecPublicationIntent>> {
        self.conn
            .query_row(
                SPEC_PUBLICATION_SELECT,
                params![intent_id],
                spec_publication_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn get_latest_spec_publication(
        &self,
        run_id: &str,
    ) -> Result<Option<SpecPublicationIntent>> {
        self.conn
            .query_row(
                "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                retry_count, last_error_json, comment_id, publisher_login,
                desired_effects_json, observed_baseline_json, expected_projection_json,
                created_at, updated_at
             FROM spec_publication_intents WHERE run_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![run_id],
                spec_publication_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    /// Return the run's most recent spec intent of `kind`. Used for the feedback
    /// cutoff, which must key on the last spec publication rather than any later
    /// diagnostic republish of the same comment.
    pub fn latest_spec_publication_of_kind(
        &self,
        run_id: &str,
        kind: SpecPublicationKind,
    ) -> Result<Option<SpecPublicationIntent>> {
        self.conn
            .query_row(
                "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                retry_count, last_error_json, comment_id, publisher_login,
                desired_effects_json, observed_baseline_json, expected_projection_json,
                created_at, updated_at
             FROM spec_publication_intents WHERE run_id = ?1 AND kind = ?2
             ORDER BY created_at DESC LIMIT 1",
                params![run_id, kind.as_str()],
                spec_publication_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    /// Return the run's existing spec intent of `kind`, oldest first. Diagnostic
    /// intents are reused across polls so a persistently ineligible issue does not
    /// accumulate one intent per poll.
    pub fn find_spec_publication_by_kind(
        &self,
        run_id: &str,
        kind: SpecPublicationKind,
    ) -> Result<Option<SpecPublicationIntent>> {
        self.conn
            .query_row(
                "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                retry_count, last_error_json, comment_id, publisher_login,
                desired_effects_json, observed_baseline_json, expected_projection_json,
                created_at, updated_at
             FROM spec_publication_intents WHERE run_id = ?1 AND kind = ?2
             ORDER BY created_at ASC LIMIT 1",
                params![run_id, kind.as_str()],
                spec_publication_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_pending_spec_publications(&self) -> Result<Vec<SpecPublicationIntent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                retry_count, last_error_json, comment_id, publisher_login,
                desired_effects_json, observed_baseline_json, expected_projection_json,
                created_at, updated_at
             FROM spec_publication_intents WHERE status = 'pending' ORDER BY created_at ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], spec_publication_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn set_spec_publication_comment(
        &mut self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE spec_publication_intents SET comment_id = ?1, publisher_login = ?2,
                updated_at = ?3 WHERE intent_id = ?4",
                params![comment_id, publisher_login, ts(Self::now()), intent_id],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn complete_spec_publication(&mut self, intent_id: &str, step: &str) -> Result<()> {
        let intent = self
            .get_spec_publication_intent(intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "spec publication intent {intent_id} not found"
                ))
            })?;
        let mut steps = intent.completed_steps;
        if !steps.iter().any(|existing| existing == step) {
            steps.push(step.to_string());
        }
        self.conn
            .execute(
                "UPDATE spec_publication_intents SET status = 'applied', completed_steps_json = ?1,
                last_error_json = NULL, updated_at = ?2 WHERE intent_id = ?3",
                params![bounded_json(&steps)?, ts(Self::now()), intent_id],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    /// Persist a spec publication failure (or intermediate status) and bump
    /// `retry_count` when an error is recorded, mirroring `update_publication_step`.
    pub fn update_spec_publication_step(
        &mut self,
        intent_id: &str,
        completed_step: &str,
        status: PublicationStatus,
        error: Option<FactoryError>,
    ) -> Result<()> {
        let intent = self
            .get_spec_publication_intent(intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "spec publication intent {intent_id} not found"
                ))
            })?;
        let mut steps = intent.completed_steps;
        if !completed_step.trim().is_empty()
            && !steps.iter().any(|step| step == completed_step.trim())
        {
            steps.push(completed_step.trim().to_string());
        }
        self.conn
            .execute(
                "UPDATE spec_publication_intents
                 SET completed_steps_json = ?1, status = ?2, last_error_json = ?3,
                     retry_count = retry_count + CASE WHEN ?3 IS NULL THEN 0 ELSE 1 END,
                     updated_at = ?4
                 WHERE intent_id = ?5",
                params![
                    bounded_json(&steps)?,
                    status.as_str(),
                    optional_json(&error)?,
                    ts(Self::now()),
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn spec_metrics(&self) -> Result<crate::spec::domain::SpecMetricsAggregate> {
        use crate::spec::domain::{SpecMetricsAggregate, SpecReviewCycleMetrics};
        use crate::triage::domain::{TriageMetricsDuration, TriageMetricsTokenTotals};
        use std::collections::BTreeMap;

        let mut aggregate = TriageMetricsAggregate::default();
        let (total, completed, failed): (u64, u64, u64) = self
            .conn
            .query_row(
                "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status IN ('failed', 'interrupted') THEN 1 ELSE 0 END), 0)
             FROM stage_runs WHERE stage = ?1",
                params![SPEC_STAGE_NAME],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(storage_error)?;
        aggregate.total_attempts = total;
        aggregate.completed_attempts = completed;
        aggregate.failed_attempts = failed;
        aggregate.ineligible_issues = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM factory_events WHERE event_type = 'spec_ineligible'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;

        let mut durations = Vec::new();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT started_at, completed_at FROM stage_runs
             WHERE stage = ?1 AND started_at IS NOT NULL AND completed_at IS NOT NULL",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![SPEC_STAGE_NAME], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (start, end) = row.map_err(storage_error)?;
            durations.push(
                (parse_ts(&end).map_err(SymphonyError::StorageError)?
                    - parse_ts(&start).map_err(SymphonyError::StorageError)?)
                .num_milliseconds()
                .max(0) as f64,
            );
        }
        durations.sort_by(f64::total_cmp);
        aggregate.duration = if durations.is_empty() {
            TriageMetricsDuration {
                average_ms: None,
                p50_ms: None,
                p95_ms: None,
            }
        } else {
            TriageMetricsDuration {
                average_ms: Some(durations.iter().sum::<f64>() / durations.len() as f64),
                p50_ms: Some(percentile(&durations, 0.50)),
                p95_ms: Some(percentile(&durations, 0.95)),
            }
        };

        let mut stmt = self
            .conn
            .prepare(
                "SELECT harness, model, COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0), COALESCE(SUM(total_tokens), 0)
             FROM spec_turns GROUP BY harness, model",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                ))
            })
            .map_err(storage_error)?;
        let mut tokens = BTreeMap::new();
        for row in rows {
            let (harness, model, input, output, total) = row.map_err(storage_error)?;
            aggregate.input_tokens = aggregate.input_tokens.saturating_add(input);
            aggregate.output_tokens = aggregate.output_tokens.saturating_add(output);
            aggregate.total_tokens = aggregate.total_tokens.saturating_add(total);
            tokens.insert(
                format!("{}/{}", harness, model.as_deref().unwrap_or("unknown")),
                TriageMetricsTokenTotals {
                    input_tokens: input,
                    output_tokens: output,
                    total_tokens: total,
                },
            );
        }
        aggregate.tokens_by_harness_model = tokens;

        // Review-loop measures: cycles used per published version and convergence
        // (attempts that published with zero unresolved blocking findings).
        let mut cycles = Vec::new();
        let mut converged = 0u64;
        let mut published = 0u64;
        let mut stmt = self
            .conn
            .prepare("SELECT review_cycles, unresolved_findings_json FROM spec_artifacts")
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (review_cycles, findings_json) = row.map_err(storage_error)?;
            cycles.push(review_cycles as f64);
            published = published.saturating_add(1);
            let unresolved: Vec<serde_json::Value> =
                serde_json::from_str(&findings_json).unwrap_or_default();
            if unresolved.is_empty() {
                converged = converged.saturating_add(1);
            }
        }
        let review_cycle_metrics = SpecReviewCycleMetrics {
            average: (!cycles.is_empty()).then(|| cycles.iter().sum::<f64>() / cycles.len() as f64),
            max: cycles
                .iter()
                .copied()
                .reduce(f64::max)
                .map(|value| value as u64),
        };
        let convergence_rate = if published == 0 {
            0.0
        } else {
            converged as f64 / published as f64
        };
        let revision_requests: u64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(revision_requests_used), 0) FROM spec_run_state",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;

        // Approval latency is measured from the preview publication of the approved
        // version to the moment the human's approval was detected (approval intent
        // created). Joining on artifact keeps revision cycles from mixing versions.
        let mut latencies = Vec::new();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT preview.updated_at, approval.created_at
             FROM spec_publication_intents approval
             JOIN spec_publication_intents preview
               ON preview.run_id = approval.run_id
              AND preview.artifact_id = approval.artifact_id
              AND preview.kind = 'preview'
              AND preview.intent_id != approval.intent_id
             WHERE approval.kind = 'approval'",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (published_at, decided_at) = row.map_err(storage_error)?;
            latencies.push(
                (parse_ts(&decided_at).map_err(SymphonyError::StorageError)?
                    - parse_ts(&published_at).map_err(SymphonyError::StorageError)?)
                .num_milliseconds()
                .max(0) as f64,
            );
        }
        latencies.sort_by(f64::total_cmp);
        let approval_latency = if latencies.is_empty() {
            TriageMetricsDuration {
                average_ms: None,
                p50_ms: None,
                p95_ms: None,
            }
        } else {
            TriageMetricsDuration {
                average_ms: Some(latencies.iter().sum::<f64>() / latencies.len() as f64),
                p50_ms: Some(percentile(&latencies, 0.50)),
                p95_ms: Some(percentile(&latencies, 0.95)),
            }
        };

        Ok(SpecMetricsAggregate {
            base: aggregate,
            review_cycles: review_cycle_metrics,
            converged_attempts: converged,
            convergence_rate,
            revision_requests,
            approval_latency,
        })
    }

    pub fn store_implementation_turn(
        &mut self,
        request: StoreImplementationTurnRequest,
    ) -> Result<ImplementationTurnRecord> {
        let stage = select_stage_run(&self.conn, &request.stage_run_id)?;
        if stage.stage != IMPLEMENTATION_STAGE_NAME || stage.status != StageStatus::Running {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is not a running implementation attempt",
                request.stage_run_id
            )));
        }
        let output_json = request.output_json.as_ref().map(bounded_json).transpose()?;
        self.conn
            .execute(
                "INSERT INTO implementation_turns (
                    turn_id, stage_run_id, ordinal, kind, status, harness, model,
                    execution_profile, input_tokens, output_tokens, total_tokens,
                    output_json, error_json, started_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    request.turn_id,
                    request.stage_run_id,
                    request.ordinal,
                    request.kind.as_str(),
                    request.status.as_str(),
                    request.harness,
                    request.model,
                    request.execution_profile.as_str(),
                    request.usage.input_tokens,
                    request.usage.output_tokens,
                    request.usage.total_tokens,
                    output_json,
                    optional_json(&request.error)?,
                    ts(request.started_at),
                    request.completed_at.map(ts),
                ],
            )
            .map_err(storage_error)?;
        self.list_implementation_turns(&request.stage_run_id)?
            .into_iter()
            .find(|turn| turn.turn_id == request.turn_id)
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "implementation turn {} was not stored",
                    request.turn_id
                ))
            })
    }

    pub fn list_implementation_turns(
        &self,
        stage_run_id: &str,
    ) -> Result<Vec<ImplementationTurnRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT turn_id, stage_run_id, ordinal, kind, status, harness, model,
                    execution_profile, input_tokens, output_tokens, total_tokens,
                    output_json, error_json, started_at, completed_at
                 FROM implementation_turns WHERE stage_run_id = ?1 ORDER BY ordinal ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![stage_run_id], implementation_turn_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn store_validation_cycle(
        &mut self,
        request: StoreValidationCycleRequest,
    ) -> Result<ValidationCycleRecord> {
        let stage = select_stage_run(&self.conn, &request.stage_run_id)?;
        if stage.stage != IMPLEMENTATION_STAGE_NAME {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is not an implementation attempt",
                request.stage_run_id
            )));
        }
        self.conn
            .execute(
                "INSERT INTO implementation_validation_cycles (
                    cycle_id, stage_run_id, cycle, passed, results_json, started_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    request.cycle_id,
                    request.stage_run_id,
                    request.cycle,
                    request.passed as i64,
                    bounded_json(&request.commands)?,
                    ts(request.started_at),
                    ts(request.completed_at),
                ],
            )
            .map_err(storage_error)?;
        self.list_validation_cycles(&request.stage_run_id)?
            .into_iter()
            .find(|cycle| cycle.cycle_id == request.cycle_id)
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "validation cycle {} was not stored",
                    request.cycle_id
                ))
            })
    }

    pub fn list_validation_cycles(&self, stage_run_id: &str) -> Result<Vec<ValidationCycleRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT cycle_id, stage_run_id, cycle, passed, results_json, started_at, completed_at
                 FROM implementation_validation_cycles
                 WHERE stage_run_id = ?1 ORDER BY cycle ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![stage_run_id], validation_cycle_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn store_implementation_artifact(
        &mut self,
        request: StoreImplementationArtifactRequest,
    ) -> Result<ImplementationArtifactRecord> {
        crate::implementation::artifact::validate_manifest(&request.manifest, None).map_err(
            |error| {
                SymphonyError::StorageError(format!(
                    "refusing invalid implementation artifact: {error}"
                ))
            },
        )?;
        let now = Self::now();
        let now_s = ts(now);
        let tx = self.conn.transaction().map_err(storage_error)?;
        let stage = select_stage_run(&tx, &request.stage_run_id)?;
        if stage.stage != IMPLEMENTATION_STAGE_NAME || stage.status != StageStatus::Running {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is not a running implementation attempt",
                request.stage_run_id
            )));
        }
        let artifact_id = new_id();
        tx.execute(
            "INSERT INTO implementation_artifacts (
                artifact_id, run_id, stage_run_id, approved_artifact_id, approved_version,
                issue_revision, configuration_revision, schema_version, manifest_json,
                base_commit, head_commit, approved_spec_path, validation_cycles,
                execution_profile, received_at, bytes_len
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                artifact_id,
                stage.run_id,
                request.stage_run_id,
                request.approved_artifact_id,
                request.approved_version,
                request.issue_revision,
                request.configuration_revision,
                request.manifest.schema_version,
                bounded_json(&request.manifest)?,
                request.base_commit,
                request.head_commit,
                request.approved_spec_path,
                request.validation_cycles,
                request.execution_profile.as_str(),
                now_s,
                request.bytes_len,
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE stage_runs SET status = ?1, completed_at = ?2,
                input_tokens = ?3, output_tokens = ?4, total_tokens = ?5, updated_at = ?6
             WHERE stage_run_id = ?7",
            params![
                StageStatus::Completed.as_str(),
                now_s,
                request.usage.input_tokens,
                request.usage.output_tokens,
                request.usage.total_tokens,
                now_s,
                request.stage_run_id,
            ],
        )
        .map_err(storage_error)?;
        let decision = match request.manifest.status {
            crate::implementation::domain::ManifestStatus::Completed => {
                ImplementationDecision::Previewed
            }
            crate::implementation::domain::ManifestStatus::Blocked => {
                ImplementationDecision::AwaitingHuman
            }
        };
        let successful = if matches!(
            request.manifest.status,
            crate::implementation::domain::ManifestStatus::Completed
        ) {
            Some(artifact_id.as_str())
        } else {
            None
        };
        tx.execute(
            "INSERT INTO implementation_run_state (
                run_id, approved_artifact_id, approved_version, configuration_revision,
                decision, successful_artifact_id, bundle_artifact_id, blocker_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8)
             ON CONFLICT(run_id) DO UPDATE SET
                approved_artifact_id = excluded.approved_artifact_id,
                approved_version = excluded.approved_version,
                configuration_revision = excluded.configuration_revision,
                decision = excluded.decision,
                successful_artifact_id = excluded.successful_artifact_id,
                blocker_json = excluded.blocker_json,
                updated_at = excluded.updated_at",
            params![
                stage.run_id,
                request.approved_artifact_id,
                request.approved_version,
                request.configuration_revision,
                decision.as_str(),
                successful,
                optional_json(&request.manifest.blocker)?,
                now_s,
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE factory_runs SET status = ?1, current_stage = ?2, updated_at = ?3 WHERE run_id = ?4",
            params![
                FactoryRunStatus::Waiting.as_str(),
                IMPLEMENTATION_STAGE_NAME,
                now_s,
                stage.run_id
            ],
        )
        .map_err(storage_error)?;
        let record = select_implementation_artifact(&tx, &artifact_id)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }

    pub fn get_implementation_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<ImplementationArtifactRecord>> {
        self.conn
            .query_row(
                IMPLEMENTATION_ARTIFACT_SELECT,
                params![artifact_id],
                implementation_artifact_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_implementation_artifacts(
        &self,
        run_id: &str,
    ) -> Result<Vec<ImplementationArtifactRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT artifact_id, run_id, stage_run_id, approved_artifact_id, approved_version,
                    issue_revision, configuration_revision, manifest_json, base_commit,
                    head_commit, approved_spec_path, validation_cycles, execution_profile,
                    received_at, bytes_len
                 FROM implementation_artifacts WHERE run_id = ?1 ORDER BY received_at DESC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], implementation_artifact_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn store_bundle_artifact(
        &mut self,
        request: StoreBundleArtifactRequest,
    ) -> Result<BundleArtifactRecord> {
        let stage = select_stage_run(&self.conn, &request.stage_run_id)?;
        if stage.stage != IMPLEMENTATION_STAGE_NAME {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is not an implementation attempt",
                request.stage_run_id
            )));
        }
        // Content-addressed reuse: identical sha256 returns the existing row.
        if let Some(existing_id) = self
            .conn
            .query_row(
                "SELECT artifact_id FROM implementation_bundle_artifacts WHERE sha256 = ?1",
                params![request.sha256],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?
        {
            self.conn
                .execute(
                    "UPDATE implementation_run_state SET bundle_artifact_id = ?1, updated_at = ?2
                     WHERE run_id = ?3",
                    params![existing_id, ts(Self::now()), stage.run_id],
                )
                .map_err(storage_error)?;
            return self.get_bundle_artifact(&existing_id)?.ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "bundle artifact {existing_id} was not found after reuse"
                ))
            });
        }
        let artifact_id = new_id();
        self.conn
            .execute(
                "INSERT INTO implementation_bundle_artifacts (
                    artifact_id, run_id, stage_run_id, implementation_artifact_id,
                    base_commit, head_commit, tree_sha, sha256, bytes_len, changed_file_count,
                    changed_paths_json, approved_spec_path, approved_spec_blob_sha,
                    harness, model, execution_profile, created_at, verified_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                params![
                    artifact_id,
                    stage.run_id,
                    request.stage_run_id,
                    request.implementation_artifact_id,
                    request.base_commit,
                    request.head_commit,
                    request.tree_sha,
                    request.sha256,
                    request.bytes_len,
                    request.changed_file_count,
                    bounded_json(&request.changed_paths)?,
                    request.approved_spec_path,
                    request.approved_spec_blob_sha,
                    request.harness,
                    request.model,
                    request.execution_profile.as_str(),
                    ts(request.created_at),
                    ts(request.verified_at),
                ],
            )
            .map_err(storage_error)?;
        self.conn
            .execute(
                "UPDATE implementation_run_state SET bundle_artifact_id = ?1, updated_at = ?2
                 WHERE run_id = ?3",
                params![artifact_id, ts(Self::now()), stage.run_id],
            )
            .map_err(storage_error)?;
        self.get_bundle_artifact(&artifact_id)?.ok_or_else(|| {
            SymphonyError::StorageError(format!("bundle artifact {artifact_id} was not stored"))
        })
    }

    pub fn get_bundle_artifact(&self, artifact_id: &str) -> Result<Option<BundleArtifactRecord>> {
        self.conn
            .query_row(
                IMPLEMENTATION_BUNDLE_SELECT,
                params![artifact_id],
                bundle_artifact_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn get_implementation_state(&self, run_id: &str) -> Result<Option<ImplementationRunState>> {
        self.conn
            .query_row(
                "SELECT run_id, approved_artifact_id, approved_version, configuration_revision,
                    decision, successful_artifact_id, bundle_artifact_id, blocker_json, updated_at
                 FROM implementation_run_state WHERE run_id = ?1",
                params![run_id],
                implementation_state_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn upsert_implementation_state(
        &mut self,
        request: UpsertImplementationStateRequest,
    ) -> Result<ImplementationRunState> {
        let now_s = ts(Self::now());
        self.conn
            .execute(
                "INSERT INTO implementation_run_state (
                    run_id, approved_artifact_id, approved_version, configuration_revision,
                    decision, successful_artifact_id, bundle_artifact_id, blocker_json, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(run_id) DO UPDATE SET
                    approved_artifact_id = excluded.approved_artifact_id,
                    approved_version = excluded.approved_version,
                    configuration_revision = excluded.configuration_revision,
                    decision = excluded.decision,
                    successful_artifact_id = excluded.successful_artifact_id,
                    bundle_artifact_id = excluded.bundle_artifact_id,
                    blocker_json = excluded.blocker_json,
                    updated_at = excluded.updated_at",
                params![
                    request.run_id,
                    request.approved_artifact_id,
                    request.approved_version,
                    request.configuration_revision,
                    request.decision.as_str(),
                    request.successful_artifact_id,
                    request.bundle_artifact_id,
                    optional_json(&request.blocker)?,
                    now_s,
                ],
            )
            .map_err(storage_error)?;
        self.get_implementation_state(&request.run_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "implementation state for run {} missing after upsert",
                    request.run_id
                ))
            })
    }

    pub fn set_implementation_decision(
        &mut self,
        run_id: &str,
        decision: ImplementationDecision,
        blocker: Option<ImplementationBlocker>,
    ) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE implementation_run_state SET decision = ?1, blocker_json = ?2, updated_at = ?3
                 WHERE run_id = ?4",
                params![
                    decision.as_str(),
                    optional_json(&blocker)?,
                    ts(Self::now()),
                    run_id
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "implementation state for run {run_id} not found"
            )));
        }
        Ok(())
    }

    pub fn create_implementation_publication_intent(
        &mut self,
        run_id: &str,
        artifact_id: Option<&str>,
        kind: ImplementationPublicationKind,
        desired_effects: &serde_json::Value,
    ) -> Result<ImplementationPublicationIntent> {
        let intent_id = new_id();
        let now_s = ts(Self::now());
        self.conn
            .execute(
                "INSERT INTO implementation_publication_intents (
                    intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                    desired_effects_json, observed_baseline_json, expected_projection_json,
                    created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', ?5, '{}', '{}', ?6, ?7)",
                params![
                    intent_id,
                    run_id,
                    artifact_id,
                    kind.as_str(),
                    bounded_json(desired_effects)?,
                    now_s,
                    now_s,
                ],
            )
            .map_err(storage_error)?;
        self.get_implementation_publication_intent(&intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(
                    "created implementation publication intent is missing".to_string(),
                )
            })
    }

    pub fn get_implementation_publication_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<ImplementationPublicationIntent>> {
        self.conn
            .query_row(
                IMPLEMENTATION_PUBLICATION_SELECT,
                params![intent_id],
                implementation_publication_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_pending_implementation_publications(
        &self,
    ) -> Result<Vec<ImplementationPublicationIntent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                    retry_count, last_error_json, comment_id, publisher_login,
                    desired_effects_json, observed_baseline_json, expected_projection_json,
                    created_at, updated_at
                 FROM implementation_publication_intents
                 WHERE status = 'pending' ORDER BY created_at ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], implementation_publication_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn list_implementation_publications_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<ImplementationPublicationIntent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                    retry_count, last_error_json, comment_id, publisher_login,
                    desired_effects_json, observed_baseline_json, expected_projection_json,
                    created_at, updated_at
                 FROM implementation_publication_intents
                 WHERE run_id = ?1 ORDER BY created_at DESC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], implementation_publication_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn complete_implementation_publication(
        &mut self,
        intent_id: &str,
        step: &str,
    ) -> Result<()> {
        let intent = self
            .get_implementation_publication_intent(intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "implementation publication intent {intent_id} not found"
                ))
            })?;
        let mut steps = intent.completed_steps;
        if !steps.iter().any(|existing| existing == step) {
            steps.push(step.to_string());
        }
        self.conn
            .execute(
                "UPDATE implementation_publication_intents SET status = 'applied',
                    completed_steps_json = ?1, last_error_json = NULL, updated_at = ?2
                 WHERE intent_id = ?3",
                params![bounded_json(&steps)?, ts(Self::now()), intent_id],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    /// Append a publication step and update status/expected projection without
    /// forcing `applied` (A1-style progressive reconciliation for automatic mode).
    pub fn record_implementation_publication_step(
        &mut self,
        intent_id: &str,
        step: &str,
        status: PublicationStatus,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        let intent = self
            .get_implementation_publication_intent(intent_id)?
            .ok_or_else(|| {
                SymphonyError::StorageError(format!(
                    "implementation publication intent {intent_id} not found"
                ))
            })?;
        let mut steps = intent.completed_steps;
        let trimmed = step.trim();
        if !trimmed.is_empty() && !steps.iter().any(|existing| existing == trimmed) {
            steps.push(trimmed.to_string());
        }
        self.conn
            .execute(
                "UPDATE implementation_publication_intents
                 SET completed_steps_json = ?1, status = ?2, last_error_json = NULL,
                     expected_projection_json = ?3, updated_at = ?4
                 WHERE intent_id = ?5",
                params![
                    bounded_json(&steps)?,
                    status.as_str(),
                    bounded_json(expected_projection)?,
                    ts(Self::now()),
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn set_implementation_publication_baseline(
        &mut self,
        intent_id: &str,
        observed_baseline: &serde_json::Value,
        expected_projection: &serde_json::Value,
    ) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE implementation_publication_intents
                 SET observed_baseline_json = ?1, expected_projection_json = ?2, updated_at = ?3
                 WHERE intent_id = ?4",
                params![
                    bounded_json(observed_baseline)?,
                    bounded_json(expected_projection)?,
                    ts(Self::now()),
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "implementation publication intent {intent_id} not found"
            )));
        }
        Ok(())
    }

    pub fn set_implementation_publication_error(
        &mut self,
        intent_id: &str,
        status: PublicationStatus,
        error: FactoryError,
    ) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE implementation_publication_intents
                 SET status = ?1, last_error_json = ?2,
                     retry_count = retry_count + 1, updated_at = ?3
                 WHERE intent_id = ?4",
                params![
                    status.as_str(),
                    optional_json(&Some(error))?,
                    ts(Self::now()),
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "implementation publication intent {intent_id} not found"
            )));
        }
        Ok(())
    }

    /// List publication intents terminalized as `blocked`, oldest first.
    ///
    /// These have left the reconcile poll set permanently, so this is the only
    /// way an operator can discover the intent ids that need recovery.
    pub fn list_blocked_implementation_publications(
        &self,
    ) -> Result<Vec<ImplementationPublicationIntent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
                    retry_count, last_error_json, comment_id, publisher_login,
                    desired_effects_json, observed_baseline_json, expected_projection_json,
                    created_at, updated_at
                 FROM implementation_publication_intents
                 WHERE status = 'blocked' ORDER BY created_at ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], implementation_publication_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    /// Operator recovery: return a `blocked` publication intent to `pending` so
    /// the reconcile loop picks it up again.
    ///
    /// Completed steps are preserved, so progressive publication resumes where
    /// it stopped rather than redoing durable work.
    ///
    /// Deliberately scoped to `blocked`. `conflict` records observed drift or a
    /// pull request owned by someone else, where blindly re-attempting could
    /// publish twice; clearing one needs a human to establish what actually
    /// changed on the forge first.
    /// The reset and its audit event commit together. Recording the
    /// intervention is part of the operation, not a follow-up: a partial commit
    /// would leave the intent pending while the caller saw an error, and the
    /// obvious retry would then report "not blocked" against a timeline that
    /// never recorded who cleared it or what error was discarded.
    pub fn reset_blocked_implementation_publication(
        &mut self,
        intent_id: &str,
        operator: &str,
    ) -> Result<ImplementationPublicationIntent> {
        let Some(intent) = self.get_implementation_publication_intent(intent_id)? else {
            return Err(SymphonyError::StorageError(format!(
                "implementation publication intent {intent_id} not found"
            )));
        };
        if intent.status != PublicationStatus::Blocked {
            return Err(SymphonyError::TriageError(format!(
                "implementation publication intent {intent_id} is {}, not blocked; \
                 only a blocked intent can be reset",
                intent.status.as_str()
            )));
        }

        let payload = bounded_json(&serde_json::json!({
            "intent_id": intent.intent_id,
            "operator": operator,
            "cleared_error": intent.last_error,
            "completed_steps": intent.completed_steps,
        }))?;
        let now = Self::now();
        let tx = self.conn.transaction().map_err(storage_error)?;
        let changed = tx
            .execute(
                "UPDATE implementation_publication_intents
                 SET status = ?1, last_error_json = NULL, retry_count = 0, updated_at = ?2
                 WHERE intent_id = ?3 AND status = ?4",
                params![
                    PublicationStatus::Pending.as_str(),
                    ts(now),
                    intent_id,
                    PublicationStatus::Blocked.as_str(),
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::TriageError(format!(
                "implementation publication intent {intent_id} changed status concurrently; \
                 re-run list-blocked and retry"
            )));
        }
        tx.execute(
            "INSERT INTO factory_events (
                event_id, run_id, stage_run_id, event_type, timestamp, payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                new_id(),
                Some(intent.run_id.as_str()),
                None::<String>,
                "implementation_publication_reset",
                ts(now),
                payload,
            ],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;

        // Deliberately not a reload. The transaction has committed, so a fallible
        // read here would put an already-durable reset behind a failure the caller
        // cannot distinguish from a failed one — and the retry would then be
        // refused with "not blocked". Return what the transaction just wrote.
        Ok(ImplementationPublicationIntent {
            status: PublicationStatus::Pending,
            retry_count: 0,
            last_error: None,
            updated_at: now,
            ..intent
        })
    }

    /// Record a publication condition that is waiting on an unmet precondition
    /// rather than on a failed attempt, leaving `retry_count` untouched.
    ///
    /// Conditions like issue-revision drift clear only when a human re-approves
    /// the issue, which can take arbitrarily long. Charging them to the retry
    /// budget consumed by [`Self::set_implementation_publication_error`] would
    /// let a normal review delay exhaust the budget and terminalize the intent
    /// as `blocked`, which no code path recovers from.
    pub fn set_implementation_publication_waiting(
        &mut self,
        intent_id: &str,
        status: PublicationStatus,
        error: FactoryError,
    ) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE implementation_publication_intents
                 SET status = ?1, last_error_json = ?2, updated_at = ?3
                 WHERE intent_id = ?4",
                params![
                    status.as_str(),
                    optional_json(&Some(error))?,
                    ts(Self::now()),
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "implementation publication intent {intent_id} not found"
            )));
        }
        Ok(())
    }

    pub fn store_draft_pr_artifact(
        &mut self,
        request: StoreDraftPrArtifactRequest,
    ) -> Result<DraftPrArtifactRecord> {
        let artifact_id = new_id();
        let now = Self::now();
        let now_s = ts(now);
        self.conn
            .execute(
                "INSERT INTO implementation_draft_pr_artifacts (
                    artifact_id, run_id, implementation_artifact_id, intent_id,
                    number, url, draft, head, base, head_sha, marker, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    artifact_id,
                    request.run_id,
                    request.implementation_artifact_id,
                    request.intent_id,
                    request.number as i64,
                    request.url,
                    if request.draft { 1 } else { 0 },
                    request.head,
                    request.base,
                    request.head_sha,
                    request.marker,
                    now_s,
                ],
            )
            .map_err(storage_error)?;
        self.get_draft_pr_artifact(&artifact_id)?.ok_or_else(|| {
            SymphonyError::StorageError("created draft PR artifact is missing".to_string())
        })
    }

    pub fn get_draft_pr_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<DraftPrArtifactRecord>> {
        self.conn
            .query_row(
                "SELECT artifact_id, run_id, implementation_artifact_id, intent_id,
                    number, url, draft, head, base, head_sha, marker, created_at
                 FROM implementation_draft_pr_artifacts WHERE artifact_id = ?1",
                params![artifact_id],
                draft_pr_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn get_draft_pr_for_implementation_artifact(
        &self,
        implementation_artifact_id: &str,
    ) -> Result<Option<DraftPrArtifactRecord>> {
        self.conn
            .query_row(
                "SELECT artifact_id, run_id, implementation_artifact_id, intent_id,
                    number, url, draft, head, base, head_sha, marker, created_at
                 FROM implementation_draft_pr_artifacts
                 WHERE implementation_artifact_id = ?1",
                params![implementation_artifact_id],
                draft_pr_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn get_draft_pr_for_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<DraftPrArtifactRecord>> {
        self.conn
            .query_row(
                "SELECT artifact_id, run_id, implementation_artifact_id, intent_id,
                    number, url, draft, head, base, head_sha, marker, created_at
                 FROM implementation_draft_pr_artifacts WHERE intent_id = ?1",
                params![intent_id],
                draft_pr_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn bind_implementation_publication_comment(
        &mut self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE implementation_publication_intents
                 SET comment_id = ?1, publisher_login = ?2, updated_at = ?3
                 WHERE intent_id = ?4",
                params![comment_id, publisher_login, ts(Self::now()), intent_id],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn store_implementation_attempt_inputs(
        &mut self,
        stage_run_id: &str,
        inputs: &ImplementationAttemptInputs,
    ) -> Result<()> {
        let stage = select_stage_run(&self.conn, stage_run_id)?;
        if stage.stage != IMPLEMENTATION_STAGE_NAME {
            return Err(SymphonyError::StorageError(format!(
                "stage run {stage_run_id} is not an implementation attempt"
            )));
        }
        self.conn
            .execute(
                "INSERT INTO implementation_attempt_inputs (
                    stage_run_id, approved_artifact_id, approved_version,
                    approval_issue_revision, base_branch, base_commit, execution_profile,
                    configuration_revision, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    stage_run_id,
                    inputs.approved_artifact_id,
                    inputs.approved_version,
                    inputs.approval_issue_revision,
                    inputs.base_branch,
                    inputs.base_commit,
                    inputs.execution_profile.as_str(),
                    inputs.configuration_revision,
                    ts(Self::now()),
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn get_implementation_attempt_inputs(
        &self,
        stage_run_id: &str,
    ) -> Result<Option<ImplementationAttemptInputs>> {
        self.conn
            .query_row(
                "SELECT approved_artifact_id, approved_version, approval_issue_revision,
                    base_branch, base_commit, execution_profile, configuration_revision
                 FROM implementation_attempt_inputs WHERE stage_run_id = ?1",
                params![stage_run_id],
                |row| {
                    let profile: String = row.get(5)?;
                    Ok(ImplementationAttemptInputs {
                        approved_artifact_id: row.get(0)?,
                        approved_version: row.get(1)?,
                        approval_issue_revision: row.get(2)?,
                        base_branch: row.get(3)?,
                        base_commit: row.get(4)?,
                        execution_profile: parse_execution_profile(&profile).map_err(row_error)?,
                        configuration_revision: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)
    }

    /// Approved A2 runs that may be claimed by A3 for `configuration_revision`.
    pub fn list_a3_eligible_approved_runs(
        &self,
        configuration_revision: &str,
    ) -> Result<Vec<A3EligibleApprovedRun>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT r.run_id, r.forge_host, r.repository, r.issue_id, r.issue_identifier,
                    r.issue_revision, s.approved_artifact_id, s.approved_version
                 FROM factory_runs r
                 JOIN spec_run_state s ON s.run_id = r.run_id
                 WHERE s.decision = 'approved'
                   AND s.approved_artifact_id IS NOT NULL
                   AND s.approved_version IS NOT NULL
                   AND EXISTS (
                     SELECT 1 FROM spec_publication_intents p
                     WHERE p.run_id = r.run_id AND p.kind = 'approval' AND p.status = 'applied'
                   )
                   AND NOT EXISTS (
                     SELECT 1 FROM implementation_artifacts a
                     WHERE a.run_id = r.run_id
                       AND a.approved_artifact_id = s.approved_artifact_id
                       AND a.configuration_revision = ?1
                       AND json_extract(a.manifest_json, '$.status') = 'completed'
                   )
                   AND NOT EXISTS (
                     SELECT 1 FROM stage_runs sr
                     WHERE sr.run_id = r.run_id
                       AND sr.stage = 'implementation'
                       AND sr.status IN ('pending', 'running')
                   )
                   AND (
                     NOT EXISTS (
                       SELECT 1 FROM implementation_run_state irs WHERE irs.run_id = r.run_id
                     )
                     OR EXISTS (
                       SELECT 1 FROM implementation_run_state irs
                       WHERE irs.run_id = r.run_id AND irs.decision = 'failed'
                     )
                   )
                 ORDER BY r.updated_at ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![configuration_revision], |row| {
                Ok(A3EligibleApprovedRun {
                    run_id: row.get(0)?,
                    forge_host: row.get(1)?,
                    repository: row.get(2)?,
                    issue_id: row.get(3)?,
                    issue_identifier: row.get(4)?,
                    issue_revision: row.get(5)?,
                    approved_artifact_id: row.get(6)?,
                    approved_version: row.get(7)?,
                })
            })
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    /// Approved A3 draft PRs whose tracker item can be considered by A4.
    ///
    /// The tracker state itself is deliberately checked by the coordinator
    /// against the live Projects v2 item. The store only returns durable A3
    /// publication facts and applies the single-owner/head-cycle guards.
    pub fn list_a4_eligible_review_runs(&self) -> Result<Vec<A4EligibleReviewRun>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT r.run_id, r.forge_host, r.repository, r.issue_id,
                    r.issue_identifier, d.artifact_id, d.implementation_artifact_id,
                    s.approved_artifact_id, d.number, d.url, d.draft, d.head,
                    d.base, d.head_sha
                 FROM factory_runs r
                 JOIN implementation_draft_pr_artifacts d ON d.run_id = r.run_id
                 JOIN implementation_publication_intents p ON p.intent_id = d.intent_id
                    AND p.status = 'applied'
                 JOIN implementation_artifacts ia
                    ON ia.artifact_id = d.implementation_artifact_id
                 JOIN spec_run_state s ON s.run_id = r.run_id
                    AND s.approved_artifact_id IS NOT NULL
                 WHERE s.decision = 'approved'
                   AND NOT EXISTS (
                     SELECT 1 FROM review_findings_artifacts a
                     WHERE a.run_id = r.run_id AND a.reviewed_head_sha = d.head_sha
                   )
                   AND NOT EXISTS (
                     SELECT 1 FROM stage_runs sr
                     WHERE sr.run_id = r.run_id AND sr.stage = ?1
                       AND sr.status IN ('pending', 'running')
                   )
                 ORDER BY r.updated_at ASC, d.created_at ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![REVIEW_STAGE_NAME], |row| {
                let draft: i64 = row.get(10)?;
                Ok(A4EligibleReviewRun {
                    run_id: row.get(0)?,
                    forge_host: row.get(1)?,
                    repository: row.get(2)?,
                    issue_id: row.get(3)?,
                    issue_identifier: row.get(4)?,
                    draft_pr_artifact_id: row.get(5)?,
                    implementation_artifact_id: row.get(6)?,
                    approved_artifact_id: row.get(7)?,
                    pr_number: row.get::<_, i64>(8)? as u64,
                    pr_url: row.get(9)?,
                    draft: draft != 0,
                    head: row.get(11)?,
                    base: row.get(12)?,
                    head_sha: row.get(13)?,
                })
            })
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn store_review_attempt_inputs(
        &mut self,
        request: StoreReviewAttemptRequest,
    ) -> Result<ReviewAttemptRecord> {
        let stage = select_stage_run(&self.conn, &request.stage_run_id)?;
        if stage.stage != REVIEW_STAGE_NAME || stage.status != StageStatus::Running {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is not a running review attempt",
                request.stage_run_id
            )));
        }
        let now_s = ts(Self::now());
        self.conn
            .execute(
                "INSERT INTO review_attempts (
                    attempt_id, run_id, stage_run_id, draft_pr_artifact_id,
                    implementation_artifact_id, spec_artifact_id, pr_number,
                    reviewed_head_sha, base_sha, status, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'running', ?10, ?10)",
                params![
                    request.attempt_id,
                    stage.run_id,
                    request.stage_run_id,
                    request.draft_pr_artifact_id,
                    request.implementation_artifact_id,
                    request.spec_artifact_id,
                    request.pr_number as i64,
                    request.reviewed_head_sha,
                    request.base_sha,
                    now_s,
                ],
            )
            .map_err(storage_error)?;
        self.get_review_attempt(&request.attempt_id)?.ok_or_else(|| {
            SymphonyError::StorageError("created review attempt is missing".to_string())
        })
    }

    pub fn get_review_attempt(&self, attempt_id: &str) -> Result<Option<ReviewAttemptRecord>> {
        self.conn
            .query_row(
                "SELECT attempt_id, run_id, stage_run_id, draft_pr_artifact_id,
                    implementation_artifact_id, spec_artifact_id, pr_number,
                    reviewed_head_sha, base_sha, status, reprompt_count,
                    worker_turn_json, manifest_json, validation_result_json,
                    last_error_json, created_at, updated_at
                 FROM review_attempts WHERE attempt_id = ?1",
                params![attempt_id],
                review_attempt_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_review_attempts(&self, run_id: &str) -> Result<Vec<ReviewAttemptRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT attempt_id, run_id, stage_run_id, draft_pr_artifact_id,
                    implementation_artifact_id, spec_artifact_id, pr_number,
                    reviewed_head_sha, base_sha, status, reprompt_count,
                    worker_turn_json, manifest_json, validation_result_json,
                    last_error_json, created_at, updated_at
                 FROM review_attempts WHERE run_id = ?1 ORDER BY created_at DESC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], review_attempt_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn update_review_attempt(
        &mut self,
        attempt_id: &str,
        status: &str,
        reprompt_count: u32,
        worker_turn: Option<&serde_json::Value>,
        manifest: Option<&serde_json::Value>,
        validation_result: Option<&serde_json::Value>,
        error: Option<&FactoryError>,
    ) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE review_attempts SET status = ?1, reprompt_count = ?2,
                    worker_turn_json = ?3, manifest_json = ?4,
                    validation_result_json = ?5, last_error_json = ?6,
                    updated_at = ?7 WHERE attempt_id = ?8",
                params![
                    status,
                    reprompt_count,
                    worker_turn.map(bounded_json).transpose()?,
                    manifest.map(bounded_json).transpose()?,
                    validation_result.map(bounded_json).transpose()?,
                    error.map(|value| bounded_json(value)).transpose()?,
                    ts(Self::now()),
                    attempt_id,
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "review attempt {attempt_id} not found"
            )));
        }
        Ok(())
    }

    /// Persist a validated manifest and terminate its stage attempt in one
    /// transaction. The immutable artifact row has no update/delete path.
    pub fn store_review_artifact(
        &mut self,
        request: StoreReviewArtifactRequest,
    ) -> Result<ReviewFindingsArtifactRecord> {
        let stage = select_stage_run(&self.conn, &request.stage_run_id)?;
        if stage.stage != REVIEW_STAGE_NAME || stage.status != StageStatus::Running {
            return Err(SymphonyError::StorageError(format!(
                "stage run {} is not a running review attempt",
                request.stage_run_id
            )));
        }
        if request.manifest.reviewed_head_sha != request.reviewed_head_sha
            || request.manifest.base_sha != request.base_sha
        {
            return Err(SymphonyError::StorageError(
                "review manifest does not match the pinned attempt".to_string(),
            ));
        }
        let now_s = ts(Self::now());
        let manifest_json = bounded_json(&request.manifest)?;
        let tx = self.conn.transaction().map_err(storage_error)?;
        let artifact_id = new_id();
        tx.execute(
            "INSERT INTO review_findings_artifacts (
                artifact_id, run_id, stage_run_id, attempt_id,
                draft_pr_artifact_id, implementation_artifact_id, spec_artifact_id,
                schema_version, reviewed_head_sha, base_sha, manifest_json,
                no_findings, finding_count, received_at, bytes_len
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                artifact_id,
                stage.run_id,
                request.stage_run_id,
                request.attempt_id,
                request.draft_pr_artifact_id,
                request.implementation_artifact_id,
                request.spec_artifact_id,
                request.manifest.schema_version,
                request.reviewed_head_sha,
                request.base_sha,
                manifest_json,
                if request.manifest.no_findings { 1 } else { 0 },
                request.manifest.findings.len() as i64,
                now_s,
                request.bytes_len,
            ],
        )
        .map_err(storage_error)?;
        for finding in &request.manifest.findings {
            let severity = match finding.severity {
                crate::review::manifest::ReviewSeverity::Blocking => "blocking",
                crate::review::manifest::ReviewSeverity::Major => "major",
                crate::review::manifest::ReviewSeverity::Minor => "minor",
                crate::review::manifest::ReviewSeverity::Nit => "nit",
            };
            let category = match finding.category {
                crate::review::manifest::ReviewFindingCategory::Correctness => "correctness",
                crate::review::manifest::ReviewFindingCategory::Security => "security",
                crate::review::manifest::ReviewFindingCategory::SpecConformance => {
                    "spec-conformance"
                }
                crate::review::manifest::ReviewFindingCategory::TestCoverage => "test-coverage",
                crate::review::manifest::ReviewFindingCategory::Maintainability => {
                    "maintainability"
                }
            };
            let identity_key = format!(
                "{}:{}:{}:{}",
                finding.path,
                finding.line,
                finding.end_line.unwrap_or(finding.line),
                finding.claim.trim()
            );
            tx.execute(
                "INSERT INTO review_finding_records (
                    finding_record_id, run_id, artifact_id, finding_id, identity_key,
                    reviewed_head_sha, severity, category, path, line, end_line,
                    claim, rationale, remediation, acceptance_criterion, confidence,
                    lifecycle_state, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                    ?13, ?14, ?15, ?16, 'new', ?17, ?17)",
                params![
                    new_id(),
                    stage.run_id,
                    artifact_id,
                    finding.finding_id,
                    identity_key,
                    request.reviewed_head_sha,
                    severity,
                    category,
                    finding.path,
                    finding.line,
                    finding.end_line,
                    finding.claim,
                    finding.rationale,
                    finding.remediation,
                    finding.acceptance_criterion,
                    finding.confidence,
                    now_s,
                ],
            )
            .map_err(storage_error)?;
        }
        let validation_result = bounded_json(&serde_json::json!({
            "accepted": true,
            "finding_count": request.manifest.findings.len()
        }))?;
        tx.execute(
            "UPDATE review_attempts SET status = 'completed', manifest_json = ?1,
                validation_result_json = ?2, updated_at = ?3 WHERE attempt_id = ?4",
            params![
                bounded_json(&request.manifest)?,
                validation_result,
                now_s,
                request.attempt_id,
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE stage_runs SET status = 'completed', completed_at = ?1,
                input_tokens = ?2, output_tokens = ?3, total_tokens = ?4,
                updated_at = ?1 WHERE stage_run_id = ?5",
            params![
                now_s,
                request.usage.input_tokens,
                request.usage.output_tokens,
                request.usage.total_tokens,
                request.stage_run_id,
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE factory_runs SET status = 'waiting', current_stage = ?1,
                updated_at = ?2 WHERE run_id = ?3",
            params![REVIEW_STAGE_NAME, now_s, stage.run_id],
        )
        .map_err(storage_error)?;
        let record = tx
            .query_row(
                "SELECT artifact_id, run_id, stage_run_id, attempt_id,
                    draft_pr_artifact_id, implementation_artifact_id, spec_artifact_id,
                    schema_version, reviewed_head_sha, base_sha, manifest_json,
                    no_findings, finding_count, received_at, bytes_len
                 FROM review_findings_artifacts WHERE artifact_id = ?1",
                params![artifact_id],
                review_artifact_from_row,
            )
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(record)
    }

    pub fn get_review_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<ReviewFindingsArtifactRecord>> {
        self.conn
            .query_row(
                "SELECT artifact_id, run_id, stage_run_id, attempt_id,
                    draft_pr_artifact_id, implementation_artifact_id, spec_artifact_id,
                    schema_version, reviewed_head_sha, base_sha, manifest_json,
                    no_findings, finding_count, received_at, bytes_len
                 FROM review_findings_artifacts WHERE artifact_id = ?1",
                params![artifact_id],
                review_artifact_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_review_artifacts(&self, run_id: &str) -> Result<Vec<ReviewFindingsArtifactRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT artifact_id, run_id, stage_run_id, attempt_id,
                    draft_pr_artifact_id, implementation_artifact_id, spec_artifact_id,
                    schema_version, reviewed_head_sha, base_sha, manifest_json,
                    no_findings, finding_count, received_at, bytes_len
                 FROM review_findings_artifacts WHERE run_id = ?1
                 ORDER BY received_at DESC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], review_artifact_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn create_review_publication_intent(
        &mut self,
        run_id: &str,
        artifact_id: &str,
        kind: &str,
        desired_effects: &serde_json::Value,
    ) -> Result<ReviewPublicationIntent> {
        let intent_id = new_id();
        let now_s = ts(Self::now());
        self.conn
            .execute(
                "INSERT INTO review_publication_intents (
                    intent_id, run_id, artifact_id, kind, status,
                    completed_steps_json, desired_effects_json,
                    observed_baseline_json, expected_projection_json,
                    created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', ?5, '{}', '{}', ?6, ?6)",
                params![
                    intent_id,
                    run_id,
                    artifact_id,
                    kind,
                    bounded_json(desired_effects)?,
                    now_s,
                ],
            )
            .map_err(storage_error)?;
        self.get_review_publication_intent(&intent_id)?.ok_or_else(|| {
            SymphonyError::StorageError("created review publication intent is missing".to_string())
        })
    }

    pub fn get_review_publication_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<ReviewPublicationIntent>> {
        self.conn
            .query_row(
                "SELECT intent_id, run_id, artifact_id, kind, status,
                    completed_steps_json, retry_count, last_error_json, comment_id,
                    publisher_login, desired_effects_json, observed_baseline_json,
                    expected_projection_json, created_at, updated_at
                 FROM review_publication_intents WHERE intent_id = ?1",
                params![intent_id],
                review_publication_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_pending_review_publications(&self) -> Result<Vec<ReviewPublicationIntent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT intent_id, run_id, artifact_id, kind, status,
                    completed_steps_json, retry_count, last_error_json, comment_id,
                    publisher_login, desired_effects_json, observed_baseline_json,
                    expected_projection_json, created_at, updated_at
                 FROM review_publication_intents WHERE status = 'pending'
                 ORDER BY created_at ASC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], review_publication_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn list_review_publications_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<ReviewPublicationIntent>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT intent_id, run_id, artifact_id, kind, status,
                    completed_steps_json, retry_count, last_error_json, comment_id,
                    publisher_login, desired_effects_json, observed_baseline_json,
                    expected_projection_json, created_at, updated_at
                 FROM review_publication_intents WHERE run_id = ?1
                 ORDER BY created_at DESC",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![run_id], review_publication_from_row)
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    pub fn bind_review_publication_comment(
        &mut self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE review_publication_intents SET comment_id = ?1,
                    publisher_login = ?2, updated_at = ?3 WHERE intent_id = ?4",
                params![comment_id, publisher_login, ts(Self::now()), intent_id],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn complete_review_publication(&mut self, intent_id: &str, step: &str) -> Result<()> {
        let intent = self
            .get_review_publication_intent(intent_id)?
            .ok_or_else(|| SymphonyError::StorageError(format!("review intent {intent_id} not found")))?;
        let mut steps = intent.completed_steps;
        if !steps.iter().any(|existing| existing == step) {
            steps.push(step.to_string());
        }
        self.conn
            .execute(
                "UPDATE review_publication_intents SET status = 'applied',
                    completed_steps_json = ?1, last_error_json = NULL, updated_at = ?2
                 WHERE intent_id = ?3",
                params![bounded_json(&steps)?, ts(Self::now()), intent_id],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn set_review_publication_error(
        &mut self,
        intent_id: &str,
        status: PublicationStatus,
        error: FactoryError,
    ) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE review_publication_intents SET status = ?1,
                    last_error_json = ?2, retry_count = retry_count + 1,
                    updated_at = ?3 WHERE intent_id = ?4",
                params![
                    status.as_str(),
                    bounded_json(&error)?,
                    ts(Self::now()),
                    intent_id,
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(SymphonyError::StorageError(format!(
                "review intent {intent_id} not found"
            )));
        }
        Ok(())
    }

    pub fn review_metrics(&self) -> Result<ReviewMetricsAggregate> {
        let mut metrics = ReviewMetricsAggregate::default();
        let (total, completed, failed): (u64, u64, u64) = self
            .conn
            .query_row(
                "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status IN ('failed', 'interrupted') THEN 1 ELSE 0 END), 0)
                 FROM stage_runs WHERE stage = ?1",
                params![REVIEW_STAGE_NAME],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(storage_error)?;
        metrics.total_attempts = total;
        metrics.completed_attempts = completed;
        metrics.failed_attempts = failed;
        metrics.blocked_attempts = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM review_publication_intents WHERE status = 'blocked'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        metrics.preview_publications = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM review_publication_intents
                 WHERE kind = 'preview' AND status = 'applied'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        metrics.findings = self
            .conn
            .query_row("SELECT COALESCE(SUM(finding_count), 0) FROM review_findings_artifacts", [], |row| row.get(0))
            .map_err(storage_error)?;
        metrics.no_findings = self
            .conn
            .query_row("SELECT COUNT(*) FROM review_findings_artifacts WHERE no_findings = 1", [], |row| row.get(0))
            .map_err(storage_error)?;
        Ok(metrics)
    }

    pub fn implementation_metrics(&self) -> Result<ImplementationMetricsAggregate> {
        use crate::triage::domain::{TriageMetricsDuration, TriageMetricsTokenTotals};
        use std::collections::BTreeMap;

        let mut aggregate = TriageMetricsAggregate::default();
        let (total, completed, failed): (u64, u64, u64) = self
            .conn
            .query_row(
                "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status IN ('failed', 'interrupted') THEN 1 ELSE 0 END), 0)
             FROM stage_runs WHERE stage = ?1",
                params![IMPLEMENTATION_STAGE_NAME],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(storage_error)?;
        aggregate.total_attempts = total;
        aggregate.completed_attempts = completed;
        aggregate.failed_attempts = failed;

        let mut durations = Vec::new();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT started_at, completed_at FROM stage_runs
             WHERE stage = ?1 AND started_at IS NOT NULL AND completed_at IS NOT NULL",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map(params![IMPLEMENTATION_STAGE_NAME], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (start, end) = row.map_err(storage_error)?;
            durations.push(
                (parse_ts(&end).map_err(SymphonyError::StorageError)?
                    - parse_ts(&start).map_err(SymphonyError::StorageError)?)
                .num_milliseconds()
                .max(0) as f64,
            );
        }
        durations.sort_by(f64::total_cmp);
        aggregate.duration = if durations.is_empty() {
            TriageMetricsDuration {
                average_ms: None,
                p50_ms: None,
                p95_ms: None,
            }
        } else {
            TriageMetricsDuration {
                average_ms: Some(durations.iter().sum::<f64>() / durations.len() as f64),
                p50_ms: Some(percentile(&durations, 0.50)),
                p95_ms: Some(percentile(&durations, 0.95)),
            }
        };

        let mut tokens = BTreeMap::new();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT harness, model, COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0), COALESCE(SUM(total_tokens), 0)
             FROM implementation_turns GROUP BY harness, model",
            )
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                ))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (harness, model, input, output, total) = row.map_err(storage_error)?;
            aggregate.input_tokens = aggregate.input_tokens.saturating_add(input);
            aggregate.output_tokens = aggregate.output_tokens.saturating_add(output);
            aggregate.total_tokens = aggregate.total_tokens.saturating_add(total);
            tokens.insert(
                format!("{}/{}", harness, model.as_deref().unwrap_or("unknown")),
                TriageMetricsTokenTotals {
                    input_tokens: input,
                    output_tokens: output,
                    total_tokens: total,
                },
            );
        }
        aggregate.tokens_by_harness_model = tokens;

        let eligible_approvals: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM spec_run_state WHERE decision = 'approved'
                 AND approved_artifact_id IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let validation_cycles: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_validation_cycles",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let validation_first_pass: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_validation_cycles
                 WHERE cycle = 1 AND passed = 1",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let validation_repairs: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_turns WHERE kind = 'repair'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let validation_exhausted: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM factory_events
                 WHERE event_type = 'implementation_validation_exhausted'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let spec_gaps: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_run_state
                 WHERE decision = 'awaiting_human'
                   AND json_extract(blocker_json, '$.kind') = 'spec_gap'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let preview_publications: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_publication_intents
                 WHERE kind = 'preview' AND status = 'applied'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let automatic_publications: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_publication_intents
                 WHERE kind = 'automatic' AND status = 'applied'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let publication_conflicts: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_publication_intents WHERE status = 'conflict'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let local_attempts: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_attempt_inputs
                 WHERE execution_profile = 'local'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let docker_attempts: u64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM implementation_attempt_inputs
                 WHERE execution_profile = 'docker'",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;

        Ok(ImplementationMetricsAggregate {
            base: aggregate,
            eligible_approvals,
            validation_cycles,
            validation_first_pass,
            validation_repairs,
            validation_exhausted,
            spec_gaps,
            preview_publications,
            automatic_publications,
            publication_conflicts,
            local_attempts,
            docker_attempts,
        })
    }
}

const SPEC_ARTIFACT_SELECT: &str =
    "SELECT artifact_id, run_id, stage_run_id, issue_revision, configuration_revision,
        version, artifact_json, review_cycles, unresolved_findings_json, received_at, bytes_len
     FROM spec_artifacts WHERE artifact_id = ?1";

const SPEC_PUBLICATION_SELECT: &str =
    "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
        retry_count, last_error_json, comment_id, publisher_login,
        desired_effects_json, observed_baseline_json, expected_projection_json,
        created_at, updated_at
     FROM spec_publication_intents WHERE intent_id = ?1";

const IMPLEMENTATION_ARTIFACT_SELECT: &str =
    "SELECT artifact_id, run_id, stage_run_id, approved_artifact_id, approved_version,
        issue_revision, configuration_revision, manifest_json, base_commit, head_commit,
        approved_spec_path, validation_cycles, execution_profile, received_at, bytes_len
     FROM implementation_artifacts WHERE artifact_id = ?1";

const IMPLEMENTATION_BUNDLE_SELECT: &str =
    "SELECT artifact_id, run_id, stage_run_id, implementation_artifact_id, base_commit,
        head_commit, tree_sha, sha256, bytes_len, changed_file_count, changed_paths_json,
        approved_spec_path, approved_spec_blob_sha, harness, model, execution_profile,
        created_at, verified_at
     FROM implementation_bundle_artifacts WHERE artifact_id = ?1";

const IMPLEMENTATION_PUBLICATION_SELECT: &str =
    "SELECT intent_id, run_id, artifact_id, kind, status, completed_steps_json,
        retry_count, last_error_json, comment_id, publisher_login,
        desired_effects_json, observed_baseline_json, expected_projection_json,
        created_at, updated_at
     FROM implementation_publication_intents WHERE intent_id = ?1";

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = p * (sorted.len() as f64 - 1.0);
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let weight = rank - lower as f64;
        sorted[lower] * (1.0 - weight) + sorted[upper] * weight
    }
}

fn review_attempt_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewAttemptRecord> {
    Ok(ReviewAttemptRecord {
        attempt_id: row.get(0)?,
        run_id: row.get(1)?,
        stage_run_id: row.get(2)?,
        draft_pr_artifact_id: row.get(3)?,
        implementation_artifact_id: row.get(4)?,
        spec_artifact_id: row.get(5)?,
        pr_number: row.get::<_, i64>(6)? as u64,
        reviewed_head_sha: row.get(7)?,
        base_sha: row.get(8)?,
        status: row.get(9)?,
        reprompt_count: row.get(10)?,
        worker_turn: optional_from_json(row.get::<_, Option<String>>(11)?)?,
        manifest: optional_from_json(row.get::<_, Option<String>>(12)?)?,
        validation_result: optional_from_json(row.get::<_, Option<String>>(13)?)?,
        last_error: optional_from_json(row.get::<_, Option<String>>(14)?)?,
        created_at: parse_ts_row(row.get::<_, String>(15)?)?,
        updated_at: parse_ts_row(row.get::<_, String>(16)?)?,
    })
}

fn review_artifact_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ReviewFindingsArtifactRecord> {
    let no_findings: i64 = row.get(11)?;
    Ok(ReviewFindingsArtifactRecord {
        artifact_id: row.get(0)?,
        run_id: row.get(1)?,
        stage_run_id: row.get(2)?,
        attempt_id: row.get(3)?,
        draft_pr_artifact_id: row.get(4)?,
        implementation_artifact_id: row.get(5)?,
        spec_artifact_id: row.get(6)?,
        schema_version: row.get(7)?,
        reviewed_head_sha: row.get(8)?,
        base_sha: row.get(9)?,
        manifest: serde_json::from_str(&row.get::<_, String>(10)?).map_err(row_error)?,
        no_findings: no_findings != 0,
        finding_count: row.get(12)?,
        received_at: parse_ts_row(row.get::<_, String>(13)?)?,
        bytes_len: row.get(14)?,
    })
}

fn review_publication_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ReviewPublicationIntent> {
    let status: String = row.get(4)?;
    Ok(ReviewPublicationIntent {
        intent_id: row.get(0)?,
        run_id: row.get(1)?,
        artifact_id: row.get(2)?,
        kind: row.get(3)?,
        status: parse_publication_status(&status).map_err(row_error)?,
        completed_steps: serde_json::from_str(&row.get::<_, String>(5)?).map_err(row_error)?,
        retry_count: row.get(6)?,
        last_error: optional_from_json(row.get::<_, Option<String>>(7)?)?,
        comment_id: row.get(8)?,
        publisher_login: row.get(9)?,
        desired_effects: serde_json::from_str(&row.get::<_, String>(10)?).map_err(row_error)?,
        observed_baseline: serde_json::from_str(&row.get::<_, String>(11)?).map_err(row_error)?,
        expected_projection: serde_json::from_str(&row.get::<_, String>(12)?).map_err(row_error)?,
        created_at: parse_ts_row(row.get::<_, String>(13)?)?,
        updated_at: parse_ts_row(row.get::<_, String>(14)?)?,
    })
}

fn select_stage_run(conn: &Connection, stage_run_id: &str) -> Result<StageRunRecord> {
    conn.query_row(
        "SELECT stage_run_id, run_id, stage, issue_revision, configuration_revision, attempt,
            owner_instance, pid, process_group_id, process_start_token, executable_identity,
            lease_heartbeat_at, status, harness, model, workspace_path, output_path, started_at,
            completed_at, input_tokens, output_tokens, total_tokens, error_json
         FROM stage_runs WHERE stage_run_id = ?1",
        params![stage_run_id],
        stage_run_from_row,
    )
    .map_err(storage_error)
}

fn select_spec_artifact(conn: &Connection, artifact_id: &str) -> Result<SpecArtifactRecord> {
    conn.query_row(
        SPEC_ARTIFACT_SELECT,
        params![artifact_id],
        spec_artifact_from_row,
    )
    .map_err(storage_error)
}

fn select_implementation_artifact(
    conn: &Connection,
    artifact_id: &str,
) -> Result<ImplementationArtifactRecord> {
    conn.query_row(
        IMPLEMENTATION_ARTIFACT_SELECT,
        params![artifact_id],
        implementation_artifact_from_row,
    )
    .map_err(storage_error)
}

fn implementation_turn_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ImplementationTurnRecord> {
    let kind: String = row.get(3)?;
    let status: String = row.get(4)?;
    let output: Option<String> = row.get(11)?;
    Ok(ImplementationTurnRecord {
        turn_id: row.get(0)?,
        stage_run_id: row.get(1)?,
        ordinal: row.get(2)?,
        kind: parse_implementation_turn_kind(&kind).map_err(row_error)?,
        status: parse_implementation_turn_status(&status).map_err(row_error)?,
        harness: row.get(5)?,
        model: row.get(6)?,
        execution_profile: row.get(7)?,
        usage: StageUsage {
            input_tokens: row.get(8)?,
            output_tokens: row.get(9)?,
            total_tokens: row.get(10)?,
        },
        output_json: output
            .map(|json| serde_json::from_str(&json).map_err(row_error))
            .transpose()?,
        error: optional_from_json(row.get::<_, Option<String>>(12)?)?,
        started_at: parse_ts_row(row.get::<_, String>(13)?)?,
        completed_at: parse_optional_ts_row(row.get::<_, Option<String>>(14)?)?,
    })
}

fn validation_cycle_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ValidationCycleRecord> {
    let passed: i64 = row.get(3)?;
    Ok(ValidationCycleRecord {
        cycle_id: row.get(0)?,
        stage_run_id: row.get(1)?,
        cycle: row.get(2)?,
        passed: passed != 0,
        commands: serde_json::from_str(&row.get::<_, String>(4)?).map_err(row_error)?,
        started_at: parse_ts_row(row.get::<_, String>(5)?)?,
        completed_at: parse_ts_row(row.get::<_, String>(6)?)?,
    })
}

fn implementation_artifact_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ImplementationArtifactRecord> {
    let profile: String = row.get(12)?;
    Ok(ImplementationArtifactRecord {
        artifact_id: row.get(0)?,
        run_id: row.get(1)?,
        stage_run_id: row.get(2)?,
        approved_artifact_id: row.get(3)?,
        approved_version: row.get(4)?,
        issue_revision: row.get(5)?,
        configuration_revision: row.get(6)?,
        manifest: serde_json::from_str(&row.get::<_, String>(7)?).map_err(row_error)?,
        base_commit: row.get(8)?,
        head_commit: row.get(9)?,
        approved_spec_path: row.get(10)?,
        validation_cycles: row.get(11)?,
        execution_profile: parse_execution_profile(&profile).map_err(row_error)?,
        received_at: parse_ts_row(row.get::<_, String>(13)?)?,
        bytes_len: row.get(14)?,
    })
}

fn bundle_artifact_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BundleArtifactRecord> {
    let profile: String = row.get(15)?;
    Ok(BundleArtifactRecord {
        artifact_id: row.get(0)?,
        run_id: row.get(1)?,
        stage_run_id: row.get(2)?,
        implementation_artifact_id: row.get(3)?,
        base_commit: row.get(4)?,
        head_commit: row.get(5)?,
        tree_sha: row.get(6)?,
        sha256: row.get(7)?,
        bytes_len: row.get(8)?,
        changed_file_count: row.get(9)?,
        changed_paths: serde_json::from_str(&row.get::<_, String>(10)?).map_err(row_error)?,
        approved_spec_path: row.get(11)?,
        approved_spec_blob_sha: row.get(12)?,
        harness: row.get(13)?,
        model: row.get(14)?,
        execution_profile: parse_execution_profile(&profile).map_err(row_error)?,
        created_at: parse_ts_row(row.get::<_, String>(16)?)?,
        verified_at: parse_ts_row(row.get::<_, String>(17)?)?,
    })
}

fn draft_pr_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftPrArtifactRecord> {
    let draft_i: i64 = row.get(6)?;
    Ok(DraftPrArtifactRecord {
        artifact_id: row.get(0)?,
        run_id: row.get(1)?,
        implementation_artifact_id: row.get(2)?,
        intent_id: row.get(3)?,
        number: row.get::<_, i64>(4)? as u64,
        url: row.get(5)?,
        draft: draft_i != 0,
        head: row.get(7)?,
        base: row.get(8)?,
        head_sha: row.get(9)?,
        marker: row.get(10)?,
        created_at: parse_ts_row(row.get::<_, String>(11)?)?,
    })
}

fn implementation_state_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ImplementationRunState> {
    let decision: String = row.get(4)?;
    Ok(ImplementationRunState {
        run_id: row.get(0)?,
        approved_artifact_id: row.get(1)?,
        approved_version: row.get(2)?,
        configuration_revision: row.get(3)?,
        decision: parse_implementation_decision(&decision).map_err(row_error)?,
        successful_artifact_id: row.get(5)?,
        bundle_artifact_id: row.get(6)?,
        blocker: optional_from_json(row.get::<_, Option<String>>(7)?)?,
        updated_at: parse_ts_row(row.get::<_, String>(8)?)?,
    })
}

fn implementation_publication_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ImplementationPublicationIntent> {
    let kind: String = row.get(3)?;
    let status: String = row.get(4)?;
    Ok(ImplementationPublicationIntent {
        intent_id: row.get(0)?,
        run_id: row.get(1)?,
        artifact_id: row.get(2)?,
        kind: parse_implementation_publication_kind(&kind).map_err(row_error)?,
        status: parse_publication_status(&status).map_err(row_error)?,
        completed_steps: serde_json::from_str(&row.get::<_, String>(5)?).map_err(row_error)?,
        retry_count: row.get(6)?,
        last_error: optional_from_json(row.get::<_, Option<String>>(7)?)?,
        comment_id: row.get(8)?,
        publisher_login: row.get(9)?,
        desired_effects: serde_json::from_str(&row.get::<_, String>(10)?).map_err(row_error)?,
        observed_baseline: serde_json::from_str(&row.get::<_, String>(11)?).map_err(row_error)?,
        expected_projection: serde_json::from_str(&row.get::<_, String>(12)?).map_err(row_error)?,
        created_at: parse_ts_row(row.get::<_, String>(13)?)?,
        updated_at: parse_ts_row(row.get::<_, String>(14)?)?,
    })
}

fn spec_turn_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpecTurnRecord> {
    let kind: String = row.get(3)?;
    let status: String = row.get(4)?;
    let output: Option<String> = row.get(10)?;
    Ok(SpecTurnRecord {
        turn_id: row.get(0)?,
        stage_run_id: row.get(1)?,
        ordinal: row.get(2)?,
        kind: parse_spec_turn_kind(&kind).map_err(row_error)?,
        status: parse_spec_turn_status(&status).map_err(row_error)?,
        harness: row.get(5)?,
        model: row.get(6)?,
        usage: StageUsage {
            input_tokens: row.get(7)?,
            output_tokens: row.get(8)?,
            total_tokens: row.get(9)?,
        },
        output_json: output
            .map(|json| serde_json::from_str(&json).map_err(row_error))
            .transpose()?,
        error: optional_from_json(row.get::<_, Option<String>>(11)?)?,
        started_at: parse_ts_row(row.get::<_, String>(12)?)?,
        completed_at: parse_optional_ts_row(row.get::<_, Option<String>>(13)?)?,
    })
}

fn spec_artifact_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpecArtifactRecord> {
    Ok(SpecArtifactRecord {
        artifact_id: row.get(0)?,
        run_id: row.get(1)?,
        stage_run_id: row.get(2)?,
        issue_revision: row.get(3)?,
        configuration_revision: row.get(4)?,
        version: row.get(5)?,
        artifact: serde_json::from_str(&row.get::<_, String>(6)?).map_err(row_error)?,
        review_cycles: row.get(7)?,
        unresolved_blocking_findings: serde_json::from_str(&row.get::<_, String>(8)?)
            .map_err(row_error)?,
        received_at: parse_ts_row(row.get::<_, String>(9)?)?,
        bytes_len: row.get(10)?,
    })
}

fn spec_state_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpecRunState> {
    let decision: String = row.get(5)?;
    Ok(SpecRunState {
        run_id: row.get(0)?,
        revision_requests_used: row.get(1)?,
        pending_approval_version: row.get(2)?,
        approved_version: row.get(3)?,
        approved_artifact_id: row.get(4)?,
        decision: parse_spec_decision(&decision).map_err(row_error)?,
        updated_at: parse_ts_row(row.get::<_, String>(6)?)?,
    })
}

fn spec_publication_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpecPublicationIntent> {
    let kind: String = row.get(3)?;
    let status: String = row.get(4)?;
    Ok(SpecPublicationIntent {
        intent_id: row.get(0)?,
        run_id: row.get(1)?,
        artifact_id: row.get(2)?,
        kind: parse_spec_publication_kind(&kind).map_err(row_error)?,
        status: parse_publication_status(&status).map_err(row_error)?,
        completed_steps: serde_json::from_str(&row.get::<_, String>(5)?).map_err(row_error)?,
        retry_count: row.get(6)?,
        last_error: optional_from_json(row.get::<_, Option<String>>(7)?)?,
        comment_id: row.get(8)?,
        publisher_login: row.get(9)?,
        desired_effects: serde_json::from_str(&row.get::<_, String>(10)?).map_err(row_error)?,
        observed_baseline: serde_json::from_str(&row.get::<_, String>(11)?).map_err(row_error)?,
        expected_projection: serde_json::from_str(&row.get::<_, String>(12)?).map_err(row_error)?,
        created_at: parse_ts_row(row.get::<_, String>(13)?)?,
        updated_at: parse_ts_row(row.get::<_, String>(14)?)?,
    })
}

fn select_artifact(conn: &Connection, artifact_id: &str) -> Result<ArtifactRecord> {
    conn.query_row(
        "SELECT artifact_id, run_id, stage_run_id, issue_revision, configuration_revision,
            route_mapping_hash, artifact_json, received_at, bytes_len
         FROM triage_artifacts WHERE artifact_id = ?1",
        params![artifact_id],
        artifact_from_row,
    )
    .map_err(storage_error)
}

fn select_publication_intent(
    conn: &Connection,
    intent_id: &str,
) -> Result<PublicationIntentRecord> {
    conn.query_row(
        "SELECT intent_id, run_id, artifact_id, mode, status, intake_label, route_label,
            project_state, route_mapping_hash, completed_steps_json, retry_count, last_error_json,
            comment_id, publisher_login, desired_effects_json, observed_baseline_json,
            expected_projection_json, created_at, updated_at
         FROM publication_intents WHERE intent_id = ?1",
        params![intent_id],
        publication_intent_from_row,
    )
    .map_err(storage_error)
}

fn factory_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FactoryRunRecord> {
    let status: String = row.get(6)?;
    Ok(FactoryRunRecord {
        run_id: row.get(0)?,
        forge_host: row.get(1)?,
        repository: row.get(2)?,
        issue_id: row.get(3)?,
        issue_identifier: row.get(4)?,
        issue_revision: row.get(5)?,
        status: parse_factory_status(&status).map_err(row_error)?,
        current_stage: row.get(7)?,
        created_at: parse_ts_row(row.get::<_, String>(8)?)?,
        updated_at: parse_ts_row(row.get::<_, String>(9)?)?,
    })
}

fn stage_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StageRunRecord> {
    let status: String = row.get(12)?;
    let error_json: Option<String> = row.get(22)?;
    Ok(StageRunRecord {
        stage_run_id: row.get(0)?,
        run_id: row.get(1)?,
        stage: row.get(2)?,
        issue_revision: row.get(3)?,
        configuration_revision: row.get(4)?,
        attempt: row.get(5)?,
        owner_instance: row.get(6)?,
        pid: row.get(7)?,
        process_group_id: row.get(8)?,
        process_start_token: row.get(9)?,
        executable_identity: row.get(10)?,
        lease_heartbeat_at: parse_optional_ts_row(row.get::<_, Option<String>>(11)?)?,
        status: parse_stage_status(&status).map_err(row_error)?,
        harness: row.get(13)?,
        model: row.get(14)?,
        workspace_path: row.get(15)?,
        output_path: row.get(16)?,
        started_at: parse_optional_ts_row(row.get::<_, Option<String>>(17)?)?,
        completed_at: parse_optional_ts_row(row.get::<_, Option<String>>(18)?)?,
        usage: StageUsage {
            input_tokens: row.get(19)?,
            output_tokens: row.get(20)?,
            total_tokens: row.get(21)?,
        },
        error: optional_from_json(error_json)?,
    })
}

fn artifact_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtifactRecord> {
    let artifact_json: String = row.get(6)?;
    Ok(ArtifactRecord {
        artifact_id: row.get(0)?,
        run_id: row.get(1)?,
        stage_run_id: row.get(2)?,
        issue_revision: row.get(3)?,
        configuration_revision: row.get(4)?,
        route_mapping_hash: row.get(5)?,
        artifact: serde_json::from_str(&artifact_json).map_err(row_error)?,
        received_at: parse_ts_row(row.get::<_, String>(7)?)?,
        bytes_len: row.get(8)?,
    })
}

fn publication_intent_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PublicationIntentRecord> {
    let mode: String = row.get(3)?;
    let status: String = row.get(4)?;
    Ok(PublicationIntentRecord {
        intent_id: row.get(0)?,
        run_id: row.get(1)?,
        artifact_id: row.get(2)?,
        mode: parse_publication_mode(&mode).map_err(row_error)?,
        status: parse_publication_status(&status).map_err(row_error)?,
        intake_label: row.get(5)?,
        route_label: row.get(6)?,
        project_state: row.get(7)?,
        route_mapping_hash: row.get(8)?,
        completed_steps: serde_json::from_str(&row.get::<_, String>(9)?).map_err(row_error)?,
        retry_count: row.get(10)?,
        last_error: optional_from_json(row.get::<_, Option<String>>(11)?)?,
        comment_id: row.get(12)?,
        publisher_login: row.get(13)?,
        desired_effects: serde_json::from_str(&row.get::<_, String>(14)?).map_err(row_error)?,
        observed_baseline: serde_json::from_str(&row.get::<_, String>(15)?).map_err(row_error)?,
        expected_projection: serde_json::from_str(&row.get::<_, String>(16)?).map_err(row_error)?,
        created_at: parse_ts_row(row.get::<_, String>(17)?)?,
        updated_at: parse_ts_row(row.get::<_, String>(18)?)?,
    })
}

fn new_id() -> String {
    Uuid::now_v7().to_string()
}

fn ts(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

fn parse_ts(value: &str) -> std::result::Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|err| err.to_string())
}

fn parse_ts_row(value: String) -> rusqlite::Result<DateTime<Utc>> {
    parse_ts(&value).map_err(row_error)
}

fn parse_optional_ts_row(value: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    value.map(parse_ts_row).transpose()
}

fn bounded_json<T: serde::Serialize>(value: &T) -> Result<String> {
    let json = serde_json::to_string(value).map_err(storage_error)?;
    if json.len() > FACTORY_EVENT_PAYLOAD_MAX_BYTES {
        return Err(SymphonyError::StorageError(format!(
            "JSON payload exceeds {} bytes",
            FACTORY_EVENT_PAYLOAD_MAX_BYTES
        )));
    }
    Ok(json)
}

fn optional_json<T: serde::Serialize>(value: &Option<T>) -> Result<Option<String>> {
    value
        .as_ref()
        .map(|value| {
            let json = serde_json::to_string(value).map_err(storage_error)?;
            Ok(truncate_utf8_bytes(
                &json,
                FACTORY_ERROR_STRING_MAX_BYTES * 4,
            ))
        })
        .transpose()
}

fn optional_from_json<T: serde::de::DeserializeOwned>(
    value: Option<String>,
) -> rusqlite::Result<Option<T>> {
    value
        .map(|json| serde_json::from_str(&json).map_err(row_error))
        .transpose()
}

fn parse_factory_status(value: &str) -> std::result::Result<FactoryRunStatus, String> {
    match value {
        "active" => Ok(FactoryRunStatus::Active),
        "waiting" => Ok(FactoryRunStatus::Waiting),
        "completed" => Ok(FactoryRunStatus::Completed),
        "failed" => Ok(FactoryRunStatus::Failed),
        "ineligible" => Ok(FactoryRunStatus::Ineligible),
        _ => Err(format!("unknown factory run status {value}")),
    }
}

fn parse_stage_status(value: &str) -> std::result::Result<StageStatus, String> {
    match value {
        "pending" => Ok(StageStatus::Pending),
        "running" => Ok(StageStatus::Running),
        "completed" => Ok(StageStatus::Completed),
        "failed" => Ok(StageStatus::Failed),
        "interrupted" => Ok(StageStatus::Interrupted),
        _ => Err(format!("unknown stage status {value}")),
    }
}

fn parse_spec_turn_kind(value: &str) -> std::result::Result<SpecTurnKind, String> {
    match value {
        "draft" => Ok(SpecTurnKind::Draft),
        "review" => Ok(SpecTurnKind::Review),
        "revise" => Ok(SpecTurnKind::Revise),
        _ => Err(format!("unknown spec turn kind {value}")),
    }
}

fn parse_spec_turn_status(value: &str) -> std::result::Result<SpecTurnStatus, String> {
    match value {
        "running" => Ok(SpecTurnStatus::Running),
        "completed" => Ok(SpecTurnStatus::Completed),
        "failed" => Ok(SpecTurnStatus::Failed),
        _ => Err(format!("unknown spec turn status {value}")),
    }
}

fn parse_spec_publication_kind(value: &str) -> std::result::Result<SpecPublicationKind, String> {
    match value {
        "preview" => Ok(SpecPublicationKind::Preview),
        "approval" => Ok(SpecPublicationKind::Approval),
        "diagnostic" => Ok(SpecPublicationKind::Diagnostic),
        _ => Err(format!("unknown spec publication kind {value}")),
    }
}

fn parse_spec_decision(value: &str) -> std::result::Result<SpecDecision, String> {
    match value {
        "awaiting_decision" => Ok(SpecDecision::AwaitingDecision),
        "revision_requested" => Ok(SpecDecision::RevisionRequested),
        "pending_approval" => Ok(SpecDecision::PendingApproval),
        "approved" => Ok(SpecDecision::Approved),
        "blocked" => Ok(SpecDecision::Blocked),
        "conflict" => Ok(SpecDecision::Conflict),
        _ => Err(format!("unknown spec decision {value}")),
    }
}

fn parse_implementation_turn_kind(
    value: &str,
) -> std::result::Result<ImplementationTurnKind, String> {
    match value {
        "implement" => Ok(ImplementationTurnKind::Implement),
        "repair" => Ok(ImplementationTurnKind::Repair),
        _ => Err(format!("unknown implementation turn kind {value}")),
    }
}

fn parse_implementation_turn_status(
    value: &str,
) -> std::result::Result<ImplementationTurnStatus, String> {
    match value {
        "running" => Ok(ImplementationTurnStatus::Running),
        "completed" => Ok(ImplementationTurnStatus::Completed),
        "failed" => Ok(ImplementationTurnStatus::Failed),
        _ => Err(format!("unknown implementation turn status {value}")),
    }
}

fn parse_execution_profile(value: &str) -> std::result::Result<ExecutionProfile, String> {
    match value {
        "local" => Ok(ExecutionProfile::Local),
        "docker" => Ok(ExecutionProfile::Docker),
        _ => Err(format!("unknown execution profile {value}")),
    }
}

fn parse_implementation_decision(
    value: &str,
) -> std::result::Result<ImplementationDecision, String> {
    match value {
        "pending" => Ok(ImplementationDecision::Pending),
        "previewed" => Ok(ImplementationDecision::Previewed),
        "awaiting_human" => Ok(ImplementationDecision::AwaitingHuman),
        "published" => Ok(ImplementationDecision::Published),
        "blocked" => Ok(ImplementationDecision::Blocked),
        "failed" => Ok(ImplementationDecision::Failed),
        "conflict" => Ok(ImplementationDecision::Conflict),
        _ => Err(format!("unknown implementation decision {value}")),
    }
}

fn parse_implementation_publication_kind(
    value: &str,
) -> std::result::Result<ImplementationPublicationKind, String> {
    match value {
        "preview" => Ok(ImplementationPublicationKind::Preview),
        "automatic" => Ok(ImplementationPublicationKind::Automatic),
        "diagnostic" => Ok(ImplementationPublicationKind::Diagnostic),
        _ => Err(format!("unknown implementation publication kind {value}")),
    }
}

fn parse_publication_status(value: &str) -> std::result::Result<PublicationStatus, String> {
    match value {
        "none" => Ok(PublicationStatus::None),
        "pending" => Ok(PublicationStatus::Pending),
        "applied" => Ok(PublicationStatus::Applied),
        "blocked" => Ok(PublicationStatus::Blocked),
        "conflict" => Ok(PublicationStatus::Conflict),
        _ => Err(format!("unknown publication status {value}")),
    }
}

fn parse_publication_mode(value: &str) -> std::result::Result<PublicationMode, String> {
    match value {
        "preview" => Ok(PublicationMode::Preview),
        "automatic" => Ok(PublicationMode::Automatic),
        _ => Err(format!("unknown publication mode {value}")),
    }
}

fn storage_error<E: std::fmt::Display>(err: E) -> SymphonyError {
    SymphonyError::StorageError(err.to_string())
}

fn row_error<E: ToString>(err: E) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            err.to_string(),
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triage::domain::{
        EvidenceKind, RiskClass, TriageEvidence, TriageRoute, TRIAGE_SCHEMA_VERSION,
    };

    fn artifact(route: TriageRoute) -> TriageArtifact {
        TriageArtifact {
            schema_version: TRIAGE_SCHEMA_VERSION,
            route,
            risk_class: RiskClass::Low,
            rationale: "Bounded issue.".to_string(),
            evidence: vec![TriageEvidence {
                kind: EvidenceKind::Issue,
                reference: "body".to_string(),
                summary: "Clear request.".to_string(),
            }],
            next_action: "Implement.".to_string(),
            clarification_question: None,
            reproduction: None,
        }
    }

    fn claim_request(issue_revision: &str, configuration_revision: &str) -> ClaimAttemptRequest {
        ClaimAttemptRequest {
            forge_host: "github.com".to_string(),
            repository: "owner/repo".to_string(),
            issue_id: "123".to_string(),
            issue_identifier: "#123".to_string(),
            issue_revision: issue_revision.to_string(),
            configuration_revision: configuration_revision.to_string(),
            owner_instance: "owner-1".to_string(),
            harness: "pi".to_string(),
            model: Some("model-a".to_string()),
            workspace_path: Some("/tmp/work".to_string()),
            output_path: Some("/tmp/out".to_string()),
            pid: Some(1),
            process_group_id: Some(1),
            process_start_token: Some("token".to_string()),
            executable_identity: Some("pi".to_string()),
        }
    }

    fn store(path: &Path) -> SqliteFactoryStore {
        SqliteFactoryStore::acquire_lock_and_migrate(path, 5_000).unwrap()
    }

    #[test]
    fn migrates_and_prevents_second_lock() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let _first = store(&path);
        let second = SqliteFactoryStore::acquire_lock_and_migrate(&path, 5_000);

        assert!(second.is_err());
    }

    #[test]
    fn claim_attempt_enforces_one_nonterminal_revision() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let first = store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .unwrap();

        assert_eq!(first.attempt, 1);
        assert!(store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .is_err());
    }

    /// A spec attempt that dies before completing must be terminated, otherwise the
    /// stage-scoped nonterminal index permanently blocks retries for that revision
    /// pair and the issue can never be specified. This encodes the retry budget in
    /// `spec.max_attempts` being reachable at all.
    #[test]
    fn failed_spec_attempt_frees_the_revision_pair_for_retry() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let first = store
            .claim_stage_attempt(SPEC_STAGE_NAME, claim_request("issue-rev", "config-rev"))
            .unwrap();

        // While the first attempt is nonterminal the same pair is locked out.
        assert!(store
            .claim_stage_attempt(SPEC_STAGE_NAME, claim_request("issue-rev", "config-rev"))
            .is_err());

        store
            .fail_attempt(
                &first.stage_run_id,
                FactoryError::new(
                    "spec_attempt_failed",
                    "spec_coordinator",
                    "runner could not start".to_string(),
                    true,
                    None,
                ),
            )
            .unwrap();

        let retry = store
            .claim_stage_attempt(SPEC_STAGE_NAME, claim_request("issue-rev", "config-rev"))
            .expect("a terminal failure must release the revision pair for a retry");
        assert_eq!(retry.attempt, 2);
        assert_ne!(retry.stage_run_id, first.stage_run_id);
    }

    /// An off-project issue keeps its intake label, so it is re-read on every poll.
    /// The coordinator must find and reuse the existing diagnostic intent; without
    /// this lookup it creates one intent per poll forever and re-emits
    /// `spec_ineligible` each time, breaking the "one idempotent diagnostic" contract.
    #[test]
    fn diagnostic_spec_intent_is_found_for_reuse_across_polls() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let run = store
            .upsert_factory_run(UpsertFactoryRunRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: "123".to_string(),
                issue_identifier: "#123".to_string(),
                issue_revision: None,
                status: FactoryRunStatus::Ineligible,
                current_stage: Some(SPEC_STAGE_NAME.to_string()),
            })
            .unwrap();

        assert!(store
            .find_spec_publication_by_kind(&run.run_id, SpecPublicationKind::Diagnostic)
            .unwrap()
            .is_none());

        let created = store
            .create_spec_publication_intent(
                &run.run_id,
                None,
                SpecPublicationKind::Diagnostic,
                &serde_json::json!({"issue_number": 123, "kind": "off_project"}),
            )
            .unwrap();

        let found = store
            .find_spec_publication_by_kind(&run.run_id, SpecPublicationKind::Diagnostic)
            .unwrap()
            .expect("the existing diagnostic intent must be reused, not duplicated");
        assert_eq!(found.intent_id, created.intent_id);

        // A preview intent on the same run must not be mistaken for the diagnostic.
        store
            .create_spec_publication_intent(
                &run.run_id,
                None,
                SpecPublicationKind::Preview,
                &serde_json::json!({"issue_number": 123}),
            )
            .unwrap();
        let found_after_preview = store
            .find_spec_publication_by_kind(&run.run_id, SpecPublicationKind::Diagnostic)
            .unwrap()
            .expect("diagnostic lookup must ignore a sibling preview intent");
        assert_eq!(found_after_preview.intent_id, created.intent_id);
    }

    /// The `spec-revise` feedback cutoff is the last time the spec was published.
    /// Diagnostics update the same owned comment afterwards, so the cutoff lookup
    /// must be kind-scoped. If it returned the newest intent of any kind, each
    /// diagnostic republish would push the cutoff past the human's feedback comment
    /// and the revision request would never be detected.
    #[test]
    fn feedback_cutoff_reads_the_spec_publication_not_a_later_diagnostic() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let run = store
            .upsert_factory_run(UpsertFactoryRunRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: "123".to_string(),
                issue_identifier: "#123".to_string(),
                issue_revision: None,
                status: FactoryRunStatus::Active,
                current_stage: Some(SPEC_STAGE_NAME.to_string()),
            })
            .unwrap();

        let preview = store
            .create_spec_publication_intent(
                &run.run_id,
                None,
                SpecPublicationKind::Preview,
                &serde_json::json!({"issue_number": 123}),
            )
            .unwrap();
        let diagnostic = store
            .create_spec_publication_intent(
                &run.run_id,
                None,
                SpecPublicationKind::Diagnostic,
                &serde_json::json!({"issue_number": 123, "message": "add feedback"}),
            )
            .unwrap();

        // The diagnostic is the newest intent overall.
        assert_eq!(
            store
                .get_latest_spec_publication(&run.run_id)
                .unwrap()
                .unwrap()
                .intent_id,
            diagnostic.intent_id
        );

        // The cutoff must still resolve to the spec publication.
        assert_eq!(
            store
                .latest_spec_publication_of_kind(&run.run_id, SpecPublicationKind::Preview)
                .unwrap()
                .expect("the spec publication is the feedback cutoff")
                .intent_id,
            preview.intent_id
        );
    }

    /// Criterion 11 requires review-cycle, convergence, revision-request, and
    /// approval-latency aggregates from durable records. Without them an operator
    /// cannot measure review-loop quality or time awaiting human input, which are
    /// the PRD's A2 measures.
    #[test]
    fn spec_metrics_report_review_loop_and_decision_aggregates() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);

        let first_stage = store
            .claim_stage_attempt(SPEC_STAGE_NAME, claim_request("issue-rev", "config-rev"))
            .unwrap();
        store
            .store_spec_turn(StoreSpecTurnRequest {
                turn_id: "turn-draft".to_string(),
                stage_run_id: first_stage.stage_run_id.clone(),
                ordinal: 1,
                kind: SpecTurnKind::Draft,
                status: SpecTurnStatus::Completed,
                harness: "pi".to_string(),
                model: Some("model-a".to_string()),
                usage: StageUsage {
                    input_tokens: 100,
                    output_tokens: 10,
                    total_tokens: 110,
                },
                output_json: None,
                error: None,
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
            })
            .unwrap();
        let artifact_one = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: first_stage.stage_run_id.clone(),
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "config-rev".to_string(),
                artifact: SpecArtifact {
                    schema_version: 1,
                    product_behavior: "behavior".to_string(),
                    technical_approach: "approach".to_string(),
                    acceptance_criteria: vec!["observable".to_string()],
                    open_decisions: vec![],
                },
                review_cycles: 1,
                unresolved_blocking_findings: vec![],
                bytes_len: 100,
                usage: StageUsage {
                    input_tokens: 100,
                    output_tokens: 10,
                    total_tokens: 110,
                },
            })
            .unwrap();
        let run_id = first_stage.run_id.clone();

        let second_stage = store
            .claim_stage_attempt(SPEC_STAGE_NAME, claim_request("issue-rev-2", "config-rev"))
            .unwrap();
        let finding = crate::spec::domain::ReviewFinding {
            severity: crate::spec::domain::FindingSeverity::Blocking,
            section: crate::spec::domain::FindingSection::General,
            summary: "not observable".to_string(),
            recommendation: "state the exact signal".to_string(),
        };
        let artifact_two = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: second_stage.stage_run_id.clone(),
                issue_revision: "issue-rev-2".to_string(),
                configuration_revision: "config-rev".to_string(),
                artifact: artifact_one.artifact.clone(),
                review_cycles: 3,
                unresolved_blocking_findings: vec![finding],
                bytes_len: 100,
                usage: StageUsage {
                    input_tokens: 200,
                    output_tokens: 20,
                    total_tokens: 220,
                },
            })
            .unwrap();

        store.increment_spec_revision_requests(&run_id, 3).unwrap();
        let preview = store
            .create_spec_publication_intent(
                &run_id,
                Some(&artifact_two.artifact_id),
                SpecPublicationKind::Preview,
                &serde_json::json!({"issue_number": 1}),
            )
            .unwrap();
        store
            .create_spec_publication_intent(
                &run_id,
                Some(&artifact_two.artifact_id),
                SpecPublicationKind::Approval,
                &serde_json::json!({"issue_number": 1}),
            )
            .unwrap();

        let metrics = store.spec_metrics().unwrap();

        assert_eq!(metrics.base.completed_attempts, 2);
        assert_eq!(metrics.review_cycles.average, Some(2.0));
        assert_eq!(metrics.review_cycles.max, Some(3));
        assert_eq!(metrics.converged_attempts, 1);
        assert!((metrics.convergence_rate - 0.5).abs() < f64::EPSILON);
        assert_eq!(metrics.revision_requests, 1);
        let average_ms = metrics
            .approval_latency
            .average_ms
            .expect("an approval intent joined to its preview must produce a latency");
        // Preview and approval are created back-to-back in this test, so latency is
        // near zero but must come from the preview→approval pair only (not a
        // self-join of the approval row).
        assert!(average_ms >= 0.0);
        assert!(average_ms < 60_000.0);
        let tokens = metrics
            .base
            .tokens_by_harness_model
            .get("pi/model-a")
            .expect("turn tokens are grouped by harness and model");
        assert_eq!(tokens.input_tokens, 100);
        assert_eq!(tokens.total_tokens, 110);
        let _ = preview;
        let _ = artifact_one;
    }

    #[test]
    fn spec_attempts_are_stage_scoped_and_publish_monotonic_versions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let _triage = store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .unwrap();
        let spec = store
            .claim_stage_attempt(SPEC_STAGE_NAME, claim_request("issue-rev", "config-rev"))
            .unwrap();
        assert_eq!(spec.stage, SPEC_STAGE_NAME);

        store
            .store_spec_turn(StoreSpecTurnRequest {
                turn_id: "turn-1".to_string(),
                stage_run_id: spec.stage_run_id.clone(),
                ordinal: 1,
                kind: SpecTurnKind::Draft,
                status: SpecTurnStatus::Completed,
                harness: "pi".to_string(),
                model: None,
                usage: StageUsage::default(),
                output_json: Some(serde_json::json!({"schema_version":1})),
                error: None,
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
            })
            .unwrap();
        let first = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: spec.stage_run_id,
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "config-rev".to_string(),
                artifact: SpecArtifact {
                    schema_version: 1,
                    product_behavior: "behavior".to_string(),
                    technical_approach: "approach".to_string(),
                    acceptance_criteria: vec!["observable".to_string()],
                    open_decisions: vec![],
                },
                review_cycles: 1,
                unresolved_blocking_findings: vec![],
                bytes_len: 100,
                usage: StageUsage::default(),
            })
            .unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(store.list_spec_turns(&first.stage_run_id).unwrap().len(), 1);

        let second_stage = store
            .claim_stage_attempt(SPEC_STAGE_NAME, claim_request("issue-rev-2", "config-rev"))
            .unwrap();
        let second = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: second_stage.stage_run_id,
                issue_revision: "issue-rev-2".to_string(),
                configuration_revision: "config-rev".to_string(),
                artifact: first.artifact.clone(),
                review_cycles: 1,
                unresolved_blocking_findings: vec![],
                bytes_len: 100,
                usage: StageUsage::default(),
            })
            .unwrap();
        assert_eq!(second.version, 2);
        assert_eq!(
            store.get_spec_artifact(&first.artifact_id).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn stores_immutable_successful_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let attempt = store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .unwrap();
        let first = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: attempt.stage_run_id.clone(),
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "config-rev".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: artifact(TriageRoute::Implement),
                bytes_len: 200,
                usage: StageUsage {
                    input_tokens: 1,
                    output_tokens: 2,
                    total_tokens: 3,
                },
            })
            .unwrap();

        assert_eq!(first.artifact.route, TriageRoute::Implement);
        assert!(store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: attempt.stage_run_id,
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "config-rev".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: artifact(TriageRoute::Spec),
                bytes_len: 200,
                usage: StageUsage::default(),
            })
            .is_err());
    }

    /// Late output from an attempt a restart already gave up on must not be able
    /// to resurrect it as a success; the interrupted attempt has to stay
    /// terminal so retry accounting and `max_attempts` keep meaning something.
    #[test]
    fn interrupted_attempt_cannot_be_completed_by_late_output() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let attempt = store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .unwrap();
        store
            .interrupt_attempt(&attempt.stage_run_id, "owner-1")
            .unwrap();

        let late = store.store_artifact(StoreArtifactRequest {
            stage_run_id: attempt.stage_run_id.clone(),
            issue_revision: "issue-rev".to_string(),
            configuration_revision: "config-rev".to_string(),
            route_mapping_hash: "routes".to_string(),
            artifact: artifact(TriageRoute::Implement),
            bytes_len: 200,
            usage: StageUsage::default(),
        });

        assert!(late.is_err(), "interrupted attempt must reject late output");
        let stage = store.get_stage_run(&attempt.stage_run_id).unwrap().unwrap();
        assert_eq!(stage.status, StageStatus::Interrupted);
        assert!(store.get_latest_artifact(&stage.run_id).unwrap().is_none());
    }

    /// Recovery can only reclaim what the running attempt recorded, so the
    /// child's identity and disposable paths must survive into the interrupted
    /// record; a claim that never recorded a process must not be swept.
    #[test]
    fn interrupted_attempt_exposes_recorded_process_for_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let attempt = store
            .claim_attempt(ClaimAttemptRequest {
                workspace_path: None,
                output_path: None,
                pid: None,
                process_group_id: None,
                process_start_token: None,
                executable_identity: None,
                ..claim_request("issue-rev", "config-rev")
            })
            .unwrap();

        assert!(
            store.list_recoverable_attempts().unwrap().is_empty(),
            "a running attempt is not recoverable work"
        );

        store
            .record_attempt_process(RecordAttemptProcessRequest {
                stage_run_id: attempt.stage_run_id.clone(),
                owner_instance: "owner-1".to_string(),
                identity: ProcessIdentity {
                    pid: 4321,
                    process_group_id: 4321,
                    start_token: Some("99887766".to_string()),
                    executable: Some("/usr/bin/pi".to_string()),
                },
                workspace_path: Some("/tmp/attempt/workspace".to_string()),
                output_path: Some("/tmp/attempt/stage-output/result.json".to_string()),
            })
            .unwrap();
        store
            .interrupt_attempt(&attempt.stage_run_id, "owner-1")
            .unwrap();

        let recoverable = store.list_recoverable_attempts().unwrap();
        assert_eq!(recoverable.len(), 1);
        let identity = recoverable[0].identity.as_ref().expect("identity recorded");
        assert_eq!(identity.pid, 4321);
        assert_eq!(identity.process_group_id, 4321);
        assert_eq!(identity.start_token.as_deref(), Some("99887766"));
        assert_eq!(identity.executable.as_deref(), Some("/usr/bin/pi"));
        assert_eq!(
            recoverable[0].workspace_path.as_deref(),
            Some("/tmp/attempt/workspace")
        );

        store.clear_attempt_process(&attempt.stage_run_id).unwrap();
        assert!(
            store.list_recoverable_attempts().unwrap().is_empty(),
            "a reclaimed attempt must not be swept again"
        );
    }

    /// Correction measurement re-reads published issues from durable records,
    /// so only applied automatic publications are candidates, and each artifact
    /// records a given observation once.
    #[test]
    fn lists_applied_automatic_publications_as_correction_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let attempt = store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .unwrap();
        let artifact = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: attempt.stage_run_id,
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "config-rev".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: artifact(TriageRoute::Implement),
                bytes_len: 200,
                usage: StageUsage::default(),
            })
            .unwrap();
        let intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact.run_id.clone(),
                artifact_id: Some(artifact.artifact_id.clone()),
                mode: PublicationMode::Automatic,
                intake_label: "needs-triage".to_string(),
                route_label: "ready-for-agent".to_string(),
                project_state: Some("Todo".to_string()),
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": "automatic_route"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();

        assert!(
            store.list_correction_candidates(10).unwrap().is_empty(),
            "a pending publication is not yet a measurable decision"
        );

        store
            .update_publication_step(
                &intent.intent_id,
                "comment_applied",
                PublicationStatus::Applied,
                None,
            )
            .unwrap();

        let candidates = store.list_correction_candidates(10).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].published_route, "implement");
        assert_eq!(candidates[0].issue_identifier, "#123");
        assert_eq!(candidates[0].intake_label, "needs-triage");

        assert!(store
            .record_route_observation(
                &artifact.artifact_id,
                RouteObservationKind::Corrected,
                "spec"
            )
            .unwrap());
        assert!(
            !store
                .record_route_observation(
                    &artifact.artifact_id,
                    RouteObservationKind::Corrected,
                    "spec"
                )
                .unwrap(),
            "the same correction must only ever be recorded once"
        );
    }

    /// A later triage attempt on the same run supersedes the earlier publication
    /// for correction measurement; comparing against both would count a legitimate
    /// new route as a correction to the old artifact.
    #[test]
    fn correction_candidates_use_latest_applied_publication_per_run() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);

        let first_attempt = store
            .claim_attempt(claim_request("issue-rev-1", "config-rev"))
            .unwrap();
        let first_artifact = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: first_attempt.stage_run_id,
                issue_revision: "issue-rev-1".to_string(),
                configuration_revision: "config-rev".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: artifact(TriageRoute::Implement),
                bytes_len: 200,
                usage: StageUsage::default(),
            })
            .unwrap();
        let first_intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: first_artifact.run_id.clone(),
                artifact_id: Some(first_artifact.artifact_id.clone()),
                mode: PublicationMode::Automatic,
                intake_label: "needs-triage".to_string(),
                route_label: "ready-for-agent".to_string(),
                project_state: Some("Todo".to_string()),
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": "automatic_route"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();
        store
            .update_publication_step(
                &first_intent.intent_id,
                "comment_applied",
                PublicationStatus::Applied,
                None,
            )
            .unwrap();

        let second_attempt = store
            .claim_attempt(claim_request("issue-rev-2", "config-rev"))
            .unwrap();
        let second_artifact = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: second_attempt.stage_run_id,
                issue_revision: "issue-rev-2".to_string(),
                configuration_revision: "config-rev".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: artifact(TriageRoute::Spec),
                bytes_len: 200,
                usage: StageUsage::default(),
            })
            .unwrap();
        assert_eq!(
            second_artifact.run_id, first_artifact.run_id,
            "both attempts belong to the same factory run"
        );
        let second_intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: second_artifact.run_id.clone(),
                artifact_id: Some(second_artifact.artifact_id.clone()),
                mode: PublicationMode::Automatic,
                intake_label: "needs-triage".to_string(),
                route_label: "ready-to-spec".to_string(),
                project_state: Some("Todo".to_string()),
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": "automatic_route"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();
        store
            .update_publication_step(
                &second_intent.intent_id,
                "comment_applied",
                PublicationStatus::Applied,
                None,
            )
            .unwrap();

        let candidates = store.list_correction_candidates(10).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].artifact_id, second_artifact.artifact_id);
        assert_eq!(candidates[0].published_route, "spec");
        assert_eq!(candidates[0].intent_id, second_intent.intent_id);
    }

    #[test]
    fn creates_and_updates_pending_publication_intents() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let attempt = store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .unwrap();
        let artifact = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: attempt.stage_run_id,
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "config-rev".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: artifact(TriageRoute::Implement),
                bytes_len: 200,
                usage: StageUsage::default(),
            })
            .unwrap();
        let intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact.run_id,
                artifact_id: Some(artifact.artifact_id),
                mode: PublicationMode::Preview,
                intake_label: "needs-triage".to_string(),
                route_label: "ready-for-agent".to_string(),
                project_state: Some("Todo".to_string()),
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"label": "ready-for-agent"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();

        assert_eq!(store.list_pending_intents(10).unwrap().len(), 1);
        store
            .update_publication_step(
                &intent.intent_id,
                "comment_pending",
                PublicationStatus::Applied,
                None,
            )
            .unwrap();
        assert!(store.list_pending_intents(10).unwrap().is_empty());
    }

    #[test]
    fn lists_pending_automatic_dispatch_guards_only() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let attempt = store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .unwrap();
        let artifact = store
            .store_artifact(StoreArtifactRequest {
                stage_run_id: attempt.stage_run_id,
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "config-rev".to_string(),
                route_mapping_hash: "routes".to_string(),
                artifact: artifact(TriageRoute::Implement),
                bytes_len: 200,
                usage: StageUsage::default(),
            })
            .unwrap();

        store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact.run_id.clone(),
                artifact_id: Some(artifact.artifact_id.clone()),
                mode: PublicationMode::Preview,
                intake_label: "needs-triage".to_string(),
                route_label: "ready-for-agent".to_string(),
                project_state: Some("Todo".to_string()),
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": "preview_comment"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();
        assert!(store
            .list_pending_automatic_dispatch_guards()
            .unwrap()
            .is_empty());

        let automatic = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: artifact.run_id,
                artifact_id: Some(artifact.artifact_id),
                mode: PublicationMode::Automatic,
                intake_label: "needs-triage".to_string(),
                route_label: "ready-for-agent".to_string(),
                project_state: Some("Todo".to_string()),
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": "automatic_route"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();
        let guards = store.list_pending_automatic_dispatch_guards().unwrap();
        assert_eq!(guards.len(), 1);
        assert_eq!(guards[0].issue_id, "123");
        assert_eq!(guards[0].intake_label, "needs-triage");

        store
            .update_publication_step(&automatic.intent_id, "", PublicationStatus::Conflict, None)
            .unwrap();
        let conflict_guards = store.list_pending_automatic_dispatch_guards().unwrap();
        assert_eq!(conflict_guards.len(), 1);
        assert_eq!(conflict_guards[0].issue_id, "123");

        store
            .update_publication_step(
                &automatic.intent_id,
                "comment_applied",
                PublicationStatus::Applied,
                None,
            )
            .unwrap();
        assert!(store
            .list_pending_automatic_dispatch_guards()
            .unwrap()
            .is_empty());
    }

    fn seed_approved_spec_run(store: &mut SqliteFactoryStore) -> (String, String, String) {
        let attempt = store
            .claim_stage_attempt(SPEC_STAGE_NAME, claim_request("issue-rev", "config-rev"))
            .unwrap();
        let artifact = store
            .store_spec_artifact(StoreSpecArtifactRequest {
                stage_run_id: attempt.stage_run_id.clone(),
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "config-rev".to_string(),
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
                usage: StageUsage::default(),
            })
            .unwrap();
        store
            .pin_spec_approval(&artifact.run_id, &artifact.artifact_id)
            .unwrap();
        let intent = store
            .create_spec_publication_intent(
                &artifact.run_id,
                Some(&artifact.artifact_id),
                SpecPublicationKind::Approval,
                &serde_json::json!({"intake_label": "ready-for-agent"}),
            )
            .unwrap();
        store
            .finalize_spec_approval(&artifact.run_id, &intent.intent_id, "route_applied")
            .unwrap();
        (artifact.run_id, artifact.artifact_id, attempt.stage_run_id)
    }

    #[test]
    fn migration_004_creates_implementation_tables() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let store = store(&path);
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'implementation_artifacts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migration_006_keeps_review_artifacts_immutable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let (run_id, spec_artifact_id, _) = seed_approved_spec_run(&mut store);
        let implementation_stage = store
            .claim_stage_attempt(
                IMPLEMENTATION_STAGE_NAME,
                claim_request("issue-rev", "impl-config"),
            )
            .unwrap();
        let implementation_artifact_id = "impl-artifact";
        let implementation_intent_id = "impl-intent";
        let draft_id = "draft-artifact";
        let now = Utc::now().to_rfc3339();
        store
            .conn
            .execute(
                "INSERT INTO implementation_artifacts (
                    artifact_id, run_id, stage_run_id, approved_artifact_id,
                    approved_version, issue_revision, configuration_revision,
                    schema_version, manifest_json, base_commit, head_commit,
                    approved_spec_path, validation_cycles, execution_profile,
                    received_at, bytes_len
                 ) VALUES (?1, ?2, ?3, ?4, 1, 'issue-rev', 'impl-config', 1,
                    ?5, 'base', 'head', 'spec.md', 1, 'local', ?6, 10)",
                params![
                    implementation_artifact_id,
                    run_id,
                    implementation_stage.stage_run_id,
                    spec_artifact_id,
                    serde_json::json!({"schema_version":1,"status":"completed","summary":"ok","acceptance_criteria":[],"known_limitations":[]}),
                    now,
                ],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO implementation_publication_intents (
                    intent_id, run_id, artifact_id, kind, status,
                    completed_steps_json, desired_effects_json,
                    observed_baseline_json, expected_projection_json,
                    created_at, updated_at
                 ) VALUES (?1, ?2, ?3, 'automatic', 'applied', '[]', '{}', '{}', '{}', ?4, ?4)",
                params![implementation_intent_id, run_id, implementation_artifact_id, now],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO implementation_draft_pr_artifacts (
                    artifact_id, run_id, implementation_artifact_id, intent_id,
                    number, url, draft, head, base, head_sha, marker, created_at
                 ) VALUES (?1, ?2, ?3, ?4, 42, 'https://example.test/pr/42', 1,
                    'feature', 'main', 'head-sha', 'marker', ?5)",
                params![draft_id, run_id, implementation_artifact_id, implementation_intent_id, now],
            )
            .unwrap();

        let review_stage = store
            .claim_stage_attempt(REVIEW_STAGE_NAME, claim_request("review-rev", "review-config"))
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO review_attempts (
                    attempt_id, run_id, stage_run_id, draft_pr_artifact_id,
                    implementation_artifact_id, spec_artifact_id, pr_number,
                    reviewed_head_sha, base_sha, status, created_at, updated_at
                 ) VALUES ('review-attempt', ?1, ?2, ?3, ?4, ?5, 42,
                    'head-sha', 'base-sha', 'completed', ?6, ?6)",
                params![run_id, review_stage.stage_run_id, draft_id, implementation_artifact_id, spec_artifact_id, now],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO review_findings_artifacts (
                    artifact_id, run_id, stage_run_id, attempt_id,
                    draft_pr_artifact_id, implementation_artifact_id, spec_artifact_id,
                    schema_version, reviewed_head_sha, base_sha, manifest_json,
                    no_findings, finding_count, received_at, bytes_len
                 ) VALUES ('review-artifact', ?1, ?2, 'review-attempt', ?3, ?4, ?5,
                    1, 'head-sha', 'base-sha', ?6, 1, 0, ?7, 2)",
                params![
                    run_id,
                    review_stage.stage_run_id,
                    draft_id,
                    implementation_artifact_id,
                    spec_artifact_id,
                    serde_json::json!({"schema_version":1,"reviewed_head_sha":"head-sha","base_sha":"base-sha","spec_conformance_summary":"none","no_findings":true,"findings":[]}),
                    now,
                ],
            )
            .unwrap();

        let update = store.conn.execute(
            "UPDATE review_findings_artifacts SET manifest_json = '{}' WHERE artifact_id = 'review-artifact'",
            [],
        );
        assert!(update.is_err(), "review artifact UPDATE must be rejected");
        let delete = store.conn.execute(
            "DELETE FROM review_findings_artifacts WHERE artifact_id = 'review-artifact'",
            [],
        );
        assert!(delete.is_err(), "review artifact DELETE must be rejected");
    }

    #[test]
    fn a3_dispatch_guards_cover_implementation_run_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let (run_id, approved_artifact_id, _) = seed_approved_spec_run(&mut store);
        assert!(store
            .list_pending_automatic_dispatch_guards()
            .unwrap()
            .is_empty());

        store
            .upsert_implementation_state(UpsertImplementationStateRequest {
                run_id: run_id.clone(),
                approved_artifact_id,
                approved_version: 1,
                configuration_revision: "impl-config".to_string(),
                decision: ImplementationDecision::Pending,
                successful_artifact_id: None,
                bundle_artifact_id: None,
                blocker: None,
            })
            .unwrap();
        let guards = store.list_pending_automatic_dispatch_guards().unwrap();
        assert_eq!(guards.len(), 1);
        assert_eq!(guards[0].issue_id, "123");
        assert_eq!(guards[0].intake_label, "implementation");
    }

    #[test]
    fn a3_eligible_excludes_nonterminal_attempts_and_successful_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let (run_id, approved_artifact_id, _) = seed_approved_spec_run(&mut store);
        let eligible = store.list_a3_eligible_approved_runs("impl-config").unwrap();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].run_id, run_id);

        let impl_attempt = store
            .claim_stage_attempt(
                IMPLEMENTATION_STAGE_NAME,
                claim_request("issue-rev", "impl-config"),
            )
            .unwrap();
        store
            .store_implementation_attempt_inputs(
                &impl_attempt.stage_run_id,
                &ImplementationAttemptInputs {
                    approved_artifact_id: approved_artifact_id.clone(),
                    approved_version: 1,
                    approval_issue_revision: "issue-rev".to_string(),
                    base_branch: "main".to_string(),
                    base_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                    execution_profile: ExecutionProfile::Local,
                    configuration_revision: "impl-config".to_string(),
                },
            )
            .unwrap();
        assert!(store
            .list_a3_eligible_approved_runs("impl-config")
            .unwrap()
            .is_empty());

        // Terminate the attempt so eligibility can be reconsidered, then store a
        // successful artifact which must permanently exclude the pin+config pair.
        store
            .fail_attempt(
                &impl_attempt.stage_run_id,
                FactoryError::new("test", "store", "interrupt", false, None),
            )
            .unwrap();
        assert_eq!(
            store
                .list_a3_eligible_approved_runs("impl-config")
                .unwrap()
                .len(),
            1
        );

        // Pending/awaiting_human/previewed decisions must exclude eligibility;
        // only absent state or decision=failed may retry.
        store
            .upsert_implementation_state(UpsertImplementationStateRequest {
                run_id: run_id.clone(),
                approved_artifact_id: approved_artifact_id.clone(),
                approved_version: 1,
                configuration_revision: "impl-config".to_string(),
                decision: ImplementationDecision::Pending,
                successful_artifact_id: None,
                bundle_artifact_id: None,
                blocker: None,
            })
            .unwrap();
        assert!(store
            .list_a3_eligible_approved_runs("impl-config")
            .unwrap()
            .is_empty());
        store
            .set_implementation_decision(&run_id, ImplementationDecision::AwaitingHuman, None)
            .unwrap();
        assert!(store
            .list_a3_eligible_approved_runs("impl-config")
            .unwrap()
            .is_empty());
        store
            .set_implementation_decision(&run_id, ImplementationDecision::Failed, None)
            .unwrap();
        assert_eq!(
            store
                .list_a3_eligible_approved_runs("impl-config")
                .unwrap()
                .len(),
            1
        );

        let retry = store
            .claim_stage_attempt(
                IMPLEMENTATION_STAGE_NAME,
                claim_request("issue-rev", "impl-config"),
            )
            .unwrap();
        let manifest = ImplementationManifest {
            schema_version: 1,
            status: crate::implementation::domain::ManifestStatus::Completed,
            head_commit: Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()),
            summary: "Done".to_string(),
            acceptance_criteria: vec![crate::implementation::domain::AcceptanceCriterionClaim {
                index: 1,
                status: crate::implementation::domain::CriterionStatus::Implemented,
                evidence: vec![crate::implementation::domain::ImplementationEvidence {
                    kind: crate::implementation::domain::EvidenceKind::Repository,
                    reference: "src/x.rs".to_string(),
                    summary: "Change".to_string(),
                }],
            }],
            known_limitations: vec![],
            blocker: None,
        };
        store
            .store_implementation_artifact(StoreImplementationArtifactRequest {
                stage_run_id: retry.stage_run_id,
                approved_artifact_id,
                approved_version: 1,
                issue_revision: "issue-rev".to_string(),
                configuration_revision: "impl-config".to_string(),
                manifest,
                base_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                head_commit: Some("4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string()),
                approved_spec_path: "specs/#123/APPROVED-v1.md".to_string(),
                validation_cycles: 1,
                execution_profile: ExecutionProfile::Local,
                bytes_len: 128,
                usage: StageUsage::default(),
            })
            .unwrap();
        assert!(store
            .list_a3_eligible_approved_runs("impl-config")
            .unwrap()
            .is_empty());
        let guards = store.list_pending_automatic_dispatch_guards().unwrap();
        assert!(guards.iter().any(|g| g.intake_label == "implementation"));
    }

    #[test]
    fn renews_and_interrupts_leases() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let attempt = store
            .claim_attempt(claim_request("issue-rev", "config-rev"))
            .unwrap();

        assert!(store.renew_lease(&attempt.stage_run_id, "owner-1").unwrap());
        store
            .conn
            .execute(
                "UPDATE stage_runs SET lease_heartbeat_at = ?1 WHERE stage_run_id = ?2",
                params![
                    ts(Utc::now() - Duration::milliseconds(TRIAGE_LEASE_STALE_AFTER_MS + 1_000)),
                    attempt.stage_run_id
                ],
            )
            .unwrap();

        assert_eq!(store.interrupt_stale_attempts().unwrap(), 1);
    }

    #[test]
    fn upserts_ineligible_run_and_lists_verified_comment_identities() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let run = store
            .upsert_factory_run(UpsertFactoryRunRequest {
                forge_host: "github.com".to_string(),
                repository: "owner/repo".to_string(),
                issue_id: "55".to_string(),
                issue_identifier: "#55".to_string(),
                issue_revision: Some("rev".to_string()),
                status: FactoryRunStatus::Ineligible,
                current_stage: Some(TRIAGE_STAGE_NAME.to_string()),
            })
            .unwrap();
        assert_eq!(run.status, FactoryRunStatus::Ineligible);

        let intent = store
            .create_publication_intent(CreatePublicationIntentRequest {
                run_id: run.run_id.clone(),
                artifact_id: None,
                mode: PublicationMode::Preview,
                intake_label: "needs-triage".to_string(),
                route_label: String::new(),
                project_state: None,
                route_mapping_hash: "routes".to_string(),
                desired_effects: serde_json::json!({"kind": "ineligible_diagnostic"}),
                observed_baseline: serde_json::json!({}),
                expected_projection: serde_json::json!({}),
            })
            .unwrap();
        store
            .set_publication_comment(&intent.intent_id, "999", "bot")
            .unwrap();

        let identities = store.list_verified_comment_identities(&run.run_id).unwrap();
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].comment_id, "999");
        assert_eq!(identities[0].publisher_login, "bot");

        let attempt = store
            .claim_attempt(claim_request("issue-rev-2", "config-rev-2"))
            .unwrap();
        store
            .fail_attempt(
                &attempt.stage_run_id,
                FactoryError::new("boom", "runner", "retry", true, None),
            )
            .unwrap();
        let failed = store.get_stage_run(&attempt.stage_run_id).unwrap().unwrap();
        assert_eq!(failed.status, StageStatus::Failed);
        assert!(failed.error.is_some());
    }

    #[test]
    fn implementation_publication_error_rejects_unknown_intent() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut store = store(&path);
        let err = store
            .set_implementation_publication_error(
                "missing-intent",
                PublicationStatus::Pending,
                FactoryError::new(
                    "retry",
                    "implementation_publication",
                    "retry later",
                    true,
                    None,
                ),
            )
            .unwrap_err();
        assert!(err.to_string().contains("missing-intent"));
    }
}
