//! Task 3 (Phase 5b): the import review model — a derived, per-line plan
//! over a persisted import + its matches. No inventory mutation anywhere in
//! this file; every test only asserts on what `build_import_review` returns.
//!
//! Scenario helpers mirror `tests/import_match.rs`'s pattern: seed a part
//! with a manufacturer variant + supplier listing, persist a crafted
//! `ParsedInvoice` via `store_import`, then drive `build_import_review` over
//! the real persisted lines.

use std::path::PathBuf;

use inventory_core::ids::{CategoryId, ImportId, PartId, VariantId};
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::import_review::ProposedAction;
use inventory_db::parts::{ListingDraft, ListingRecord, PartDraft, VariantDraft, VariantRecord};
use inventory_db::{Database, DbError};
use inventory_import::{LineKind, Money, ParsedInvoice, ParsedLine, ParsedOrderMeta, SourceFormat};

fn open() -> (tempfile::TempDir, Database, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    let attachments = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments).unwrap();
    (dir, db, attachments)
}

fn category_id(db: &Database, name: &str) -> CategoryId {
    let raw: String = db
        .raw_conn()
        .query_row("SELECT id FROM categories WHERE name = ?1", [name], |r| {
            r.get(0)
        })
        .unwrap();
    CategoryId::from_string(raw).unwrap()
}

fn make_part(db: &mut Database, name: &str) -> PartId {
    let cat = category_id(db, "Resistor");
    db.create_part(&PartDraft {
        display_name: name.to_string(),
        category_id: cat,
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    })
    .unwrap()
    .id
}

fn variant(db: &mut Database, part_id: &PartId, manufacturer: &str, mpn: &str) -> VariantRecord {
    db.add_variant(
        part_id,
        &VariantDraft {
            manufacturer: manufacturer.to_string(),
            mpn: mpn.to_string(),
            description: String::new(),
            package: None,
            datasheet_url: None,
            product_url: None,
            lifecycle: None,
            notes: String::new(),
        },
    )
    .unwrap()
}

fn listing(
    db: &mut Database,
    variant_id: &VariantId,
    supplier: &str,
    supplier_sku: &str,
) -> ListingRecord {
    db.add_supplier_listing(
        variant_id,
        &ListingDraft {
            supplier: supplier.to_string(),
            supplier_sku: supplier_sku.to_string(),
            product_url: None,
            packaging: None,
            typical_order: None,
            last_unit_price_micros: None,
            currency: None,
            last_purchase_date: None,
        },
    )
    .unwrap()
}

fn order_meta() -> ParsedOrderMeta {
    ParsedOrderMeta {
        order_number: Some("ORDER-1".to_string()),
        invoice_number: None,
        shipment_number: None,
        order_date: Some("2026-01-01".to_string()),
        currency: "USD".to_string(),
        subtotal: None,
        shipping: None,
        tax: None,
        tariff: None,
        total: None,
        web_order_id: None,
    }
}

/// A `Part`-kind line with the given identifying fields and explicit
/// ordered/shipped whole-unit counts (so tests can drive the
/// backorder/partial-shipment logic precisely).
#[allow(clippy::too_many_arguments)]
fn part_line(
    line_number: u32,
    supplier_sku: Option<&str>,
    mpn: Option<&str>,
    manufacturer: Option<&str>,
    ordered: Option<u32>,
    shipped: Option<u32>,
) -> ParsedLine {
    ParsedLine {
        line_number: Some(line_number),
        supplier_sku: supplier_sku.map(str::to_string),
        mpn: mpn.map(str::to_string),
        manufacturer: manufacturer.map(str::to_string),
        description: Some("a part".to_string()),
        ordered: ordered.map(|q| Quantity::from_whole(q as i64).unwrap()),
        shipped: shipped.map(|q| Quantity::from_whole(q as i64).unwrap()),
        backordered: None,
        unit_price: Money::parse("1.00", "USD"),
        extended_price: Money::parse("10.00", "USD"),
        packaging: None,
        customer_reference: None,
        kind: LineKind::Part,
        confidence: 1.0,
        raw: serde_json::json!({}),
    }
}

