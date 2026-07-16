-- Full-text search index and matching-memory schema (Phase 2c).
--
-- search_text holds one denormalized searchable blob per part; parts_fts is
-- an FTS5 external-content index over it kept in sync by triggers. part_aliases
-- remembers supplier SKUs / MPNs seen during import so repeat imports resolve
-- to the same part. equivalence_decisions remembers user judgments ("these
-- two parts are/aren't the same") so matching never re-asks about a pair it
-- has already been told about.

CREATE TABLE search_text (
    part_id TEXT PRIMARY KEY REFERENCES parts(id) ON DELETE CASCADE,
    body    TEXT NOT NULL
) STRICT;

-- FTS5 virtual tables cannot be STRICT (documented deviation).
CREATE VIRTUAL TABLE parts_fts USING fts5(
    part_id UNINDEXED,
    body,
    content='search_text',
    content_rowid='rowid',
    tokenize="unicode61 remove_diacritics 2 tokenchars '-_.'"
);

CREATE TRIGGER search_text_ai AFTER INSERT ON search_text BEGIN
    INSERT INTO parts_fts(rowid, part_id, body) VALUES (new.rowid, new.part_id, new.body);
END;
CREATE TRIGGER search_text_ad AFTER DELETE ON search_text BEGIN
    INSERT INTO parts_fts(parts_fts, rowid, part_id, body) VALUES ('delete', old.rowid, old.part_id, old.body);
END;
CREATE TRIGGER search_text_au AFTER UPDATE ON search_text BEGIN
    INSERT INTO parts_fts(parts_fts, rowid, part_id, body) VALUES ('delete', old.rowid, old.part_id, old.body);
    INSERT INTO parts_fts(rowid, part_id, body) VALUES (new.rowid, new.part_id, new.body);
END;

CREATE TABLE part_aliases (
    id          TEXT PRIMARY KEY,
    alias_kind  TEXT NOT NULL CHECK (alias_kind IN ('supplier_sku', 'mpn')),
    alias_value TEXT NOT NULL,
    part_id     TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    source      TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (alias_kind, alias_value)
) STRICT;
CREATE INDEX idx_aliases_value ON part_aliases(alias_value);

CREATE TABLE equivalence_decisions (
    id         TEXT PRIMARY KEY,
    part_a     TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    part_b     TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    decision   TEXT NOT NULL CHECK (decision IN ('approved', 'rejected')),
    note       TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- Canonical pair ordering: the lexicographically smaller part id is
    -- always stored in part_a, so a pair can only ever be represented one
    -- way and UNIQUE(part_a, part_b) is sufficient to dedupe it.
    UNIQUE (part_a, part_b),
    CHECK (part_a < part_b)
) STRICT;

-- Seeding is insert-only (see seed.rs), so a database that already ran the
-- Phase 2b seed with vgs_threshold's old (non-identity) flag would never
-- pick up the later flip to identity=1 in the current seed data. Applying
-- it here, in migration 0004, brings every upgraded database in line with
-- the current seed for this one built-in attribute. Safe: no production
-- database has run 0004 yet, so this is still pre-release housekeeping,
-- not a live-data migration.
UPDATE attribute_defs SET identity = 1 WHERE key = 'vgs_threshold' AND built_in = 1;
