-- Phase 5c Task 2: per-field provenance (spec §5/§11).
--
-- Enrichment (Task 3+) records WHERE each field's current value came from,
-- one row per (part, field) tracking only the CURRENT source — this is not
-- a history log, it's an upsert-on-reapply table the compare-and-apply
-- (Task 5) reads before proposing a change: a field whose recorded source
-- is 'manual' (a human typed it in deliberately) is never auto-overwritten;
-- it's surfaced as a proposed change requiring explicit approval instead.
--
-- `field_key` is a stable, provider-agnostic string identifying the
-- enriched field (e.g. `variant.datasheet_url`, `variant.lifecycle`,
-- `attr.<attribute key>`, `description`, `category`) — not a DB column
-- name, since a field may live on `manufacturer_variants`, on
-- `part_attribute_values`, or directly on `parts`.
--
-- `parts.metadata_complete` already exists (migration 0002); enrichment
-- sets it once enough fields are populated. No new completeness column here.
CREATE TABLE field_provenance (
    id         TEXT PRIMARY KEY,
    part_id    TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    field_key  TEXT NOT NULL,
    source     TEXT NOT NULL
        CHECK (source IN ('invoice', 'digikey', 'manufacturer', 'datasheet', 'inferred', 'manual', 'measured')),
    confidence REAL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (part_id, field_key)
) STRICT;
CREATE INDEX idx_field_provenance_part ON field_provenance(part_id);
