# Import matching, review, and commit (`inventory-db::{import_match,import_review,import_commit}`)

Phase 5b's "Match -> Review -> Confirm" half of spec §10, picking up where
`docs/parsers.md` (Phase 5a, "Upload -> Extract") leaves off. A persisted
`ImportRecord`/`ImportLineRecord` (from `parse_and_store_import` /
`store_import`) is read-only all the way through Review; `commit_import` is
the single point where an import actually touches inventory.

```
Upload → Extract        Match            Review              Confirm
(5a)                    (5b, read-only)  (5b, read-only)     (5b, the only mutation)
parse_and_store_import  match_import     build_import_review commit_import
                                                               ↓
                                                          reverse_import (undo)
```

## Match (`import_match.rs`)

`Database::match_import(import_id)` scores every `line_kind = 'part'` line of
an import against parts already on file, one `ImportLineMatch` per line:

- `candidate_from_line(line, supplier)` adapts a persisted `ImportLineRecord`
  into a `MatchCandidate` — supplier, supplier_sku, manufacturer, mpn. A blank
  (empty-after-trim) field is treated as absent, the same "blank means None"
  convention `find_matches` uses internally. `category_id`/`attributes`/
  `package` are left empty: description-based category/attribute inference is
  enrichment (5c) — SKU/MPN/alias matching (the levels a bare import line can
  reach) works without them.
- `Database::match_import_line` hands the candidate to the existing 7-level
  `find_matches` (`ExactSku` → `ExactMpn` → `KnownAlias` → `ExactIdentity` →
  `ProbableEquivalent` → `Similar` → `None`, see `docs/schema.md`) — no
  matching logic is reimplemented here. `ImportLineMatch{matches, top}` keeps
  the full ranked list plus the best (if any).
- Non-`part` lines (`fee`/`tariff`/`no_charge`/`unknown`) are excluded from
  `match_import` entirely — they're never eligible to resolve to a part.
- Entirely read-only: no `parts`/`manufacturer_variants`/`supplier_listings`/
  `transactions` row is ever written here.

## Review (`import_review.rs`)

`Database::build_import_review(import_id)` assembles one `ImportReview` —
purely derived from `match_import` plus the persisted lines, still no
inventory write:

- Every line of the import appears (not just `part` lines), so the review
  shows the whole order.
- **`ProposedAction`** is the sensible default, not a decision: a `part` line
  with a top match proposes `AddStockToExisting{part_id, verdict_kind}`; a
  `part` line with no match proposes `CreateNew`; `fee`/`tariff`/`no_charge`
  propose `NonInventory`; an `unknown`-kind line proposes `Ignore` (the parser
  itself couldn't classify it, so the default is "don't act"). The 5d UI lets
  a user override any of these before confirming.
- **`receive_qty_milli` is always the SHIPPED quantity, never ordered**
  (spec §10: ordered 10 / shipped 8 / backordered 2 → receive 8). It's
  surfaced as a raw `i64` milli count rather than a `Quantity` — an import
  line has no unit of its own (DigiKey invoices carry a bare count, not a
  unit), and guessing one (`each`) would very often be wrong once a line
  resolves to a part with a different `quantity_unit` (e.g. `Feet`, `Grams`).
  Building the unit-correct `Quantity` is deferred to `commit_import`, which
  has the resolved `part_id` and can read that part's real `quantity_unit`
  first.
- A `part` line whose shipped amount is absent-or-zero *and* whose ordered
  amount is positive gets `receive_qty_milli = None` plus the warning "fully
  backordered — nothing to receive". A partial shipment (shipped < ordered
  but shipped > 0) receives the shipped amount with no warning — that's a
  completely normal partial delivery, not an anomaly.
- `duplicate_of` re-runs 5a's `find_duplicate_imports` (matching on
  order/invoice/shipment number or the source file's content hash), filtering
  the import's own id out of the result, so the review can warn "this looks
  like an order you already imported" without ever silently blocking — the
  user can still confirm.

