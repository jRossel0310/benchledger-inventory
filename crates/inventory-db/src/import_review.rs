//! Derive a per-line review plan for a persisted import (Phase 5b Task 3,
//! spec §10 "Review"). This is the middle stage of Match -> Review -> Commit:
//! it reads the import and its lines, matches every part-kind line against
//! parts on file (delegating entirely to `match_import`, Task 2), and folds
//! that into a default proposed action + warning per line. It is **purely
//! derived** — it never writes to `parts`/`manufacturer_variants`/
//! `supplier_listings`/`transactions`/anything inventory-side. Commit
//! (Task 4) is the only mutation; the UI (5d) lets the user override any of
//! these defaults before confirming.
//!
//! ## `receive_qty_milli` is raw milli, not a `Quantity`
//!
//! An import line has no unit of its own — `imports`/`import_lines` never
//! stored one (see `imports.rs`), because DigiKey invoices don't carry a
//! part's inventory unit, only a bare count. Building a `Quantity` here would
//! force a guess (`each`, matching the parsers' own default — see
//! `inventory-import`) that is very often WRONG once a line resolves to an
//! existing part with a different `quantity_unit` (e.g. `Feet`, `Grams`).
//! Rather than bake in a wrong-unit assumption at review time, this module
//! surfaces the exact shipped amount as a raw `i64` milli count
//! (`receive_qty_milli`) and defers building the unit-correct `Quantity` to
//! Task 4 (`commit_import`), which has the resolved `part_id` in hand and can
//! read that part's real `quantity_unit` before constructing the `Quantity`
//! it actually receives.

use inventory_core::ids::{ImportId, ImportLineId, PartId};
use inventory_import::{ParsedInvoice, ParsedOrderMeta, SourceFormat};

use crate::imports::ImportRecord;
use crate::matching::MatchResult;
use crate::{Database, DbError};

/// The default per-line action `build_import_review` proposes, derived
/// purely from the line's `line_kind` and its top match (Task 2). The 5d UI
/// lets the user override any of these; this is only the sensible starting
/// point, never itself a mutation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProposedAction {
    /// A `part` line with a top match on file: receive the shipped quantity
    /// against `part_id`. `verdict_kind` mirrors the top `MatchResult`'s
    /// (e.g. `"exact_sku"`) so the UI can render the reason without
    /// re-deriving it from `matches`.
    AddStockToExisting {
        part_id: PartId,
        verdict_kind: String,
    },
    /// A `part` line with no match anywhere in the 7-level hierarchy:
    /// propose creating a new part for it.
    CreateNew,
    /// `fee`/`tariff`/`no_charge`: never creates inventory or a receive.
    NonInventory,
    /// `unknown`-kind line (the parser couldn't classify it): undetermined,
    /// so the default is to not act — the user decides in 5d.
    Ignore,
}

/// One reviewable line: the persisted line's identifying/quantity/price
/// fields, its match results (part lines only — see `match_import`), the
/// default `proposed` action, and an optional human-readable `warning`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ImportReviewLine {
    pub line_id: ImportLineId,
    pub line_number: Option<i64>,
    pub kind: String,
    pub supplier_sku: Option<String>,
    pub mpn: Option<String>,
    pub manufacturer: Option<String>,
    pub description: Option<String>,
    /// The line's SHIPPED quantity, raw milli — never the ordered quantity
    /// (spec §10: ordered 10 / shipped 8 / backordered 2 -> receive 8). See
    /// the module doc comment for why this is a raw `i64` rather than a
    /// `Quantity`. `None` for any non-`part` line, and for a `part` line
    /// whose shipped amount is absent or zero (nothing to receive).
    pub receive_qty_milli: Option<i64>,
    pub ordered_milli: Option<i64>,
    pub backordered_milli: Option<i64>,
    pub unit_price_micros: Option<i64>,
    pub matches: Vec<MatchResult>,
    pub proposed: ProposedAction,
    pub warning: Option<String>,
}

/// The full review for one persisted import: its metadata, every one of its
/// lines (part AND non-part, so the review shows the whole order, not just
/// the inventory-affecting subset), any other imports on file that look like
/// the same order/file (`duplicate_of`, self excluded), and a receive-line
/// count for a quick "N lines will receive stock" summary.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ImportReview {
    pub import: ImportRecord,
    pub lines: Vec<ImportReviewLine>,
    pub duplicate_of: Vec<ImportRecord>,
    pub total_receive_lines: usize,
}

