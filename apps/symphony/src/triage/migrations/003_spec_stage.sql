-- A2 spec-stage durability. All changes are additive except replacing the A1
-- nonterminal index with its stage-scoped equivalent.
DROP INDEX IF EXISTS idx_stage_runs_one_nonterminal_revision;
CREATE UNIQUE INDEX IF NOT EXISTS idx_stage_runs_one_nonterminal_revision
ON stage_runs(run_id, stage, issue_revision, configuration_revision)
WHERE status IN ('pending', 'running');

CREATE TABLE IF NOT EXISTS spec_turns (
  turn_id TEXT PRIMARY KEY,
  stage_run_id TEXT NOT NULL REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL,
  harness TEXT NOT NULL,
  model TEXT,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  output_json TEXT,
  error_json TEXT,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  UNIQUE(stage_run_id, ordinal)
);

CREATE TABLE IF NOT EXISTS spec_artifacts (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  stage_run_id TEXT NOT NULL UNIQUE REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  issue_revision TEXT NOT NULL,
  configuration_revision TEXT NOT NULL,
  version INTEGER NOT NULL,
  schema_version INTEGER NOT NULL,
  artifact_json TEXT NOT NULL,
  review_cycles INTEGER NOT NULL,
  unresolved_findings_json TEXT NOT NULL,
  received_at TEXT NOT NULL,
  bytes_len INTEGER NOT NULL,
  UNIQUE(run_id, version),
  UNIQUE(run_id, issue_revision, configuration_revision)
);

CREATE INDEX IF NOT EXISTS idx_spec_artifacts_run_version
ON spec_artifacts(run_id, version DESC);

CREATE TABLE IF NOT EXISTS spec_run_state (
  run_id TEXT PRIMARY KEY REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  revision_requests_used INTEGER NOT NULL DEFAULT 0,
  pending_approval_version INTEGER,
  approved_version INTEGER,
  approved_artifact_id TEXT REFERENCES spec_artifacts(artifact_id) ON DELETE SET NULL,
  decision TEXT NOT NULL DEFAULT 'awaiting_decision',
  updated_at TEXT NOT NULL
);

-- Spec intents are separate because A1 publication_intents has a triage-artifact
-- foreign key and a fixed effect shape.
CREATE TABLE IF NOT EXISTS spec_publication_intents (
  intent_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  artifact_id TEXT REFERENCES spec_artifacts(artifact_id) ON DELETE SET NULL,
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

CREATE INDEX IF NOT EXISTS idx_spec_publication_intents_pending
ON spec_publication_intents(status, updated_at);