## Confirm (`import_commit.rs`)

`Database::commit_import(import_id, decisions: &[(ImportLineId,
LineDecision)])` is the **only** mutation in the pipeline. `LineDecision` is
the caller's resolved choice per line (5d's UI builds these from a reviewed
`ImportReview`, overriding `ProposedAction` as the user directs):

- `AddStock { part_id }` — receive the shipped quantity against an existing
  part.
- `CreateNew { draft, variant, listing }` — create a brand-new part (+ first
  variant + supplier listing), then receive against it.
- `AddAsVariant { part_id, variant, listing }` — add a second-source
  manufacturer variant + supplier listing to an existing part, then receive
  against it.
- `Skip` — nothing: no part, no variant, no listing, no receive, no price
  history, no alias.

`commit_import` opens **one** `rusqlite` transaction and, inside it:

1. Rejects (before mutating anything) any `AddStock`/`CreateNew`/
   `AddAsVariant` decision keyed to a non-`part` line, with
   `DbError::NonPartLineNotReceivable` — fee/tariff/no_charge/unknown lines
   can never create inventory, enforced here (not only as the review's
   advisory `NonInventory` default) so a caller can't bypass the rule.
   `Skip` is exempt — harmless regardless of the line's kind.
2. Resolves each non-skip decision's `part_id`, creating parts/variants/
   listings through new in-tx helpers (`create_part_in_tx`/
   `add_variant_in_tx`/`add_supplier_listing_in_tx`, extracted from the
   existing public `create_part`/`add_variant`/`add_supplier_listing` so
   their behavior is byte-identical, `docs/architecture.md`).
3. Collects one `Receive` `LedgerOp` per line whose `shipped_milli` is
   present and `> 0` (SHIPPED, never ordered — a zero/absent-shipped line
   creates no receive, though its part is still created at zero stock) and
   applies them ALL via `build_group_in_tx` — the same atomic-group primitive
   `build_from_bom` uses — as one `transaction_groups` row.
4. Inserts a `price_history` row per non-skip line that has a unit price
   (even a backordered line's quoted price is a real price observation), and
   updates the `last_unit_price_micros`/`last_purchase_date` of any listing
   the decision itself just created (`AddStock` never creates a listing, so
   it's left alone rather than guessing which of a matched part's existing
   listings to touch).
5. Records the line's supplier_sku/mpn as `part_aliases` (`INSERT OR IGNORE`,
   source `import`) so a repeat import of the same SKU resolves straight to
   `KnownAlias` next time — an already-known alias is left pointing at
   whichever part first claimed it rather than failing the commit.
6. Sets `imports.status = 'committed'` and `imports.commit_group_id` to the
   receive group's id (migration 0008), then commits the transaction.
   `refresh_search_text` runs per touched part after the commit.

**Atomicity:** any failure before the final commit — a CHECK violation, an
invalid draft, an over-draw — rolls back the *entire* commit. No partial
parts, no partial receives; the import stays `parsed`.

**The zero-receives edge case:** if every decided line is `Skip` or fully
backordered, there's nothing to give `build_group_in_tx`. That's still a
valid commit (a backordered line still gets its part on file), not a
failure: `commit_import` sets `commit_group_id` to `NULL` and returns a
synthetic, never-persisted `GroupRecord` so the return type stays uniform.

## Reverse

`Database::reverse_import(import_id, note)` undoes a `committed` import in
one transaction: `reverse_group` on the commit group (every receive returns
stock to its pre-import level) plus the `imports.status = 'reversed'` flip,
folded together so a failed reversal can never leave the import
half-flipped — the same atomicity pattern `build_from_bom` established.
**Parts created by the commit are not deleted** — they simply return to zero
stock, matching the system's "history is never deleted" rule (the part,
variant, listing, price_history, and alias rows all remain on file). A
receiveless commit (`commit_group_id IS NULL`) has nothing to reverse
group-wise; only the status flips.

