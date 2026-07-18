PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS factory_runs (
  run_id TEXT PRIMARY KEY,
  forge_host TEXT NOT NULL,
  repository TEXT NOT NULL,
  issue_id TEXT NOT NULL,
  issue_identifier TEXT NOT NULL,
  issue_revision TEXT,
  status TEXT NOT NULL,
  current_stage TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (forge_host, repository, issue_id)
);

CREATE TABLE IF NOT EXISTS stage_runs (
  stage_run_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  stage TEXT NOT NULL,
  issue_revision TEXT NOT NULL,
  configuration_revision TEXT NOT NULL,
  attempt INTEGER NOT NULL,
  owner_instance TEXT,
  pid INTEGER,
  process_group_id INTEGER,
  process_start_token TEXT,
  executable_identity TEXT,
  lease_heartbeat_at TEXT,
  status TEXT NOT NULL,
  harness TEXT NOT NULL,
  model TEXT,
  workspace_path TEXT,
  output_path TEXT,
  started_at TEXT,
  completed_at TEXT,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  error_json TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (run_id, stage, attempt)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_stage_runs_one_nonterminal_revision
ON stage_runs(run_id, issue_revision, configuration_revision)
WHERE status IN ('pending', 'running');

CREATE TABLE IF NOT EXISTS triage_artifacts (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  stage_run_id TEXT NOT NULL UNIQUE REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  issue_revision TEXT NOT NULL,
  configuration_revision TEXT NOT NULL,
  route_mapping_hash TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  route TEXT NOT NULL,
  risk_class TEXT NOT NULL,
  artifact_json TEXT NOT NULL,
  received_at TEXT NOT NULL,
  bytes_len INTEGER NOT NULL,
  UNIQUE (run_id, issue_revision, configuration_revision)
);

CREATE TABLE IF NOT EXISTS publication_intents (
  intent_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  artifact_id TEXT REFERENCES triage_artifacts(artifact_id) ON DELETE SET NULL,
  mode TEXT NOT NULL,
  status TEXT NOT NULL,
  intake_label TEXT NOT NULL,
  route_label TEXT NOT NULL,
  project_state TEXT,
  route_mapping_hash TEXT NOT NULL,
  completed_steps_json TEXT NOT NULL,
  retry_count INTEGER NOT NULL DEFAULT 0,
  last_error_json TEXT,
  comment_id TEXT,
  publisher_login TEXT,
  desired_effects_json TEXT NOT NULL,
  observed_baseline_json TEXT NOT NULL,
  expected_projection_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_publication_intents_pending
ON publication_intents(status, updated_at);

CREATE TABLE IF NOT EXISTS factory_events (
  event_id TEXT PRIMARY KEY,
  run_id TEXT REFERENCES factory_runs(run_id) ON DELETE SET NULL,
  stage_run_id TEXT REFERENCES stage_runs(stage_run_id) ON DELETE SET NULL,
  event_type TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_factory_events_run_id
ON factory_events(run_id, timestamp);
