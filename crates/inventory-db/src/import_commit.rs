//! Atomic import commit + reversal (Phase 5b Task 4, spec §10 "Confirm").
//! This is the ONLY mutation in the Match -> Review -> Commit pipeline:
//! everything upstream (`import_match.rs`, `import_review.rs`) is read-only.
//!
//! `commit_import` takes the caller's resolved per-line [`LineDecision`]s (the
//! 5d UI builds these from a reviewed [`crate::import_review::ImportReview`];
//! 5b's own tests build them directly) and applies ALL of them -- new parts/
//! variants/listings, every line's SHIPPED-quantity receive, price history,
//! and remembered SKU/MPN aliases -- inside ONE `rusqlite` transaction. Any
//! failure before the final `tx.commit()` rolls back everything: no partial
//! parts, no partial receives, the import stays `parsed`. This reuses every
//! tested atomic primitive Phase 2-4 already built rather than inventing a
//! new one:
//!
//! - `create_part_in_tx`/`add_variant_in_tx`/`add_supplier_listing_in_tx`
//!   (Task 1, `parts.rs`) for the in-tx creation half.
//! - `build_group_in_tx` (Phase 4, `ledger.rs`) for the receive half -- every
//!   non-skip, positively-shipped line's `Receive` op lands in ONE
//!   `transaction_groups` row, exactly like `build_from_bom`'s BOM-consumption
//!   group.
//!
//! `reverse_import` mirrors that: `reverse_group_in_tx` (extracted from
//! `reverse_group` the same way `build_group_in_tx` was extracted from
//! `apply_group`) plus the `imports.status='reversed'` flip run in the SAME
//! transaction, so the group reversal and the status flip commit or roll back
//! together -- the exact `build_from_bom` atomicity pattern the plan calls
//! for.
//!
//! ## Only SHIPPED quantity ever receives
//!
//! `receive_milli` is always the line's `shipped_milli`, NEVER `ordered_milli`
//! (spec §10: ordered 10 / shipped 8 / backordered 2 -> receive 8). A line
//! whose `shipped_milli` is `None` or `<= 0` creates NO receive op -- its
//! part/variant/listing (for `CreateNew`/`AddAsVariant`) is still created, at
//! zero stock, so a fully-backordered line's part exists ready to receive the
//! rest on a later shipment/import.
//!
//! ## Only `line_kind = 'part'` lines may create inventory
//!
//! `list_import_lines` returns every line of an import regardless of kind --
//! `fee`/`tariff`/`no_charge`/`unknown` lines included. Spec §10's hard rule
//! is that those NEVER create a part or a receive, so before anything is
//! mutated `commit_import` rejects any `AddStock`/`CreateNew`/`AddAsVariant`
//! decision keyed to a non-`part` line's `ImportLineId` with
//! `DbError::NonPartLineNotReceivable`. `Skip` is exempt from this check --
//! it produces nothing regardless of the line's kind, so keying a `Skip` to
//! a fee/tariff/etc. line is harmless and allowed.
//!
//! ## The zero-receives edge case
//!
//! If every decided line is `Skip` or fully-backordered, there is nothing to
//! give `build_group_in_tx` -- and unlike `apply_group`'s public entry point,
//! nothing here should surface `DbError::EmptyGroup` as a *failure*, because
//! creating parts/variants/listings with no receives is a completely valid
//! commit (spec: a backordered line still gets its part on file). So
//! `commit_import` simply skips the group step when `receive_ops` is empty,
//! sets `imports.commit_group_id = NULL`, and returns a **synthetic**
//! `GroupRecord` (a fresh, never-persisted `GroupId`, empty `transactions`)
//! so the return type stays `GroupRecord` for every caller (Task 5's command
//! wrapper needs one concrete success shape). Nothing in the database ever
//! claims this synthetic id exists -- `get_group` on it returns `None`, which
//! is consistent with there being no real group to look up. Symmetrically,
//! `reverse_import` on a receiveless commit (`commit_group_id IS NULL`) has
//! nothing to reverse group-wise: it only flips `status='reversed'` and
//! returns the same kind of synthetic `GroupRecord`.
//!
//! ## Aliases tolerate duplicates
//!
//! `part_aliases` has `UNIQUE(alias_kind, alias_value)`. A repeat import (or
//! two lines that happen to share a SKU/MPN) must not fail the whole commit
//! over a duplicate alias, so alias inserts here use `INSERT OR IGNORE`
//! rather than the public `add_alias`'s hard `AliasTaken` error: an
//! already-known alias is left pointing at whichever part first claimed it,
//! which is fine -- the whole point (spec §10) is that a *repeat* import of
//! the same SKU resolves to `KnownAlias` next time, not that this insert be
//! authoritative.
//!
//! ## Reversal never deletes parts
//!
//! `reverse_import` only reverses the receive transactions (stock returns to
//! its pre-import level) and flips the import's status. A part created by
//! `CreateNew`/`AddAsVariant` during the commit is NOT deleted by reversal --
//! it simply ends up at zero stock, matching the rest of this system's
//! "history is never deleted" rule (the part, variant, and listing rows, plus
//! the price_history rows and aliases, all remain on file).
//!
//! // Phase 5d: single-line correction (spec §10 -- fixing one mismatched
//! // line of an already-committed import without reversing the whole thing)
//! // is deliberately NOT built here. The must-have for 5b is the atomic
//! // whole-import commit + whole-group reversal above; a per-line corrector
//! // would need to reverse just that line's `Receive` (an individual
//! // grouped transaction cannot be reversed alone -- see
//! // `DbError::TransactionInGroup` in `ledger.rs`) and re-receive against the
//! // corrected part, which either means partially un-grouping a commit or
//! // building a new composing primitive neither of which this task's scope
//! // calls for. Until then, the documented workaround is `reverse_import`
//! // (undo the whole commit) followed by a fresh `commit_import` with
//! // corrected decisions.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, Transaction};

