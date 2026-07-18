//! Task 2 (Phase 5b): match persisted import lines against existing parts,
//! reusing the existing 7-level matcher (`matching::find_matches`) verbatim.
//!
//! Scenario helpers seed a part with a manufacturer variant + supplier
//! listing (mirrors `tests/matching.rs`'s pattern) and persist a crafted
//! `ParsedInvoice` via `store_import` (mirrors `tests/imports.rs`'s pattern)
//! so each test can drive `match_import_line`/`match_import` over real
//! persisted `ImportLineRecord`s rather than hand-built ones.

use std::path::PathBuf;

use inventory_core::ids::{CategoryId, PartId, VariantId};
use inventory_core::quantity::{Quantity, QuantityUnit};
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

/// A minimal `Part`-kind line with the given identifying fields; quantities/
/// price are filled in with harmless non-null defaults since they're
/// irrelevant to matching. `supplier_sku`/`mpn`/`manufacturer` pass through
/// verbatim (including `Some("")`, to exercise the blank -> absent mapping).
fn part_line(
    line_number: u32,
    supplier_sku: Option<&str>,
    mpn: Option<&str>,
    manufacturer: Option<&str>,
) -> ParsedLine {
    ParsedLine {
        line_number: Some(line_number),
        supplier_sku: supplier_sku.map(str::to_string),
        mpn: mpn.map(str::to_string),
        manufacturer: manufacturer.map(str::to_string),
        description: Some("a part".to_string()),
        ordered: Some(Quantity::from_whole(10).unwrap()),
        shipped: Some(Quantity::from_whole(10).unwrap()),
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

// --- match_import_line: exact_sku -------------------------------------------

#[test]
fn line_matching_supplier_sku_is_exact_sku() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "10k resistor");
    let v = variant(&mut db, &part, "Yageo", "RC0603FR-0710KL");
    listing(&mut db, &v.id, "DigiKey", "311-10.0KLRCT-ND");

    let parsed = invoice(
        "DigiKey",
        vec![part_line(1, Some("311-10.0KLRCT-ND"), None, None)],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let lines = db.list_import_lines(&record.id).unwrap();
    assert_eq!(lines.len(), 1);

    let result = db.match_import_line(&lines[0], &record.supplier).unwrap();
    assert_eq!(result.line_id, lines[0].id);
    assert_eq!(result.line_number, Some(1));
    let top = result.top.clone().expect("should have a top match");
    assert_eq!(top.verdict_kind, "exact_sku");
    assert_eq!(top.part_id, part);
    assert!(
        !top.explanation.is_empty(),
        "explanation should be populated"
    );
    assert_eq!(result.matches.len(), 1);
}

// --- match_import_line: exact_mpn, blank sku --------------------------------

#[test]
fn line_matching_mpn_with_blank_sku_is_exact_mpn() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "10k resistor");
    variant(&mut db, &part, "Yageo", "RC0603FR-0710KL");

    // Blank (empty-string) supplier_sku must be treated as absent, not as a
    // literal empty-string SKU to search for.
    let parsed = invoice(
        "DigiKey",
        vec![part_line(
            1,
            Some(""),
            Some("RC0603FR-0710KL"),
            Some("Yageo"),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let lines = db.list_import_lines(&record.id).unwrap();

    let result = db.match_import_line(&lines[0], &record.supplier).unwrap();
    let top = result.top.expect("should have a top match");
    assert_eq!(top.verdict_kind, "exact_mpn");
    assert_eq!(top.part_id, part);
}

// --- match_import_line: known_alias, no listing -----------------------------

#[test]
fn line_matching_aliased_sku_with_no_listing_is_known_alias() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "10k resistor");
    db.add_alias("supplier_sku", "OLD-SKU-1", &part, "manual")
        .unwrap();

    let parsed = invoice("DigiKey", vec![part_line(1, Some("OLD-SKU-1"), None, None)]);
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let lines = db.list_import_lines(&record.id).unwrap();

    let result = db.match_import_line(&lines[0], &record.supplier).unwrap();
    let top = result.top.expect("should have a top match");
    assert_eq!(top.verdict_kind, "known_alias");
    assert_eq!(top.part_id, part);
}

// --- match_import_line: unknown line ----------------------------------------

#[test]
fn line_matching_unknown_sku_and_mpn_has_no_matches() {
    let (_g, mut db, attachments) = open();
    // Seed an unrelated part so the database isn't empty, proving the
    // "no match" outcome is a real miss rather than an empty part table.
    make_part(&mut db, "unrelated part");

    let parsed = invoice(
        "DigiKey",
        vec![part_line(
            1,
            Some("NOPE-SKU"),
            Some("NOPE-MPN"),
            Some("Nobody"),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let lines = db.list_import_lines(&record.id).unwrap();

    let result = db.match_import_line(&lines[0], &record.supplier).unwrap();
    assert!(result.matches.is_empty());
    assert!(result.top.is_none());
}

// --- match_import: non-part lines excluded ----------------------------------

#[test]
fn match_import_excludes_non_part_lines() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "10k resistor");
    let v = variant(&mut db, &part, "Yageo", "RC0603FR-0710KL");
    listing(&mut db, &v.id, "DigiKey", "311-10.0KLRCT-ND");

    let parsed = invoice(
        "DigiKey",
        vec![
            part_line(1, Some("311-10.0KLRCT-ND"), None, None),
            non_inventory_line(2, LineKind::Tariff),
            non_inventory_line(3, LineKind::Fee),
        ],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();

    let results = db.match_import(&record.id).unwrap();
    assert_eq!(
        results.len(),
        1,
        "only the Part line should be matched: {results:?}"
    );
    assert_eq!(results[0].line_number, Some(1));
    let top = results[0]
        .top
        .clone()
        .expect("the part line should match the seeded part");
    assert_eq!(top.part_id, part);
    assert_eq!(top.verdict_kind, "exact_sku");
}

#[test]
fn match_import_unknown_import_id_is_not_found() {
    let (_g, mut db, _attachments) = open();
    let err = db
        .match_import(&inventory_core::ids::ImportId::new())
        .unwrap_err();
    assert!(matches!(err, DbError::ImportNotFound));
}

// --- sanity: find_duplicate_imports remains callable for Task 3 ------------

#[test]
fn find_duplicate_imports_still_resolves_for_same_order() {
    let (_g, mut db, attachments) = open();
    let parsed = invoice("DigiKey", vec![part_line(1, Some("SKU-1"), None, None)]);
    let stored = db
        .store_import(&attachments, &parsed, b"bytes-a", "a.csv")
        .unwrap();

    // Same supplier + order_number as the stored import, but different bytes
    // entirely — must still resolve as a duplicate via the order-number path.
    let resend = parsed.clone();
    let different_bytes_hash = inventory_db::imports::hash_bytes(b"different-bytes");
    let dupes = db
        .find_duplicate_imports(&resend, &different_bytes_hash)
        .unwrap();
    assert_eq!(dupes.len(), 1);
    assert_eq!(dupes[0].id, stored.id);
}
