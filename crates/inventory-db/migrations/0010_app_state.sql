-- Phase 6 Task 1: internal app state (spec §13 publish config's runtime
-- state — NOT the public-publish config itself, which lives in the existing
-- `settings` table alongside every other user preference; this table is for
-- values the app itself derives and updates, not values a user edits).
--
-- Keys used starting this phase:
--   last_published_digest — the `content_digest` of the most recently
--     published snapshot (sha256 hex); publish is skipped when the current
--     snapshot's digest matches this, so an unchanged inventory never
--     produces a new commit.
--   pending_publish        — present ('1') when a publish attempt failed and
--     needs a retry (close-time flow, startup quiet retry); absent
--     otherwise. A key/value store models "absent" as no row rather than a
--     boolean column, so `clear_app_state` (a plain DELETE) is the natural
--     way to un-set it.
--   last_published_at      — the `published_at` timestamp actually written
--     into the last successfully published snapshot.
--
-- Deliberately generic (key/value, not one typed column per concern) since
-- Phase 7's backup flow needs its own pending/last-success markers with the
-- same shape — this table is the shared home for both rather than a
-- publish-specific one.
CREATE TABLE app_state (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
