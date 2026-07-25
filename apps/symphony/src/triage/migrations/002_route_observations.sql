-- Post-publication route observations.
--
-- The primary key makes correction measurement idempotent: the reconciler runs
-- on every poll, but a given artifact records a given corrected route (or a
-- given inconsistent label set) exactly once, so repeated polling cannot
-- inflate the correction count.
CREATE TABLE IF NOT EXISTS route_observations (
  artifact_id TEXT NOT NULL REFERENCES triage_artifacts(artifact_id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  value TEXT NOT NULL,
  observed_at TEXT NOT NULL,
  PRIMARY KEY (artifact_id, kind, value)
);