## Commands and hooks

`apps/desktop/src-tauri/src/commands.rs` wraps the above as thin typed
commands: `parse_and_store_import` (bytes + filename → detect format → the
matching DigiKey parser → `store_import`), `get_import_review`,
`list_imports`, `list_import_lines`, `commit_import`, `reverse_import`.
`apps/desktop/src/hooks/imports.ts` follows the Phase 3/4 TanStack Query
pattern: `useImports`/`useImportReview`/`useImportLines` (queries),
`useParseImport`/`useCommitImport`/`useReverseImport` (mutations).
`useCommitImport`/`useReverseImport` both invalidate the imports list, the
committed import's review/lines, and the same broad ledger surface
`useReverseGroup` invalidates for any group (per-part stock/transactions,
search, dashboard, recent activity, history) — matching the Phase 4 hook
pattern for anything that returns a `GroupRecord`.

## Using the import UI

Phase 5d built the desktop UI over this exact pipeline — nothing below
changes the Rust-side contract above, it's the human path through it.

1. **Upload** — `/orders` always shows an upload dropzone above the imports
   table (drag-drop or a file picker; PDF, CSV, or XLSX). The backend
   detects the format and only persists an `ImportRecord` once parsing
   succeeds, so a failed upload leaves nothing behind — the error names
   itself and nothing was stored. A successful upload navigates straight to
   that import's review screen (`/orders/$importId`).
2. **Review** — the review screen shows the order summary (financials, line
   count, how many lines will actually receive stock, a backorder count),
   then every line with its default `ProposedAction` already applied as a
   starting decision. **Shipped, never ordered** is what actually receives —
   a partial shipment (backordered > 0 but shipped > 0) is a normal partial
   delivery, not an anomaly; a fully-backordered line receives nothing and
   is flagged. Override any line's decision via "Change…": pick a suggested
   match, search for a different part, add as a second-source variant, spin
   up a brand-new part (with its own bin), or skip the line entirely.
   Non-inventory lines (fee/tariff/no_charge/unknown) render read-only —
   there's nothing to decide, they can never create inventory.
   - **Duplicate warning**: if this order (by order/invoice/shipment number
     or file content hash) matches an import already on file, a prominent
     but non-blocking alert links to the prior import(s) — review and commit
     can still proceed.
3. **Commit** — "Commit import" is disabled while any "Create new part"
   draft is still incomplete (missing a display name or category). Once
   enabled, it applies every line's decision as ONE atomic transaction group
   — new parts/variants/listings, each line's shipped-quantity receive,
   price history, and remembered SKU/MPN aliases — and the screen freezes
   into a read-only view of what was committed. The receive shows up in
   `/inventory` immediately and as a group in History (grouped, reversible
   from either screen).
4. **Reverse** — from either the review screen (once `committed`) or
   History, "Reverse import" undoes the whole receive group as one
   transaction after a confirmation dialog. Parts the commit created are
   never deleted — they return to zero stock and stay on file, same as any
   other reversal.

See `docs/ui.md`'s "Orders & Imports" section for the screen-by-screen
component breakdown (`OrdersList`, `UploadImport`, `ImportReview`,
`ReviewLineTable`, `LineActionEditor`, `CreateFromLineDialog`).

## What's deferred past 5d

- Per-line correction of an already-committed import without reversing the
  whole thing — spec §10 wants this, but building it needs either partially
  un-grouping a commit or a new composing ledger primitive, neither of which
  was a 5b/5d must-have. The documented workaround today is
  `reverse_import` (undo the whole commit) followed by a fresh
  `commit_import` with corrected decisions.
- PDF import needs a real `pdfium.dll`/`libpdfium.so` loaded via the
  `pdfium` Cargo feature (off by default — see `docs/build.md`); CSV and
  XLSX import work in every build today. See `docs/parsers.md` and
  `docs/known-limitations.md`.
