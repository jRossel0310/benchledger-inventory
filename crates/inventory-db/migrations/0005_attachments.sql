-- Content-addressed attachments (Phase 3 Task 10, spec §9).
--
-- File bytes live on disk under the data dir's `attachments/` folder, named by
-- the SHA-256 hex digest of their content (`<hash>` or `<hash>.<ext>` when an
-- extension is known). Because the file name is derived purely from content,
-- storing identical bytes twice yields exactly one file and — via this table's
-- `content_hash` PRIMARY KEY — exactly one metadata row (deduplication). The
-- blob itself is never stored in SQLite; only its metadata is.
--
-- `part_attachments` links a stored blob to a part: many parts may share one
-- deduplicated blob, and one part may carry many attachments. Removing a link
-- never deletes the shared blob or its `attachments` row — other parts or
-- dimensions may still reference the same content — so blob garbage collection
-- is deliberately out of scope here.
--
-- Dimension attachments reuse the `dimensions.attachment_id` column that
-- migration 0003 created without a foreign key (deferred until this table
-- existed). It now holds a `content_hash` referencing `attachments`. It stays a
-- plain column rather than gaining a DB-level FK for two reasons: SQLite cannot
-- add a foreign key to an existing STRICT table without a full table rebuild,
-- and a dimension has at most one attachment (its measurement photo), so a 1:1
-- column is the natural shape and a `dimension_attachments` link table would be
-- overkill. The relationship is enforced in the application layer
-- (`attachment_store::set_dimension_attachment`), the same way the column was
-- originally introduced deferred.

CREATE TABLE attachments (
    content_hash  TEXT PRIMARY KEY,           -- SHA-256 hex digest of the bytes
    ext           TEXT,                        -- lowercase extension w/o dot, or NULL
    size_bytes    INTEGER NOT NULL,
    kind          TEXT NOT NULL DEFAULT 'other' CHECK (kind IN
        ('invoice', 'datasheet', 'photo', 'measurement_photo',
         'drawing', 'cad', 'project_doc', 'other')),
    original_name TEXT,
    source        TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE TABLE part_attachments (
    part_id      TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL REFERENCES attachments(content_hash),
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (part_id, content_hash)
) STRICT;
CREATE INDEX idx_part_attachments_hash ON part_attachments(content_hash);
