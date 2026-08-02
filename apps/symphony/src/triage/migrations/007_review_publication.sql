-- A4 PR2: durable formal review identity and routing projection.
-- Additive only; existing preview publication rows remain valid.

ALTER TABLE review_publication_intents ADD COLUMN review_id TEXT;
ALTER TABLE review_publication_intents ADD COLUMN review_url TEXT;
ALTER TABLE review_publication_intents ADD COLUMN route_state TEXT;
