-- A5 verification-stage durability (#617). Additive only; A1-A4 tables and
-- uniqueness rules are unchanged.

CREATE UNIQUE INDEX IF NOT EXISTS idx_verification_one_nonterminal_pin
ON stage_runs(run_id)
WHERE stage = 'verification' AND status IN ('pending', 'running');

-- One durable A5 attempt per stage_run, persisted before execution together
-- with the pinned PR revision and every input artifact identity.
CREATE TABLE IF NOT EXISTS verification_attempts (
  attempt_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  stage_run_id TEXT NOT NULL UNIQUE REFERENCES stage_runs(stage_run_id) ON DELETE CASCADE,
  pr_number INTEGER NOT NULL,
  reviewed_head_sha TEXT NOT NULL,
  base_sha TEXT NOT NULL,
  spec_artifact_id TEXT NOT NULL REFERENCES spec_artifacts(artifact_id),
  implementation_artifact_id TEXT NOT NULL REFERENCES implementation_artifacts(artifact_id),
  review_artifact_id TEXT NOT NULL REFERENCES review_findings_artifacts(artifact_id),
  configuration_revision TEXT NOT NULL,
  execution_profile TEXT NOT NULL,
  status TEXT NOT NULL,
  workspace_path TEXT,
  evidence_dir TEXT,
  error_json TEXT,
  verifier_pid INTEGER,
  verifier_process_group_id INTEGER,
  verifier_start_token TEXT,
  verifier_executable TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Durable verifier launch identity (idempotent for pre-existing stores).
ALTER TABLE verification_attempts ADD COLUMN verifier_pid INTEGER;
ALTER TABLE verification_attempts ADD COLUMN verifier_process_group_id INTEGER;
ALTER TABLE verification_attempts ADD COLUMN verifier_start_token TEXT;
ALTER TABLE verification_attempts ADD COLUMN verifier_executable TEXT;

CREATE INDEX IF NOT EXISTS idx_verification_attempts_run
ON verification_attempts(run_id, created_at DESC);

-- One durable command run per configured command per attempt. The launch
-- barrier records status 'launching' plus a nonce before spawn; the controller
-- CAS-transitions to 'running' with the captured PID/process-group/start-token
-- or container identity before releasing the payload.
CREATE TABLE IF NOT EXISTS verification_command_runs (
  command_run_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  attempt_id TEXT NOT NULL REFERENCES verification_attempts(attempt_id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('test', 'acceptance')),
  configuration_revision TEXT NOT NULL,
  command_sha256 TEXT NOT NULL,
  status TEXT NOT NULL CHECK (
    status IN ('launching', 'running', 'completed', 'failed', 'interrupted', 'not_run')
  ),
  launch_nonce TEXT,
  pid INTEGER,
  process_group_id INTEGER,
  process_start_token TEXT,
  executable_identity TEXT,
  container_id TEXT,
  started_at TEXT,
  completed_at TEXT,
  duration_ms INTEGER,
  exit_code INTEGER,
  termination_reason TEXT,
  passed INTEGER,
  output_tail TEXT,
  output_sha256 TEXT,
  execution_profile TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(attempt_id, ordinal)
);

CREATE INDEX IF NOT EXISTS idx_verification_command_runs_attempt
ON verification_command_runs(attempt_id, ordinal);

-- Immutable evidence metadata for one collected file. Bytes live in the
-- content-addressed artifact store under the recorded digest.
CREATE TABLE IF NOT EXISTS verification_evidence (
  evidence_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  attempt_id TEXT NOT NULL REFERENCES verification_attempts(attempt_id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  bytes_len INTEGER NOT NULL,
  collected_at TEXT NOT NULL,
  UNIQUE(attempt_id, relative_path)
);

CREATE INDEX IF NOT EXISTS idx_verification_evidence_attempt
ON verification_evidence(attempt_id, collected_at);

-- The computed gate for one attempt. A failed gate is expected product
-- evidence, not an infrastructure error, and stays in Verification.
CREATE TABLE IF NOT EXISTS verification_gates (
  gate_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  attempt_id TEXT NOT NULL UNIQUE REFERENCES verification_attempts(attempt_id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN ('pending', 'passed', 'failed')),
  verifier_manifest_json TEXT,
  command_summary_json TEXT,
  computed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Durable intent for the single owned preview comment. Preview mode performs
-- no other tracker or PR mutation.
CREATE TABLE IF NOT EXISTS verification_publication_intents (
  intent_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  attempt_id TEXT NOT NULL REFERENCES verification_attempts(attempt_id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  status TEXT NOT NULL,
  completed_steps_json TEXT NOT NULL,
  retry_count INTEGER NOT NULL DEFAULT 0,
  last_error_json TEXT,
  comment_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_verification_publication_intents_pending
ON verification_publication_intents(status, updated_at);
