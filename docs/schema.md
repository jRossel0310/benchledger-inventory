# Database schema

Numbered migrations live in `crates/inventory-db/migrations/`. Current version: 4.

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
