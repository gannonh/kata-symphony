-- A3 PR2: immutable draft-PR artifacts linked to implementation artifacts.
-- Additive only; publication intents and A1/A2 tables are unchanged.

CREATE TABLE IF NOT EXISTS implementation_draft_pr_artifacts (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES factory_runs(run_id) ON DELETE CASCADE,
  implementation_artifact_id TEXT NOT NULL UNIQUE REFERENCES implementation_artifacts(artifact_id) ON DELETE CASCADE,
  intent_id TEXT NOT NULL REFERENCES implementation_publication_intents(intent_id) ON DELETE CASCADE,
  number INTEGER NOT NULL,
  url TEXT NOT NULL,
  draft INTEGER NOT NULL,
  head TEXT NOT NULL,
  base TEXT NOT NULL,
  head_sha TEXT NOT NULL,
  marker TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_implementation_draft_pr_run
ON implementation_draft_pr_artifacts(run_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_implementation_draft_pr_intent
ON implementation_draft_pr_artifacts(intent_id);
