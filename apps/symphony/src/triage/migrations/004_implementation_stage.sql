-- A3 implementation-stage durability. Additive tables only; A1/A2 indexes
-- and uniqueness rules are unchanged.

CREATE TABLE IF NOT EXISTS implementation_attempt_inputs (
  stage_run_id TEXT PRIMARY KEY REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  approved_artifact_id TEXT NOT NULL REFERENCES spec_artifacts(artifact_id),
  approved_version INTEGER NOT NULL,
  approval_issue_revision TEXT NOT NULL,
  base_branch TEXT NOT NULL,
  base_commit TEXT NOT NULL,
  execution_profile TEXT NOT NULL,
  configuration_revision TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_implementation_one_nonterminal_pin
ON stage_runs(run_id)
WHERE stage = 'implementation' AND status IN ('pending', 'running');

CREATE TABLE IF NOT EXISTS implementation_turns (
  turn_id TEXT PRIMARY KEY,
  stage_run_id TEXT NOT NULL REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL,
  harness TEXT NOT NULL,
  model TEXT,
  execution_profile TEXT NOT NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  output_json TEXT,
  error_json TEXT,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  UNIQUE(stage_run_id, ordinal)
);

CREATE TABLE IF NOT EXISTS implementation_validation_cycles (
  cycle_id TEXT PRIMARY KEY,
  stage_run_id TEXT NOT NULL REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  cycle INTEGER NOT NULL,
  passed INTEGER NOT NULL,
  results_json TEXT NOT NULL,
  started_at TEXT NOT NULL,
  completed_at TEXT NOT NULL,
  UNIQUE(stage_run_id, cycle)
);

CREATE TABLE IF NOT EXISTS implementation_artifacts (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  stage_run_id TEXT NOT NULL UNIQUE REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  approved_artifact_id TEXT NOT NULL REFERENCES spec_artifacts(artifact_id),
  approved_version INTEGER NOT NULL,
  issue_revision TEXT NOT NULL,
  configuration_revision TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  manifest_json TEXT NOT NULL,
  base_commit TEXT NOT NULL,
  head_commit TEXT,
  approved_spec_path TEXT NOT NULL,
  validation_cycles INTEGER NOT NULL,
  execution_profile TEXT NOT NULL,
  received_at TEXT NOT NULL,
  bytes_len INTEGER NOT NULL,
  UNIQUE(run_id, approved_artifact_id, configuration_revision)
);

CREATE INDEX IF NOT EXISTS idx_implementation_artifacts_run
ON implementation_artifacts(run_id, received_at DESC);

CREATE TABLE IF NOT EXISTS implementation_bundle_artifacts (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  stage_run_id TEXT NOT NULL REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  implementation_artifact_id TEXT NOT NULL UNIQUE REFERENCES implementation_artifacts(artifact_id) ON DELETE CASCADE,
  base_commit TEXT NOT NULL,
  head_commit TEXT NOT NULL,
  tree_sha TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  bytes_len INTEGER NOT NULL,
  changed_file_count INTEGER NOT NULL,
  changed_paths_json TEXT NOT NULL,
  approved_spec_path TEXT NOT NULL,
  approved_spec_blob_sha TEXT NOT NULL,
  harness TEXT NOT NULL,
  model TEXT,
  execution_profile TEXT NOT NULL,
  created_at TEXT NOT NULL,
  verified_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_implementation_bundle_sha256
ON implementation_bundle_artifacts(sha256);

CREATE TABLE IF NOT EXISTS implementation_run_state (
  run_id TEXT PRIMARY KEY REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  approved_artifact_id TEXT NOT NULL REFERENCES spec_artifacts(artifact_id),
  approved_version INTEGER NOT NULL,
  configuration_revision TEXT NOT NULL,
  decision TEXT NOT NULL DEFAULT 'pending',
  successful_artifact_id TEXT REFERENCES implementation_artifacts(artifact_id) ON DELETE SET NULL,
  bundle_artifact_id TEXT REFERENCES implementation_bundle_artifacts(artifact_id) ON DELETE SET NULL,
  blocker_json TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS implementation_publication_intents (
  intent_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  artifact_id TEXT REFERENCES implementation_artifacts(artifact_id) ON DELETE SET NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL,
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

CREATE INDEX IF NOT EXISTS idx_implementation_publication_intents_pending
ON implementation_publication_intents(status, updated_at);

CREATE UNIQUE INDEX IF NOT EXISTS idx_implementation_one_nonterminal_publication
ON implementation_publication_intents(artifact_id)
WHERE artifact_id IS NOT NULL AND status IN ('pending', 'conflict', 'blocked');
