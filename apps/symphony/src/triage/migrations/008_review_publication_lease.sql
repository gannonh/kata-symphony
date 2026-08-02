-- A4 PR2: durable cross-process publication lease.
-- Additive only; lease ownership expires after a coordinator crash.

ALTER TABLE review_publication_intents ADD COLUMN lease_owner TEXT;
ALTER TABLE review_publication_intents ADD COLUMN lease_expires_at TEXT;