impl Database {
    /// Assemble the review for `import_id`. Purely derived — reads the
    /// import, ALL of its lines, and runs `match_import` (Task 2, itself
    /// read-only) once for the part-line matches; no inventory table is ever
    /// written here.
    pub fn build_import_review(&mut self, import_id: &ImportId) -> Result<ImportReview, DbError> {
        let import = self.get_import(import_id)?.ok_or(DbError::ImportNotFound)?;
        let all_lines = self.list_import_lines(import_id)?;
        let line_matches = self.match_import(import_id)?;

        let mut lines = Vec::with_capacity(all_lines.len());
        for line in &all_lines {
            let is_part = line.line_kind == "part";
            let found = line_matches.iter().find(|m| m.line_id == line.id);
            let matches = found.map(|m| m.matches.clone()).unwrap_or_default();
            let top = found.and_then(|m| m.top.clone());

            let (receive_qty_milli, warning) = if is_part {
                shipped_receive_and_warning(line.shipped_milli, line.ordered_milli)
            } else {
                (None, None)
            };

            let proposed = if is_part {
                match &top {
                    Some(m) => ProposedAction::AddStockToExisting {
                        part_id: m.part_id.clone(),
                        verdict_kind: m.verdict_kind.clone(),
                    },
                    None => ProposedAction::CreateNew,
                }
            } else if line.line_kind == "unknown" {
                ProposedAction::Ignore
            } else {
                // fee | tariff | no_charge
                ProposedAction::NonInventory
            };

            lines.push(ImportReviewLine {
                line_id: line.id.clone(),
                line_number: line.line_number,
                kind: line.line_kind.clone(),
                supplier_sku: line.supplier_sku.clone(),
                mpn: line.mpn.clone(),
                manufacturer: line.manufacturer.clone(),
                description: line.description.clone(),
                receive_qty_milli,
                ordered_milli: line.ordered_milli,
                backordered_milli: line.backordered_milli,
                unit_price_micros: line.unit_price_micros,
                matches,
                proposed,
                warning,
            });
        }

        let total_receive_lines = lines
            .iter()
            .filter(|l| l.kind == "part" && l.receive_qty_milli.is_some_and(|q| q > 0))
            .count();

        let duplicate_of = duplicate_imports_excluding_self(self, &import)?;

        Ok(ImportReview {
            import,
            lines,
            duplicate_of,
            total_receive_lines,
        })
    }
}

/// `(receive_qty_milli, warning)` for one `part` line from its persisted
/// shipped/ordered milli: shipped > 0 always receives exactly that amount,
/// no warning (a partial shipment — shipped < ordered but shipped > 0 — is
/// completely normal). Shipped absent-or-zero receives nothing; that only
/// earns the backorder warning when `ordered` is positive (a genuinely
/// backordered line), never merely because shipped data is missing.
fn shipped_receive_and_warning(
    shipped_milli: Option<i64>,
    ordered_milli: Option<i64>,
) -> (Option<i64>, Option<String>) {
    match shipped_milli {
        Some(shipped) if shipped > 0 => (Some(shipped), None),
        Some(0) if ordered_milli.is_some_and(|o| o > 0) => (
            None,
            Some("fully backordered — nothing to receive".to_string()),
        ),
        _ => (None, None),
    }
}

/// `find_duplicate_imports` (5a) resolved for an already-persisted import:
/// re-derives the `ParsedInvoice`-shaped identifiers `find_duplicate_imports`
/// needs from the stored `ImportRecord`'s own fields (`lines`/`warnings` are
/// irrelevant to that query and left empty), looks up the content hash this
/// import's own file was stored under, and filters the import's own id out
/// of the result — otherwise every import would trivially "duplicate itself"
/// via its own content hash.
fn duplicate_imports_excluding_self(
    db: &Database,
    import: &ImportRecord,
) -> Result<Vec<ImportRecord>, DbError> {
    let file_hash: String = db
        .raw_conn()
        .query_row(
            "SELECT attachment_hash FROM import_files WHERE import_id = ?1 ORDER BY rowid LIMIT 1",
            [import.id.as_str()],
            |r| r.get(0),
        )
        .unwrap_or_default();

    let synthetic = ParsedInvoice {
        supplier: import.supplier.clone(),
        // Irrelevant to `find_duplicate_imports` (it only reads supplier +
        // order identifiers), but filled in honestly rather than defaulted
        // to a fixed variant.
        source_format: SourceFormat::from_db_str(&import.source_format)
            .unwrap_or(SourceFormat::Csv),
        order: ParsedOrderMeta {
            order_number: import.order_number.clone(),
            invoice_number: import.invoice_number.clone(),
            shipment_number: import.shipment_number.clone(),
            order_date: import.order_date.clone(),
            currency: import.currency.clone(),
            subtotal: None,
            shipping: None,
            tax: None,
            tariff: None,
            total: None,
            web_order_id: import.web_order_id.clone(),
        },
        lines: vec![],
        warnings: vec![],
    };

    let dupes = db.find_duplicate_imports(&synthetic, &file_hash)?;
    Ok(dupes.into_iter().filter(|d| d.id != import.id).collect())
}
