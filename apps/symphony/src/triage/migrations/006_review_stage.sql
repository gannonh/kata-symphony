-- A4 agent-review durability. Additive only; A1/A2/A3 tables and
-- uniqueness rules are unchanged.

CREATE UNIQUE INDEX IF NOT EXISTS idx_review_one_nonterminal_pin
ON stage_runs(run_id)
WHERE stage = 'review' AND status IN ('pending', 'running');

-- One durable A4 attempt per stage_run. Re-prompts are turns inside the same
-- attempt, so restart can recover the pinned inputs and bounded prompt budget.
CREATE TABLE IF NOT EXISTS review_attempts (
  attempt_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  stage_run_id TEXT NOT NULL UNIQUE REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  draft_pr_artifact_id TEXT NOT NULL REFERENCES implementation_draft_pr_artifacts(artifact_id),
  implementation_artifact_id TEXT NOT NULL REFERENCES implementation_artifacts(artifact_id),
  spec_artifact_id TEXT NOT NULL REFERENCES spec_artifacts(artifact_id),
  pr_number INTEGER NOT NULL,
  reviewed_head_sha TEXT NOT NULL,
  base_sha TEXT NOT NULL,
  status TEXT NOT NULL,
  reprompt_count INTEGER NOT NULL DEFAULT 0,
  worker_turn_json TEXT,
  manifest_json TEXT,
  validation_result_json TEXT,
  last_error_json TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_review_attempts_run_head
ON review_attempts(run_id, reviewed_head_sha, created_at DESC);

-- Immutable, validated worker output. A new PR head opens a new cycle; the
-- (run, reviewed_head_sha) key prevents duplicate completed cycles.
CREATE TABLE IF NOT EXISTS review_findings_artifacts (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  stage_run_id TEXT NOT NULL UNIQUE REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  attempt_id TEXT NOT NULL UNIQUE REFERENCES review_attempts(attempt_id) ON DELETE CASCADE,
  draft_pr_artifact_id TEXT NOT NULL REFERENCES implementation_draft_pr_artifacts(artifact_id),
  implementation_artifact_id TEXT NOT NULL REFERENCES implementation_artifacts(artifact_id),
  spec_artifact_id TEXT NOT NULL REFERENCES spec_artifacts(artifact_id),
  schema_version INTEGER NOT NULL,
  reviewed_head_sha TEXT NOT NULL,
  base_sha TEXT NOT NULL,
  manifest_json TEXT NOT NULL,
  no_findings INTEGER NOT NULL,
  finding_count INTEGER NOT NULL,
  received_at TEXT NOT NULL,
  bytes_len INTEGER NOT NULL,
  UNIQUE(run_id, reviewed_head_sha)
);

CREATE INDEX IF NOT EXISTS idx_review_findings_artifacts_run
ON review_findings_artifacts(run_id, received_at DESC);

-- PR1 uses this for the marker-owned preview issue comment. PR2 extends the
-- progressive completed-step vocabulary for atomic review publication/routing.
CREATE TABLE IF NOT EXISTS review_publication_intents (
  intent_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  artifact_id TEXT REFERENCES review_findings_artifacts(artifact_id) ON DELETE SET NULL,
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

CREATE INDEX IF NOT EXISTS idx_review_publication_intents_pending
ON review_publication_intents(status, updated_at);

CREATE UNIQUE INDEX IF NOT EXISTS idx_review_one_nonterminal_publication
ON review_publication_intents(artifact_id)
WHERE artifact_id IS NOT NULL AND status IN ('pending', 'conflict', 'blocked');

-- Per-finding durable identities make lifecycle classification possible in PR2
-- without mutating the immutable manifest artifact from the original cycle.
CREATE TABLE IF NOT EXISTS review_finding_records (
  finding_record_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  artifact_id TEXT NOT NULL REFERENCES review_findings_artifacts(artifact_id) ON DELETE CASCADE,
  finding_id TEXT NOT NULL,
  identity_key TEXT NOT NULL,
  reviewed_head_sha TEXT NOT NULL,
  severity TEXT NOT NULL,
  category TEXT NOT NULL,
  path TEXT NOT NULL,
  line INTEGER NOT NULL,
  end_line INTEGER,
  claim TEXT NOT NULL,
  rationale TEXT NOT NULL,
  remediation TEXT NOT NULL,
  acceptance_criterion TEXT,
  confidence REAL NOT NULL,
  lifecycle_state TEXT NOT NULL DEFAULT 'new',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(artifact_id, finding_id),
  UNIQUE(run_id, reviewed_head_sha, identity_key)
);

CREATE INDEX IF NOT EXISTS idx_review_finding_records_run_head
ON review_finding_records(run_id, reviewed_head_sha, lifecycle_state);
