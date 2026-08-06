-- A4 review-cycle identity correction (#617): review artifacts, finding
-- records, and retry budgets are keyed by (run_id, reviewed_head_sha,
-- base_sha) so a base-only change opens a fresh A4 cycle. Data, findings, and
-- publication references are preserved; only uniqueness constraints and
-- indexes are rebuilt. Runs with foreign_keys OFF because the rebuild drops
-- tables that review_publication_intents references. The whole rebuild is one
-- transaction: a mid-batch failure rolls back to the pre-009 schema so the
-- next startup retries the migration instead of skipping it half-applied.

PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

-- Rebuild review_findings_artifacts with the head/base identity.
CREATE TABLE IF NOT EXISTS review_findings_artifacts_new (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE RESTRICT,
  stage_run_id TEXT NOT NULL UNIQUE REFERENCES stage_runs(stage_run_id) ON DELETE RESTRICT,
  attempt_id TEXT NOT NULL UNIQUE REFERENCES review_attempts(attempt_id) ON DELETE RESTRICT,
  draft_pr_artifact_id TEXT NOT NULL REFERENCES implementation_draft_pr_artifacts(artifact_id),
  implementation_artifact_id TEXT NOT NULL REFERENCES implementation_artifacts(artifact_id),
  spec_artifact_id TEXT NOT NULL REFERENCES spec_artifacts(artifact_id),
  schema_version INTEGER NOT NULL,
  reviewed_head_sha TEXT NOT NULL,
  base_sha TEXT NOT NULL,
  manifest_json TEXT NOT NULL,
  no_findings INTEGER NOT NULL CHECK (no_findings IN (0, 1)),
  finding_count INTEGER NOT NULL,
  received_at TEXT NOT NULL,
  bytes_len INTEGER NOT NULL,
  UNIQUE(run_id, reviewed_head_sha, base_sha)
);

INSERT INTO review_findings_artifacts_new
SELECT artifact_id, run_id, stage_run_id, attempt_id,
       draft_pr_artifact_id, implementation_artifact_id, spec_artifact_id,
       schema_version, reviewed_head_sha, base_sha, manifest_json,
       no_findings, finding_count, received_at, bytes_len
FROM review_findings_artifacts;

DROP TABLE review_findings_artifacts;
ALTER TABLE review_findings_artifacts_new RENAME TO review_findings_artifacts;

CREATE INDEX IF NOT EXISTS idx_review_findings_artifacts_run
ON review_findings_artifacts(run_id, received_at DESC);

CREATE TRIGGER IF NOT EXISTS review_findings_artifacts_immutable_update
BEFORE UPDATE ON review_findings_artifacts
BEGIN
  SELECT RAISE(ABORT, 'review findings artifacts are immutable');
END;

CREATE TRIGGER IF NOT EXISTS review_findings_artifacts_immutable_delete
BEFORE DELETE ON review_findings_artifacts
BEGIN
  SELECT RAISE(ABORT, 'review findings artifacts are immutable');
END;

-- Rebuild review_finding_records with the head/base identity. base_sha is
-- backfilled from the owning findings artifact.
CREATE TABLE IF NOT EXISTS review_finding_records_new (
  finding_record_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  artifact_id TEXT NOT NULL REFERENCES review_findings_artifacts(artifact_id) ON DELETE CASCADE,
  finding_id TEXT NOT NULL,
  identity_key TEXT NOT NULL,
  reviewed_head_sha TEXT NOT NULL,
  base_sha TEXT NOT NULL,
  severity TEXT NOT NULL CHECK (severity IN ('blocking', 'major', 'minor', 'nit')),
  category TEXT NOT NULL CHECK (
    category IN (
      'correctness',
      'security',
      'spec-conformance',
      'test-coverage',
      'maintainability'
    )
  ),
  path TEXT NOT NULL,
  line INTEGER NOT NULL,
  end_line INTEGER,
  claim TEXT NOT NULL,
  rationale TEXT NOT NULL,
  remediation TEXT NOT NULL,
  acceptance_criterion TEXT,
  confidence REAL NOT NULL CHECK (confidence BETWEEN 0.0 AND 1.0),
  lifecycle_state TEXT NOT NULL DEFAULT 'new'
    CHECK (lifecycle_state IN ('new', 'persisting', 'resolved')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(artifact_id, finding_id),
  UNIQUE(run_id, reviewed_head_sha, base_sha, identity_key)
);

INSERT INTO review_finding_records_new
SELECT r.finding_record_id, r.run_id, r.artifact_id, r.finding_id, r.identity_key,
       r.reviewed_head_sha, a.base_sha, r.severity, r.category, r.path, r.line,
       r.end_line, r.claim, r.rationale, r.remediation, r.acceptance_criterion,
       r.confidence, r.lifecycle_state, r.created_at, r.updated_at
FROM review_finding_records r
JOIN review_findings_artifacts a ON a.artifact_id = r.artifact_id;

DROP TABLE review_finding_records;
ALTER TABLE review_finding_records_new RENAME TO review_finding_records;

CREATE INDEX IF NOT EXISTS idx_review_finding_records_run_head
ON review_finding_records(run_id, reviewed_head_sha, base_sha, lifecycle_state);

-- Review attempt retry budgets are consumed per head/base pair.
DROP INDEX IF EXISTS idx_review_attempts_run_head;
CREATE INDEX IF NOT EXISTS idx_review_attempts_run_head
ON review_attempts(run_id, reviewed_head_sha, base_sha, created_at DESC);

COMMIT;

PRAGMA foreign_keys = ON;