fn non_inventory_line(line_number: u32, kind: LineKind) -> ParsedLine {
    ParsedLine {
        line_number: Some(line_number),
        supplier_sku: None,
        mpn: None,
        manufacturer: None,
        description: Some("Tariff".to_string()),
        ordered: None,
        shipped: None,
        backordered: None,
        unit_price: None,
        extended_price: Money::parse("2.87", "USD"),
        packaging: None,
        customer_reference: None,
        kind,
        confidence: 1.0,
        raw: serde_json::json!({"row": "TARIFF"}),
    }
}

fn invoice(supplier: &str, lines: Vec<ParsedLine>) -> ParsedInvoice {
    ParsedInvoice {
        supplier: supplier.to_string(),
        source_format: SourceFormat::Csv,
        order: order_meta(),
        lines,
        warnings: vec![],
    }
}

// --- an exact match proposes AddStockToExisting -----------------------------

#[test]
fn part_line_with_exact_match_proposes_add_stock_to_existing() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "10k resistor");
    let v = variant(&mut db, &part, "Yageo", "RC0603FR-0710KL");
    listing(&mut db, &v.id, "DigiKey", "311-10.0KLRCT-ND");

    let parsed = invoice(
        "DigiKey",
        vec![part_line(
            1,
            Some("311-10.0KLRCT-ND"),
            None,
            None,
            Some(10),
            Some(10),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();

    let review = db.build_import_review(&record.id).unwrap();
    assert_eq!(review.lines.len(), 1);
    let line = &review.lines[0];
    match &line.proposed {
        ProposedAction::AddStockToExisting {
            part_id,
            verdict_kind,
        } => {
            assert_eq!(*part_id, part);
            assert_eq!(verdict_kind, "exact_sku");
        }
        other => panic!("expected AddStockToExisting, got {other:?}"),
    }
    assert_eq!(line.receive_qty_milli, Some(10_000));
    assert!(line.warning.is_none());
    assert_eq!(review.total_receive_lines, 1);
}

// --- an unmatched line proposes CreateNew ------------------------------------

#[test]
fn unmatched_part_line_proposes_create_new() {
    let (_g, mut db, attachments) = open();
    // Seed an unrelated part so the "no match" outcome is a real miss, not
    // an empty parts table.
    make_part(&mut db, "unrelated part");

    let parsed = invoice(
        "DigiKey",
        vec![part_line(
            1,
            Some("NOPE-SKU"),
            Some("NOPE-MPN"),
            Some("Nobody"),
            Some(5),
            Some(5),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();

    let review = db.build_import_review(&record.id).unwrap();
    assert_eq!(review.lines[0].proposed, ProposedAction::CreateNew);
    assert!(review.lines[0].matches.is_empty());
    assert_eq!(review.lines[0].receive_qty_milli, Some(5_000));
}

// --- fully backordered: no receive, warns ------------------------------------

#[test]
fn fully_backordered_part_line_has_no_receive_qty_and_warns() {
    let (_g, mut db, attachments) = open();
    let parsed = invoice(
        "DigiKey",
        vec![part_line(1, Some("SKU-X"), None, None, Some(10), Some(0))],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();

    let review = db.build_import_review(&record.id).unwrap();
    let line = &review.lines[0];
    assert!(
        line.receive_qty_milli.is_none() || line.receive_qty_milli == Some(0),
        "fully backordered line must not propose a positive receive: {:?}",
        line.receive_qty_milli
    );
    assert_eq!(
        line.warning.as_deref(),
        Some("fully backordered — nothing to receive")
    );
    assert_eq!(review.total_receive_lines, 0);
}

// --- partial shipment: receive shipped, no warning ---------------------------

#[test]
fn partial_shipment_receives_shipped_amount_without_warning() {
    let (_g, mut db, attachments) = open();
    let parsed = invoice(
        "DigiKey",
        vec![part_line(1, Some("SKU-Y"), None, None, Some(10), Some(8))],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();

    let review = db.build_import_review(&record.id).unwrap();
    let line = &review.lines[0];
    assert_eq!(line.receive_qty_milli, Some(8_000));
    assert_eq!(line.ordered_milli, Some(10_000));
    assert_eq!(line.backordered_milli, None);
    assert!(line.warning.is_none());
    assert_eq!(review.total_receive_lines, 1);
}

// --- fee/tariff lines are non-inventory but still appear ---------------------

#[test]
fn fee_and_tariff_lines_propose_non_inventory_but_still_appear() {
    let (_g, mut db, attachments) = open();
    let parsed = invoice(
        "DigiKey",
        vec![
            part_line(1, Some("SKU-1"), None, None, Some(1), Some(1)),
            non_inventory_line(2, LineKind::Fee),
            non_inventory_line(3, LineKind::Tariff),
        ],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();

    let review = db.build_import_review(&record.id).unwrap();
    assert_eq!(review.lines.len(), 3);
    assert_eq!(review.lines[1].kind, "fee");
    assert_eq!(review.lines[1].proposed, ProposedAction::NonInventory);
    assert!(review.lines[1].matches.is_empty());
    assert_eq!(review.lines[2].kind, "tariff");
    assert_eq!(review.lines[2].proposed, ProposedAction::NonInventory);
}

// --- unknown-kind line proposes Ignore ---------------------------------------

#[test]
fn unknown_kind_line_proposes_ignore() {
    let (_g, mut db, attachments) = open();
    let parsed = invoice("DigiKey", vec![non_inventory_line(1, LineKind::Unknown)]);
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();

    let review = db.build_import_review(&record.id).unwrap();
    assert_eq!(review.lines[0].kind, "unknown");
    assert_eq!(review.lines[0].proposed, ProposedAction::Ignore);
}

// --- duplicate_of: same order imported twice, self excluded -----------------

#[test]
fn duplicate_of_flags_repeat_import_but_excludes_self() {
    let (_g, mut db, attachments) = open();
    let parsed = invoice(
        "DigiKey",
        vec![part_line(1, Some("SKU-1"), None, None, Some(1), Some(1))],
    );
    let first = db
        .store_import(&attachments, &parsed, b"bytes-a", "a.csv")
        .unwrap();
    let second = db
        .store_import(&attachments, &parsed, b"bytes-b", "b.csv")
        .unwrap();

    let review_first = db.build_import_review(&first.id).unwrap();
    let dup_ids_first: Vec<_> = review_first
        .duplicate_of
        .iter()
        .map(|d| d.id.clone())
        .collect();
    assert!(
        !dup_ids_first.contains(&first.id),
        "an import must never be flagged as its own duplicate"
    );
    assert!(
        dup_ids_first.contains(&second.id),
        "the later same-order import should show up as a duplicate of the first: {dup_ids_first:?}"
    );

    let review_second = db.build_import_review(&second.id).unwrap();
    let dup_ids_second: Vec<_> = review_second
        .duplicate_of
        .iter()
        .map(|d| d.id.clone())
        .collect();
    assert!(!dup_ids_second.contains(&second.id));
    assert!(dup_ids_second.contains(&first.id));
}

// --- total_receive_lines counts only positive-receive part lines ------------

#[test]
fn total_receive_lines_counts_only_positive_receive_part_lines() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "10k resistor");
    let v = variant(&mut db, &part, "Yageo", "RC0603FR-0710KL");
    listing(&mut db, &v.id, "DigiKey", "311-10.0KLRCT-ND");

    let parsed = invoice(
        "DigiKey",
        vec![
            // matched, receives 10
            part_line(1, Some("311-10.0KLRCT-ND"), None, None, Some(10), Some(10)),
            // unmatched (CreateNew), still receives 5
            part_line(
                2,
                Some("NEW-SKU"),
                Some("NEW-MPN"),
                Some("Nobody"),
                Some(5),
                Some(5),
            ),
            // fully backordered, receives nothing
            part_line(3, Some("BACKORDER-SKU"), None, None, Some(10), Some(0)),
            // non-inventory, never counted
            non_inventory_line(4, LineKind::Fee),
        ],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();

    let review = db.build_import_review(&record.id).unwrap();
    assert_eq!(review.lines.len(), 4);
    assert_eq!(review.total_receive_lines, 2);
}

// --- unknown import id ---------------------------------------------------

#[test]
fn build_import_review_unknown_import_id_is_not_found() {
    let (_g, mut db, _attachments) = open();
    let err = db.build_import_review(&ImportId::new()).unwrap_err();
    assert!(matches!(err, DbError::ImportNotFound));
}
