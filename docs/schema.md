# Database schema

Numbered migrations live in `crates/inventory-db/migrations/`. Current version: 7.

## Conventions
- All tables STRICT; IDs are 26-char ULID strings; quantities are INTEGER
  milli-units (x1000); prices are INTEGER micros; timestamps are SQLite
  `datetime('now')` UTC strings.
- Deterministic seed rows use all-zero-prefix ULIDs (Miscellaneous category:
  `00000000000000000000000000`).

## Migration 0001 — settings
`settings(key PK, value)` — inventory-level preferences (backed up).

## Migration 0002 — inventory schema
- `categories` — minimal for 2a (id, name, group_name, built_in); the typed
  attribute system arrives in migration 0003 (Phase 2b).
- `projects` — stub (id, name); Phase 4 extends.
- `parts` — canonical parts. Quantity semantics live on the part
  (`quantity_unit`); `low_stock_threshold_milli`, notes (public/private),
  `usage_behavior`, `archived`, `metadata_complete`.
- `part_tags` — (part_id, tag) rows.
- `manufacturer_variants` — variants per part; at most one preferred per part
  (partial unique index).
- `supplier_listings` — per variant; unique (variant, supplier, sku).
- `part_stock` — aggregates: available/reserved/checked_out + lifetime
  received/consumed, every column `CHECK >= 0`. Updated ONLY inside the same
  SQL transaction as a ledger insert.
- `transaction_groups` + `transactions` — append-only ledger. Types:
  receive, reserve, release_reservation, check_out, return, consume_available,
  consume_reserved, consume_checked_out, adjust_up, adjust_down,
  transfer_reservation, reverse. A row is reversible at most once (partial
  unique index on `reversed_txn_id`); reversal rows carry swapped states and
  reference their original. `bom_item_id`/`import_id` gain FKs in Phases 4/5.
  Member order is preserved via rowid (append-only table; rows are never
  deleted — rowid reuse cannot occur).

## Invariants (three layers)
1. SQL CHECK constraints (negative stock impossible).
2. Domain layer computes every delta from `LedgerOp` (`inventory-core::ledger`).
3. `validate_invariants()` replays the ledger and compares — run at startup
   (quiet), in tests, and before backup/restore (Phase 7).

Reversal rows store swapped from/to states (and swapped project columns for
transfers); group members cannot be reversed individually
(`TransactionInGroup`) — reverse the group.

## Migration 0003 — attributes and dimensions
- `attribute_defs` — typed definitions (8 data types; unit_kind for number_unit/range;
  identity flag feeds duplicate matching). Built-ins seed idempotently at open
  (insert-only; deterministic ids `0000000000000000000000A###`/`C###`).
- `category_attributes` — per-category links with display_order and hidden.
- `attribute_choices` — allowed values for choice/multi_choice.
- `part_attribute_values` — one row per (part, attribute): original text always
  preserved; value_num holds the normalized f64 for filtering; exact identity
  comparison re-parses original_text (see `inventory-core::units`).
- `dimensions` — structured measurements (overall/body/mounting/custom),
  normalized to mm/g, with source provenance; attachment_id FK arrives Phase 3.

## Units engine
`inventory-core::units` parses electronics notation to exact `(mantissa, exp10)`
canonical form: 10k = 10 kΩ = 10000 ohm; 0.1 µF = 100 nF = 100000 pF; 1/4 W =
0.25 W; 3V3 = 3.3 V; 4k7; 0R; inches convert exactly (25.4 mm). Package codes
normalize imperial/metric (0603 = 1608 metric) in `inventory-core::packages`.

## Migration 0004 — search index and matching memory
- `search_text` — one denormalized searchable blob per part (`part_id` PK
  referencing `parts`, `body` TEXT). Rebuilt wholesale by
  `Database::refresh_search_text` from every piece of content the search bar
  can match: display name, category name, description, bin label, tags,
  manufacturer + MPN, supplier SKUs, attribute key + original text +
  formatted value (both bounds for `range` attributes), and dimension names.
  Every content-mutating method (create/update part, archive toggle,
  variant/listing/attribute/dimension/tag writes) calls it, so `search_text`
  is the single choke-point new searchable content must pass through. STRICT.
- `parts_fts` — an FTS5 external-content index over `search_text`
  (`content='search_text', content_rowid='rowid'`), tokenizer `unicode61
  remove_diacritics 2 tokenchars '-_.'` so hyphenated SKUs and part numbers
  (`296-TLV9002IDDFRCT-ND`) stay single tokens instead of splitting on `-`.
  **FTS5 virtual tables cannot be declared STRICT** — a deliberate, documented
  exception to the STRICT-everywhere convention (SQLite rejects `STRICT` on
  `CREATE VIRTUAL TABLE`).