use inventory_core::ids::{GroupId, ImportId, ImportLineId, ListingId, PartId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};

use crate::imports::ImportLineRecord;
use crate::ledger::{build_group_in_tx, reverse_group_in_tx, GroupRecord};
use crate::parts::{
    add_supplier_listing_in_tx, add_variant_in_tx, create_part_in_tx, ListingDraft, PartDraft,
    VariantDraft,
};
use crate::{Database, DbError};

/// The caller's resolved action for one import line. The 5d UI builds these
/// from a reviewed [`crate::import_review::ImportReviewLine`] (overriding the
/// derived `ProposedAction` as the user directs); 5b's own tests construct
/// them directly against a persisted import's lines.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LineDecision {
    /// Receive this line's shipped quantity against an existing part.
    AddStock { part_id: PartId },
    /// Create a brand-new part (+ its first variant + supplier listing), then
    /// receive the shipped quantity against it.
    CreateNew {
        draft: PartDraft,
        variant: VariantDraft,
        listing: ListingDraft,
    },
    /// Add a new manufacturer variant + supplier listing to an EXISTING part
    /// (a second-source SKU for a part already on file), then receive the
    /// shipped quantity against that part.
    AddAsVariant {
        part_id: PartId,
        variant: VariantDraft,
        listing: ListingDraft,
    },
    /// Do nothing for this line: no part, no variant, no listing, no receive,
    /// no price history, no alias.
    Skip,
}

/// What one non-skip decision resolved to, threaded from the creation step
/// through to the receive/price-history/alias step below.
struct Resolved {
    part_id: PartId,
    unit: QuantityUnit,
    /// The listing this decision created (`CreateNew`/`AddAsVariant`), whose
    /// `last_unit_price_micros`/`last_purchase_date` should be refreshed from
    /// this line's price. `AddStock` never creates a listing, so this stays
    /// `None` for it -- see the module doc comment ("keep it simple") on why
    /// an `AddStock` line does not attempt to guess which existing listing to
    /// update.
    listing_id: Option<ListingId>,
}

