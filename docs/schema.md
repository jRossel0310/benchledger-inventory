# Database schema

Numbered migrations live in `crates/inventory-db/migrations/`. Current version: 2.

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
