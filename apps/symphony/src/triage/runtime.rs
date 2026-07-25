//! Runtime wiring for triage: shared store, HTTP query, and event emission.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::domain::{EventKind, EventSeverity, ServiceConfig};
use crate::error::{Result, SymphonyError};
use crate::event_stream::EventHub;
use crate::github::client::GithubClient;
use crate::github::projects_v2::ProjectsV2Client;
use crate::http_server::{
    factory_run_http_response, factory_run_metrics_http_response, FactoryRunHttpResponse,
    FactoryRunMetricsHttpResponse, FactoryRunQuery,
};
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
    ClaimAttemptRequest, CreatePublicationIntentRequest, FactoryRunStore,
    PendingAutomaticDispatchGuard, SqliteFactoryStore, StoreArtifactRequest, StoredCommentIdentity,
    UpsertFactoryRunRequest,
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
            Ok(Some(factory_run_http_response(
                &run,
                &attempts,
                artifact.as_ref(),
                publication.as_ref(),
            )))
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
        run_id: &str,
        issue_revision: &str,
        configuration_revision: &str,
    ) -> Result<Vec<StageRunRecord>> {
        self.with_store(|store| {
            store.list_stage_attempts_for_revision(run_id, issue_revision, configuration_revision)
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
    coordinator: TriageCoordinator<SharedFactoryStore, GithubTriageIntake, GithubClient>,
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
        if config.triage.enabled {
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
        if !config.triage.enabled {
            return Ok(None);
        }

        if !matches!(config.tracker.kind.as_deref(), Some("github")) {
            return Err(SymphonyError::TriageError(
                "triage requires tracker.kind=github".to_string(),
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
                    "tracker.repo_owner is required for triage".to_string(),
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
                    "tracker.repo_name is required for triage".to_string(),
                )
            })?;
        let project_number = config.tracker.github_project_number.ok_or_else(|| {
            SymphonyError::InvalidWorkflowConfig(
                "tracker.github_project_number is required for triage".to_string(),
            )
        })?;

        let resolved = crate::github::auth::resolve_github_token(&config.tracker)
            .ok_or(SymphonyError::MissingGithubApiToken)?;
        let token = resolved.token;

        let forge_host = forge_host_from_endpoint(&config.tracker.endpoint);
        let repository = format!("{owner}/{repo}");
        let storage_path = resolve_storage_path(&config.storage, &forge_host, owner, repo);
        tracing::info!(
            event = "triage_storage_resolved",
            path = %storage_path_for_log(&storage_path),
            mode = %config.triage.mode,
            enabled = config.triage.enabled,
            "resolved triage SQLite storage path"
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
        let managed_labels = config.triage.routes.managed_labels();
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
            config.triage.max_intake_pages,
        );

        let workflow_dir = workflow_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| Path::new(".").to_path_buf());
        let owner_instance = format!("symphony-{}-{}", std::process::id(), uuid::Uuid::new_v4());
        let project_display_name = format!("#{project_number}");

        let mut coordinator = TriageCoordinator::new(
            store.clone(),
            intake,
            client,
            TriageCoordinatorConfig {
                forge_host,
                repository,
                owner_instance,
                workflow_dir,
                project_display_name,
            },
        )
        .with_routing(routing);
        let sessions = Arc::new(Mutex::new(crate::domain::TriageSessionRegistry::default()));
        coordinator = coordinator.with_session_registry(sessions.clone());
        if let Some(hub) = event_hub {
            coordinator = coordinator.with_events(Arc::new(EventHubEmitter::new(hub)));
        }

        Ok(Some(Self {
            coordinator,
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
        self.coordinator.poll_once(config).await
    }
}