impl Database {
    /// Apply every decision for `import_id` as ONE atomic transaction. See
    /// the module doc comment for the full step-by-step and the zero-receives
    /// edge case. Returns the receive group (or a synthetic empty one -- see
    /// above) on success; any error rolls back the entire transaction and the
    /// import stays `parsed`.
    pub fn commit_import(
        &mut self,
        import_id: &ImportId,
        decisions: &[(ImportLineId, LineDecision)],
    ) -> Result<GroupRecord, DbError> {
        let import = self.get_import(import_id)?.ok_or(DbError::ImportNotFound)?;
        if import.status != "parsed" {
            return Err(DbError::ImportNotCommittable);
        }

        let line_map: HashMap<ImportLineId, ImportLineRecord> = self
            .list_import_lines(import_id)?
            .into_iter()
            .map(|l| (l.id.clone(), l))
            .collect();

        // Reject any inventory-creating decision keyed to a non-`part` line
        // BEFORE anything is mutated (fail-fast, nothing to roll back): spec
        // §10's hard rule is that Fee/Tariff/NoCharge/Unknown lines NEVER
        // create a part or a receive, so `AddStock`/`CreateNew`/
        // `AddAsVariant` may only resolve against a `line_kind = 'part'`
        // line. `Skip` is exempt -- it does nothing either way regardless of
        // the line's kind, so keying a `Skip` to a non-part line is
        // harmless and allowed.
        for (line_id, decision) in decisions {
            if matches!(decision, LineDecision::Skip) {
                continue;
            }
            let line = line_map.get(line_id).ok_or(DbError::ImportLineNotFound)?;
            if line.line_kind != "part" {
                return Err(DbError::NonPartLineNotReceivable {
                    line_kind: line.line_kind.clone(),
                });
            }
        }

        // Human-readable order reference for receive notes -- prefer the
        // order number, then invoice number, falling back to the import id
        // itself so a note is never blank.
        let order_ref = import
            .order_number
            .clone()
            .or_else(|| import.invoice_number.clone())
            .unwrap_or_else(|| import_id.as_str().to_string());

        let tx = self.conn_mut().transaction()?;

        let mut receive_ops: Vec<LedgerOp> = Vec::new();
        let mut touched_parts: Vec<PartId> = Vec::new();

        for (line_id, decision) in decisions {
            let line = line_map
                .get(line_id)
                .ok_or(DbError::ImportLineNotFound)?
                .clone();

            let resolved = match decision {
                LineDecision::Skip => continue,
                LineDecision::AddStock { part_id } => {
                    let unit = part_unit_in_tx(&tx, part_id)?;
                    Resolved {
                        part_id: part_id.clone(),
                        unit,
                        listing_id: None,
                    }
                }
                LineDecision::CreateNew {
                    draft,
                    variant,
                    listing,
                } => {
                    let new_part_id = create_part_in_tx(&tx, draft)?;
                    let variant_id = add_variant_in_tx(&tx, &new_part_id, variant)?;
                    let listing_id = add_supplier_listing_in_tx(&tx, &variant_id, listing)?;
                    Resolved {
                        part_id: new_part_id,
                        unit: draft.quantity_unit,
                        listing_id: Some(listing_id),
                    }
                }
                LineDecision::AddAsVariant {
                    part_id,
                    variant,
                    listing,
                } => {
                    let unit = part_unit_in_tx(&tx, part_id)?;
                    let variant_id = add_variant_in_tx(&tx, part_id, variant)?;
                    let listing_id = add_supplier_listing_in_tx(&tx, &variant_id, listing)?;
                    Resolved {
                        part_id: part_id.clone(),
                        unit,
                        listing_id: Some(listing_id),
                    }
                }
            };

            touched_parts.push(resolved.part_id.clone());

            // Receive: SHIPPED only, never ordered; skip zero/absent shipped
            // (fully backordered -- the part/variant/listing above is still
            // created, just at zero stock).
            if let Some(shipped) = line.shipped_milli {
                if shipped > 0 {
                    let quantity = Quantity::from_milli(shipped, resolved.unit)?;
                    receive_ops.push(LedgerOp::Receive {
                        part_id: resolved.part_id.clone(),
                        quantity,
                        note: format!("import {order_ref}"),
                    });
                }
            }

            // price_history: one row per non-skip line that has a unit price,
            // regardless of whether it received anything (a backordered
            // line's quoted price is still a real price observation).
            if let Some(unit_price_micros) = line.unit_price_micros {
                let ph_id = inventory_core::id::new_id();
                tx.execute(
                    "INSERT INTO price_history (id, part_id, supplier, supplier_sku,
                        unit_price_micros, currency, quantity_milli, import_id, purchased_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![
                        ph_id,
                        resolved.part_id.as_str(),
                        import.supplier,
                        line.supplier_sku,
                        unit_price_micros,
                        import.currency,
                        line.shipped_milli,
                        import_id.as_str(),
                        import.order_date,
                    ],
                )?;

                // The listing this decision itself just created/added gets
                // its running last-price/last-purchase-date fields kept in
                // sync. `AddStock` never creates a listing here (see
                // `Resolved::listing_id` doc comment), so it is intentionally
                // left alone -- picking one of a matched part's existing
                // listings to overwrite would be a guess, not a fact.
                if let Some(listing_id) = &resolved.listing_id {
                    tx.execute(
                        "UPDATE supplier_listings
                         SET last_unit_price_micros = ?2, last_purchase_date = ?3
                         WHERE id = ?1",
                        rusqlite::params![
                            listing_id.as_str(),
                            unit_price_micros,
                            import.order_date,
                        ],
                    )?;
                }
            }

            // Remember this line's identifiers so a repeat import resolves to
            // KnownAlias (see module doc comment on why duplicates are
            // tolerated rather than erroring).
            if let Some(sku) = non_empty(line.supplier_sku.as_deref()) {
                insert_alias_in_tx(&tx, "supplier_sku", sku, &resolved.part_id, "import")?;
            }
            if let Some(mpn) = non_empty(line.mpn.as_deref()) {
                insert_alias_in_tx(&tx, "mpn", mpn, &resolved.part_id, "import")?;
            }
        }

        let group = if receive_ops.is_empty() {
            None
        } else {
            let note = format!("import commit: {order_ref}");
            Some(build_group_in_tx(
                &tx,
                "import_commit",
                &note,
                &receive_ops,
            )?)
        };

        tx.execute(
            "UPDATE imports SET status = 'committed', commit_group_id = ?2 WHERE id = ?1",
            rusqlite::params![import_id.as_str(), group.as_ref().map(|g| g.id.as_str())],
        )?;

        tx.commit()?;

        touched_parts.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        touched_parts.dedup();
        for part_id in &touched_parts {
            self.refresh_search_text(part_id)?;
        }

        Ok(group.unwrap_or_else(|| {
            synthesize_empty_group(
                "import_commit",
                format!("import commit: {order_ref} (no receives)"),
            )
        }))
    }