- Three triggers (`search_text_ai`/`_ad`/`_au`) keep `parts_fts` synced with
  `search_text` on every insert/update/delete, using FTS5's external-content
  "special command" row (`INSERT INTO parts_fts(parts_fts, rowid, ...)
  VALUES ('delete', ...)`) to retract the old index entry before an update's
  second statement re-indexes the new body. Covered by a dedicated sync test
  (`fts_stays_in_sync_with_search_text`).
- `part_aliases` — remembers supplier SKUs / MPNs seen during import so a
  repeat import resolves straight to the same part (`alias_kind` CHECK
  `IN ('supplier_sku', 'mpn')`, `alias_value`, `part_id` FK ON DELETE CASCADE,
  `source`). `UNIQUE(alias_kind, alias_value)` — a given SKU/MPN can point at
  exactly one part; a collision surfaces as `DbError::AliasTaken`. Indexed on
  `alias_value`. STRICT.
- `equivalence_decisions` — remembers user judgments about whether two parts
  are ("approved") or aren't ("rejected") the same device, plus a free-text
  `note`. **Canonical pair ordering**: `CHECK (part_a < part_b)` alongside
  `UNIQUE (part_a, part_b)` means a given pair is only ever stored one way —
  callers (`record_equivalence`/`equivalence_between`) always sort the two
  part ids lexicographically before reading or writing, so the UNIQUE
  constraint alone is sufficient to dedupe a pair regardless of which order
  it's presented in. STRICT.

## Migration 0005 — attachments
Content-addressed file attachments (Phase 3 Task 10, spec §9). File bytes live
on disk under the data dir's `attachments/` folder, named by the SHA-256 hex
digest of their content (`<hash>` or `<hash>.<ext>`); the blob itself is never
stored in SQLite, only its metadata.

- `attachments` — one metadata row per distinct blob. `content_hash` TEXT
  **PRIMARY KEY** (the SHA-256 hex digest), `ext` (lowercase, no dot, or NULL),
  `size_bytes`, `kind` (CHECK-constrained: invoice / datasheet / photo /
  measurement_photo / drawing / cad / project_doc / other), `original_name`,
  `source`, `created_at`. Because both the file name and this PK derive purely
  from content, storing identical bytes any number of times yields exactly one
  file and one row — **content-hash deduplication**. The first writer's
  `ext`/`kind`/`original_name`/`source` are canonical; later stores of the same
  bytes return that row unchanged. STRICT.
- `part_attachments` — links a blob to a part. Composite `PRIMARY KEY (part_id,
  content_hash)`; `part_id` FK **ON DELETE CASCADE** (deleting a part drops its
  links) referencing `parts`; `content_hash` FK referencing `attachments`.
  Many parts may share one deduplicated blob and one part may carry many
  attachments. Indexed on `content_hash`. STRICT.
- **Dimension attachments** reuse the `dimensions.attachment_id` column
  migration 0003 created without an FK (deferred until this table existed). It
  now holds a `content_hash`, enforced in the application layer
  (`attachment_store::set_dimension_attachment`) rather than by a DB-level FK:
  SQLite can't add an FK to an existing STRICT table without a full rebuild, and
  a dimension has at most one attachment (its measurement photo), so a 1:1
  column is the natural shape.
- **Blob garbage collection is deliberately out of scope.** Removing a
  `part_attachments`/dimension link never deletes the shared blob or its
  `attachments` row (other parts/dimensions may still reference the same
  content); orphaned-blob GC is deferred to Phase 7.
