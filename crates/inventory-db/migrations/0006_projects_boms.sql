-- Phase 4 Task 1: project fields + BOM schema (spec "Projects and BOMs").
--
-- Fleshes out the Phase 2a stub `projects` table (id, name, created_at) with
-- a real lifecycle: status, description, build_quantity, repo_link, notes,
-- and completed_at (stamped by the domain layer when status becomes
-- 'completed'). SQLite can add columns with per-column CHECK constraints and
-- constant defaults to an existing STRICT table via
-- `ALTER TABLE ... ADD COLUMN` as long as the CHECK only refers to the new
-- column and the default satisfies it, so the stub is extended in place
-- rather than rebuilt. The stub `create_project` (crates/inventory-db/src/
-- ledger.rs) only ever inserted `(id, name)`; existing and future rows using
-- that path pick up the defaults below and remain valid.
ALTER TABLE projects ADD COLUMN status TEXT NOT NULL DEFAULT 'planned'
    CHECK (status IN ('planned', 'active', 'completed', 'archived'));
ALTER TABLE projects ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN build_quantity INTEGER NOT NULL DEFAULT 1
    CHECK (build_quantity >= 1);
ALTER TABLE projects ADD COLUMN repo_link TEXT;
ALTER TABLE projects ADD COLUMN notes TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN completed_at TEXT;

-- A project's bill of materials: one row per part required to build it.
-- `quantity_per_build_milli` is the per-build quantity in milli-units (x1000,
-- matching part_stock/transactions). The total required for the project
-- (quantity_per_build x projects.build_quantity) is computed by the domain
-- layer rather than stored, so a build_quantity change doesn't require
-- rewriting every BOM row. `reserved`/`consumed` per line are likewise never
-- stored here — they are derived by summing `transactions` rows carrying
-- this row's id in `bom_item_id` (see the index below), the same
-- derive-don't-duplicate approach `validate.rs` already uses to reconcile
-- `part_stock` from the ledger. This avoids a second source of truth that
-- could drift from the append-only ledger.
CREATE TABLE bom_items (
    id                       TEXT PRIMARY KEY,
    project_id               TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    part_id                  TEXT NOT NULL REFERENCES parts(id),
    quantity_per_build_milli INTEGER NOT NULL CHECK (quantity_per_build_milli > 0),
    reference_designators    TEXT NOT NULL DEFAULT '',
    required                 INTEGER NOT NULL DEFAULT 1 CHECK (required IN (0, 1)),
    notes                    TEXT NOT NULL DEFAULT '',
    created_at               TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, part_id)
) STRICT;
CREATE INDEX idx_bom_items_project ON bom_items(project_id);

-- Approved substitute parts for a BOM line: reserve-BOM and build-from-BOM
-- may draw from a substitute when the primary part is short of what's
-- needed, per spec.
CREATE TABLE bom_substitutes (
    bom_item_id TEXT NOT NULL REFERENCES bom_items(id) ON DELETE CASCADE,
    part_id     TEXT NOT NULL REFERENCES parts(id),
    PRIMARY KEY (bom_item_id, part_id)
) STRICT;

-- transactions.bom_item_id (added as a bare, FK-less column in migration
-- 0002, Phase 2a) intentionally stays without a database-level foreign key:
-- SQLite cannot add a foreign key to an existing column of an already-STRICT
-- table without a full table rebuild, and transactions is an append-only
-- ledger the domain layer (bom.rs / build.rs, Phase 4 Tasks 3-4) always
-- populates from a real bom_items.id — a DB-level FK would only ever catch a
-- domain bug, never bad user input, since nothing writes to `transactions`
-- directly. Indexed here (partial, only non-NULL rows) for the derived
-- reserved/consumed queries above, which filter by bom_item_id.
CREATE INDEX idx_txn_bom_item ON transactions(bom_item_id) WHERE bom_item_id IS NOT NULL;