    /// Reverse a committed import: every receive it produced returns to its
    /// pre-import stock level (via `reverse_group_in_tx` on the commit
    /// group), and `imports.status` flips to `reversed` -- both in ONE
    /// transaction, so a failed reversal never leaves the import half-flipped
    /// (see module doc comment). Parts created by the original commit are
    /// NOT deleted; they simply return to zero stock. A receiveless commit
    /// (`commit_group_id IS NULL`) has nothing to reverse group-wise; only
    /// the status flips.
    pub fn reverse_import(
        &mut self,
        import_id: &ImportId,
        note: &str,
    ) -> Result<GroupRecord, DbError> {
        let import = self.get_import(import_id)?.ok_or(DbError::ImportNotFound)?;
        if import.status != "committed" {
            return Err(DbError::ImportNotReversible);
        }
        let commit_group_id: Option<String> = self.raw_conn().query_row(
            "SELECT commit_group_id FROM imports WHERE id = ?1",
            [import_id.as_str()],
            |r| r.get(0),
        )?;

        let tx = self.conn_mut().transaction()?;
        let group = match commit_group_id {
            Some(raw) => {
                let group_id = GroupId::from_string(raw)
                    .map_err(|_| DbError::Corrupt("bad commit_group_id".into()))?;
                reverse_group_in_tx(&tx, &group_id, note)?
            }
            None => synthesize_empty_group_in_tx(&tx, "reverse:import_commit", note.to_string())?,
        };

        tx.execute(
            "UPDATE imports SET status = 'reversed' WHERE id = ?1",
            [import_id.as_str()],
        )?;
        tx.commit()?;
        Ok(group)
    }
}

/// A part's `quantity_unit`, read through the open transaction (so it sees
/// anything the same commit already inserted earlier in the transaction --
/// though `AddStock`/`AddAsVariant` only ever target pre-existing parts, this
/// stays tx-scoped for consistency with `add_variant_in_tx`'s same-tx
/// visibility rule). Typed `PartNotFound` rather than a bare FK/row-missing
/// error, matching every other part lookup in this codebase.
fn part_unit_in_tx(tx: &Transaction<'_>, part_id: &PartId) -> Result<QuantityUnit, DbError> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT quantity_unit FROM parts WHERE id = ?1",
            [part_id.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    let raw = raw.ok_or(DbError::PartNotFound)?;
    QuantityUnit::from_sql(&raw)
        .ok_or_else(|| DbError::Corrupt(format!("unknown quantity_unit '{raw}'")))
}

/// `INSERT OR IGNORE` into `part_aliases` -- see the module doc comment on
/// why a duplicate `(kind, value)` is tolerated rather than erroring.
fn insert_alias_in_tx(
    tx: &Transaction<'_>,
    kind: &str,
    value: &str,
    part_id: &PartId,
    source: &str,
) -> Result<(), DbError> {
    let id = inventory_core::id::new_id();
    tx.execute(
        "INSERT OR IGNORE INTO part_aliases (id, alias_kind, alias_value, part_id, source)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, kind, value, part_id.as_str(), source],
    )?;
    Ok(())
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// A never-persisted `GroupRecord` for the zero-receives edge case -- see the
/// module doc comment. `created_at` is read from `datetime('now')` inside the
/// transaction so it is a real, transaction-consistent timestamp even though
/// the row itself is never written.
fn synthesize_empty_group(kind: &str, note: String) -> GroupRecord {
    GroupRecord {
        id: GroupId::new(),
        kind: kind.to_string(),
        note,
        reversed_group_id: None,
        created_at: String::new(),
        transactions: Vec::new(),
    }
}

fn synthesize_empty_group_in_tx(
    tx: &Transaction<'_>,
    kind: &str,
    note: String,
) -> Result<GroupRecord, DbError> {
    let created_at: String = tx.query_row("SELECT datetime('now')", [], |r| r.get(0))?;
    Ok(GroupRecord {
        id: GroupId::new(),
        kind: kind.to_string(),
        note,
        reversed_group_id: None,
        created_at,
        transactions: Vec::new(),
    })
}