- **Path safety:** caller-supplied extensions pass `sanitize_ext`, which trims,
  strips a leading dot, lowercases, and requires the result to be empty or match
  `[a-z0-9]{1,16}` — a whitelist that cannot encode `/`, `\`, `..`, or any path
  metacharacter, so no sanitized value can escape the attachments directory.

## Identity signatures and duplicate matching
`inventory-db::identity` builds a part's *identity signature*: one
`IdentityValue` per identity-flagged attribute on its category, re-parsed
into exact comparable form (`ParsedValue` for `number_unit`/`range`,
`normalize_package(...).canonical` for `package`, trimmed original text
otherwise) — never `f64`, so "10k" and "10000 ohm" compare equal without
float-equality traps. `inventory-db::matching` scores candidates/parts
against a 7-level verdict hierarchy (`ExactSku` → `ExactMpn` → `KnownAlias`
→ `ExactIdentity` → `ProbableEquivalent` → `Similar` → `None`); passive
categories (resistors, capacitors, inductors, resistor networks, ferrite
beads, crystals) auto-combine on an exact identity match, every other
category — including all custom ones — caps at `ProbableEquivalent` so
active/IC parts are never silently merged. See `docs/search.md` for the
search query grammar this migration's FTS index serves.

## Migration 0006 — projects and BOMs
Fleshes out the Phase 2a stub `projects` table (id, name, created_at) with a
real lifecycle, and adds the bill-of-materials tables (Phase 4, spec
"Projects and BOMs").

- `projects` gains, via `ALTER TABLE ... ADD COLUMN` (SQLite allows this on an
  existing STRICT table when each new column's CHECK only refers to itself
  and its default satisfies that CHECK, so the stub is extended in place
  rather than rebuilt): `status TEXT NOT NULL DEFAULT 'planned' CHECK IN
  ('planned', 'active', 'completed', 'archived')`, `description TEXT NOT NULL
  DEFAULT ''`, `build_quantity INTEGER NOT NULL DEFAULT 1 CHECK (>= 1)`,
  `repo_link TEXT` (nullable), `notes TEXT NOT NULL DEFAULT ''`,
  `completed_at TEXT` (nullable — stamped by the domain layer when status
  becomes `completed`, cleared when it leaves `completed`).
- `bom_items` — one row per part required to build a project: `id` TEXT PK,
  `project_id` REFERENCES `projects(id)` **ON DELETE CASCADE**, `part_id`
  REFERENCES `parts(id)`, `quantity_per_build_milli INTEGER NOT NULL CHECK
  (> 0)`, `reference_designators TEXT NOT NULL DEFAULT ''`, `required INTEGER
  NOT NULL DEFAULT 1 CHECK IN (0, 1)`, `notes TEXT NOT NULL DEFAULT ''`,
  `created_at`, `UNIQUE (project_id, part_id)` (one BOM line per part per
  project). Indexed on `project_id`. STRICT. The total quantity a build needs
  (`quantity_per_build x projects.build_quantity`) is computed by the domain
  layer on every read rather than stored, so changing `build_quantity`
  doesn't require rewriting every BOM row. `reserved`/`consumed` per line are
  likewise never stored — they are derived by summing `transactions` rows
  attributed to this line (see below and `docs/decisions.md`), the same
  derive-don't-duplicate approach `validate.rs` uses to reconcile
  `part_stock` from the ledger.
- `bom_substitutes` — approved substitute parts for a BOM line: `bom_item_id`
  REFERENCES `bom_items(id)` **ON DELETE CASCADE**, `part_id` REFERENCES
  `parts(id)`, `PRIMARY KEY (bom_item_id, part_id)`. STRICT. Reserve-BOM and
  build-from-BOM may draw from a substitute when the primary part is short,
  per spec.
- `transactions.bom_item_id` (added as a bare, FK-less TEXT column back in
  migration 0002, Phase 2a) intentionally **stays without a database-level
  foreign key**: SQLite cannot add a foreign key to an existing column of an
  already-STRICT table without a full table rebuild, and `transactions` is an
  append-only ledger that only the domain layer ever writes to (nothing
  writes to it directly) — see `docs/decisions.md` for the full reasoning. A
  partial index, `idx_txn_bom_item ON transactions(bom_item_id) WHERE
  bom_item_id IS NOT NULL`, supports the derived reserved/consumed queries.
  In practice, Task 4's `reserve_bom`/`build_from_bom` (`crates/inventory-db/
  src/build.rs`) don't populate `bom_item_id` on the ops they emit — see that
  module's doc comment — so `bom.rs`'s derivation keys on `(project_id,
  part_id)` instead, which this schema's `bom_items.UNIQUE(project_id,
  part_id)` makes exactly equivalent.

## Migration 0007 — imports, price history, matching-memory, and checkouts
Phase 5a (spec §10 import pipeline + §4.2 matching memory). Nothing in this
migration touches `part_stock`/`transactions` — 5a only captures what a
parser saw; matching/enrichment/commit into inventory is 5b/5c. Money is
`*_micros` (i64, x1_000_000, not a float) since DigiKey unit prices carry up
to 5 decimals (`1.82000`) that a float would round; quantities are
`*_milli` (x1000), matching `part_stock`/`transactions` elsewhere in the
schema. See `docs/parsers.md` for the `ParsedInvoice` → row mapping this
schema persists (`inventory-db::imports::store_import`).

- `imports` — one row per parsed supplier order file: `id` TEXT PK,
  `supplier`, `order_number`/`invoice_number`/`shipment_number`/
  `order_date` (all nullable — a parser never fabricates a field the source
  document didn't provide), `currency TEXT NOT NULL DEFAULT 'USD'`,
  `subtotal_micros`/`shipping_micros`/`tax_micros`/`tariff_micros`/
  `total_micros` (nullable INTEGER), `source_format TEXT NOT NULL CHECK
  IN ('pdf', 'csv', 'xlsx')`, `status TEXT NOT NULL DEFAULT 'parsed' CHECK
  IN ('parsed', 'committed', 'reversed')` (5a always writes `'parsed'`;
  5b's commit/reversal flow advances it), `web_order_id`, `notes TEXT
  DEFAULT ''`, `created_at`. Indexed on `order_number` and
  `invoice_number` — the §10 duplicate-import signal
  (`find_duplicate_imports`) matches on these plus the attachment hash
  below. STRICT.
- `import_files` — points at the existing content-addressed `attachments`
  store (migration 0005) rather than storing bytes itself: `id` TEXT PK,
  `import_id` REFERENCES `imports(id)` **ON DELETE CASCADE**,
  `attachment_hash TEXT NOT NULL` (the attachment's SHA-256 hex digest —
  original file bytes are always recoverable, re-parsing is always
  possible), `original_filename TEXT NOT NULL`, `byte_size INTEGER NOT
  NULL`, `created_at`. A separate table (not a column on `imports`) because
  one import may, in principle, have more than one source file. Indexed on
  `import_id` and `attachment_hash`. STRICT.
- `import_lines` — one row per parsed line item: `id` TEXT PK, `import_id`
  REFERENCES `imports(id)` **ON DELETE CASCADE**, `line_number` (nullable
  INTEGER), `supplier_sku`/`mpn`/`manufacturer`/`description` (nullable
  TEXT), `ordered_milli`/`shipped_milli`/`backordered_milli` (nullable
  INTEGER — all three quantities are recorded; the shipped-vs-ordered
  *decision* for stock is applied at commit time in 5b, not here),
  `unit_price_micros`/`extended_price_micros` (nullable INTEGER),
  `packaging`/`customer_reference` (nullable TEXT), `raw_json TEXT NOT
  NULL` (the parser's full original extracted fields for this line — CSV/
  XLSX cell text keyed by header, or PDF extracted-text-per-field — so
  review/debugging can always see exactly what the parser saw, independent
  of how well the typed columns above captured it), `line_kind TEXT NOT
  NULL DEFAULT 'part' CHECK IN ('part', 'fee', 'tariff', 'no_charge',
  'unknown')`, `parse_confidence REAL NOT NULL DEFAULT 1.0`, `created_at`.
  Indexed on `import_id`. STRICT.
- `price_history` — one row per observed purchase price point: `id` TEXT
  PK, `part_id` REFERENCES `parts(id)` (nullable — populated once matching
  resolves a line to a part, 5b), `supplier`/`supplier_sku` (nullable
  TEXT), `unit_price_micros INTEGER NOT NULL`, `currency TEXT NOT NULL`,
  `quantity_milli` (nullable INTEGER), `import_id` REFERENCES `imports(id)`
  **ON DELETE SET NULL** (a price observation remains historically true
  even if the import that produced it is later reversed/deleted — unlike
  `import_files`/`import_lines`, which cascade, losing the `import_id`
  attribution here does not invalidate the row), `purchased_at`
  (nullable), `created_at`. Populated at commit time (5b); the schema is
  added now so it's complete. Indexed on `part_id`. STRICT.
- `equivalence_families` + `equivalence_family_members` — completes the
  §4.2 matching-memory trio started in Phase 2c (`part_aliases`,
  `equivalence_decisions` — not recreated here), for groups of parts a
  human has judged interchangeable-enough to remember (distinct from the
  pairwise `equivalence_decisions`). `equivalence_families(id TEXT PK,
  name, note, created_at)`; `equivalence_family_members(family_id TEXT NOT
  NULL REFERENCES equivalence_families(id) ON DELETE CASCADE, part_id TEXT
  NOT NULL REFERENCES parts(id), PRIMARY KEY (family_id, part_id))`. Both
  STRICT.
- `project_checkouts` — the §4.2 stub table for parts checked out against a
  project outside the normal build-consumption flow: `id` TEXT PK,
  `project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE`,
  `part_id TEXT NOT NULL REFERENCES parts(id)`, `quantity_milli INTEGER NOT
  NULL CHECK (> 0)`, `checked_out_at`, `note TEXT DEFAULT ''`. Wiring this
  into build/checkout commands is deferred past 5a. STRICT.
