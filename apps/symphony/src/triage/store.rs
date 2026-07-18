use crate::error::{Result, SymphonyError};
use crate::triage::domain::{
    truncate_utf8_bytes, ArtifactRecord, FactoryError, FactoryEventRecord, FactoryRunRecord,
    FactoryRunStatus, PublicationIntentRecord, PublicationMode, PublicationStatus, StageRunRecord,
    StageStatus, StageUsage, TriageArtifact, TriageMetricsAggregate,
    FACTORY_ERROR_STRING_MAX_BYTES, FACTORY_EVENT_PAYLOAD_MAX_BYTES, TRIAGE_LEASE_STALE_AFTER_MS,
    TRIAGE_STAGE_NAME,
};
use crate::triage::storage_path::lock_path_for_storage;
use chrono::{DateTime, Duration, Utc};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::Duration as StdDuration;
use uuid::Uuid;

const INIT_SQL: &str = include_str!("migrations/001_init.sql");

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
    fn list_intents_for_run(&self, run_id: &str) -> Result<Vec<PublicationIntentRecord>>;
    fn list_verified_comment_identities(&self, run_id: &str) -> Result<Vec<StoredCommentIdentity>>;
    fn list_stage_attempts_for_revision(
        &self,
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
    fn set_publication_comment(
        &mut self,
        intent_id: &str,
        comment_id: &str,
        publisher_login: &str,
    ) -> Result<()>;
    fn record_event(&mut self, event: FactoryEventRecord) -> Result<()>;
    fn renew_lease(&mut self, stage_run_id: &str, owner_instance: &str) -> Result<bool>;
    fn interrupt_stale_attempts(&mut self) -> Result<u64>;
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
        conn.execute_batch(INIT_SQL).map_err(storage_error)?;

        Ok(Self {
            conn,
            _lock_file: lock_file,
        })
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
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
                params![
                    run_id,
                    issue_revision,
                    configuration_revision,
                    TRIAGE_STAGE_NAME
                ],
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
}
