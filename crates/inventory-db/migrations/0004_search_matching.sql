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
