use inventory_core::quantity::Quantity;
use inventory_db::Database;
use inventory_import::{LineKind, Money, ParsedInvoice, ParsedLine, ParsedOrderMeta, SourceFormat};

/// Open a database plus a separate, initially-empty attachments directory —
/// same pattern as `tests/attachments.rs`.
fn open() -> (tempfile::TempDir, Database, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    let attachments = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments).unwrap();
    (dir, db, attachments)
}

/// A synthesized two-line DigiKey-shaped invoice with exact prices/
/// quantities and every order-level total populated.
fn sample_invoice() -> ParsedInvoice {
    ParsedInvoice {
        supplier: "DigiKey".to_string(),
        source_format: SourceFormat::Csv,
        order: ParsedOrderMeta {
            order_number: Some("100353602".to_string()),
            invoice_number: Some("INV-1".to_string()),
            shipment_number: Some("1".to_string()),
            order_date: Some("2026-01-01".to_string()),
            currency: "USD".to_string(),
            subtotal: Money::parse("14.28", "USD"),
            shipping: Money::parse("4.99", "USD"),
            tax: Money::parse("1.24", "USD"),
            tariff: Money::parse("2.87", "USD"),
            total: Money::parse("23.38", "USD"),
            web_order_id: Some("373838988".to_string()),
        },
        lines: vec![
            ParsedLine {
                line_number: Some(1),
                supplier_sku: Some("296-1234-ND".to_string()),
                mpn: Some("TPS7A4700RGWR".to_string()),
                manufacturer: Some("Texas Instruments".to_string()),
                description: Some("LDO REG".to_string()),
                ordered: Some(Quantity::from_whole(10).unwrap()),
                shipped: Some(Quantity::from_whole(8).unwrap()),
                backordered: Some(Quantity::from_whole(2).unwrap()),
                unit_price: Money::parse("1.82000", "USD"),
                extended_price: Money::parse("14.56", "USD"),
                packaging: Some("Cut Tape".to_string()),
                customer_reference: Some("REF-A".to_string()),
                kind: LineKind::Part,
                confidence: 1.0,
                raw: serde_json::json!({"Part Number": "296-1234-ND", "Qty": "10"}),
            },
            ParsedLine {
                line_number: Some(2),
                supplier_sku: Some("311-1.00KCRCT-ND".to_string()),
                mpn: Some("RC0603FR-071KL".to_string()),
                manufacturer: Some("Yageo".to_string()),
                description: Some("RES 1K OHM 1%".to_string()),
                ordered: Some(Quantity::from_whole(100).unwrap()),
                shipped: Some(Quantity::from_whole(100).unwrap()),
                backordered: None,
                unit_price: Money::parse("0.01000", "USD"),
                extended_price: Money::parse("1.00", "USD"),
                packaging: Some("Cut Tape".to_string()),
                customer_reference: None,
                kind: LineKind::Part,
                confidence: 1.0,
                raw: serde_json::json!({"Part Number": "311-1.00KCRCT-ND", "Qty": "100"}),
            },
        ],
        warnings: vec![],
    }
}

/// A minimal invoice where every optional field (order-level and line-level)
/// is `None`, to prove `store_import` never fabricates a value and every
/// absent field lands as a SQL NULL.
fn sparse_invoice() -> ParsedInvoice {
    ParsedInvoice {
        supplier: "DigiKey".to_string(),
        source_format: SourceFormat::Pdf,
        order: ParsedOrderMeta {
            order_number: None,
            invoice_number: None,
            shipment_number: None,
            order_date: None,
            currency: "USD".to_string(),
            subtotal: None,
            shipping: None,
            tax: None,
            tariff: None,
            total: None,
            web_order_id: None,
        },
        lines: vec![ParsedLine {
            line_number: None,
            supplier_sku: None,
            mpn: None,
            manufacturer: None,
            description: None,
            ordered: None,
            shipped: None,
            backordered: None,
            unit_price: None,
            extended_price: None,
            packaging: None,
            customer_reference: None,
            kind: LineKind::Unknown,
            confidence: 0.2,
            raw: serde_json::json!({"raw text": "unparseable row"}),
        }],
        warnings: vec!["unrecognized row".to_string()],
    }
}

fn table_count(db: &Database, table: &str) -> i64 {
    db.raw_conn()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn store_import_persists_import_and_lines_with_exact_values() {
    let (_g, mut db, dir) = open();
    let parsed = sample_invoice();
    let original = b"pretend this is a CSV file's bytes";

    let record = db
        .store_import(&dir, &parsed, original, "order_100353602.csv")
        .unwrap();

    assert_eq!(record.supplier, "DigiKey");
    assert_eq!(record.order_number.as_deref(), Some("100353602"));
    assert_eq!(record.invoice_number.as_deref(), Some("INV-1"));
    assert_eq!(record.shipment_number.as_deref(), Some("1"));
    assert_eq!(record.order_date.as_deref(), Some("2026-01-01"));
    assert_eq!(record.currency, "USD");
    assert_eq!(record.source_format, "csv");
    assert_eq!(record.status, "parsed");
    assert_eq!(record.web_order_id.as_deref(), Some("373838988"));
    assert_eq!(record.line_count, 2);

    // Money round-trips as exact micros, verbatim from `Money::parse`.
    assert_eq!(record.subtotal_micros, Some(14_280_000));
    assert_eq!(record.shipping_micros, Some(4_990_000));
    assert_eq!(record.tax_micros, Some(1_240_000));
    assert_eq!(record.tariff_micros, Some(2_870_000));
    assert_eq!(record.total_micros, Some(23_380_000));

    let lines = db.list_import_lines(&record.id).unwrap();
    assert_eq!(lines.len(), 2);

    let l0 = &lines[0];
    assert_eq!(l0.line_number, Some(1));
    assert_eq!(l0.supplier_sku.as_deref(), Some("296-1234-ND"));
    assert_eq!(l0.mpn.as_deref(), Some("TPS7A4700RGWR"));
    assert_eq!(l0.manufacturer.as_deref(), Some("Texas Instruments"));
    assert_eq!(l0.description.as_deref(), Some("LDO REG"));
    assert_eq!(l0.packaging.as_deref(), Some("Cut Tape"));
    assert_eq!(l0.customer_reference.as_deref(), Some("REF-A"));
    // Quantities exact, verbatim milli.
    assert_eq!(l0.ordered_milli, Some(10_000));
    assert_eq!(l0.shipped_milli, Some(8_000));
    assert_eq!(l0.backordered_milli, Some(2_000));
    // Prices exact, verbatim micros (5-decimal unit price preserved).
    assert_eq!(l0.unit_price_micros, Some(1_820_000));
    assert_eq!(l0.extended_price_micros, Some(14_560_000));
    assert_eq!(l0.line_kind, "part");
    assert_eq!(l0.parse_confidence, 1.0);

    // raw_json round-trips exactly through serde_json.
    let raw_back: serde_json::Value = serde_json::from_str(&l0.raw_json).unwrap();
    assert_eq!(raw_back, parsed.lines[0].raw);

    let l1 = &lines[1];
    assert_eq!(l1.line_number, Some(2));
    assert_eq!(l1.backordered_milli, None, "no backorder on line 2");
    assert_eq!(l1.customer_reference, None);
    let raw_back1: serde_json::Value = serde_json::from_str(&l1.raw_json).unwrap();
    assert_eq!(raw_back1, parsed.lines[1].raw);

    // Original bytes are retrievable from the attachments store by the
    // hash `store_import` used, and the recorded size is correct.
    let files: Vec<(String, i64, String)> = {
        let mut stmt = db
            .raw_conn()
            .prepare("SELECT attachment_hash, byte_size, original_filename FROM import_files WHERE import_id = ?1")
            .unwrap();
        let rows = stmt
            .query_map([record.id.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        rows.collect::<Result<_, _>>().unwrap()
    };
    assert_eq!(files.len(), 1);
    let (hash, byte_size, original_filename) = &files[0];
    assert_eq!(*byte_size, original.len() as i64);
    assert_eq!(original_filename, "order_100353602.csv");
    let read_back = db.read_attachment(&dir, hash).unwrap();
    assert_eq!(read_back, original);

    // The hash exposed for duplicate-checking matches what got stored.
    assert_eq!(&inventory_db::imports::hash_bytes(original), hash);
}

#[test]
fn store_import_maps_absent_fields_to_null_not_a_guess() {
    let (_g, mut db, dir) = open();
    let parsed = sparse_invoice();
    let record = db
        .store_import(&dir, &parsed, b"scan bytes", "scan.pdf")
        .unwrap();

    assert_eq!(record.order_number, None);
    assert_eq!(record.invoice_number, None);
    assert_eq!(record.shipment_number, None);
    assert_eq!(record.order_date, None);
    assert_eq!(record.subtotal_micros, None);
    assert_eq!(record.shipping_micros, None);
    assert_eq!(record.tax_micros, None);
    assert_eq!(record.tariff_micros, None);
    assert_eq!(record.total_micros, None);
    assert_eq!(record.web_order_id, None);
    assert_eq!(record.source_format, "pdf");

    let lines = db.list_import_lines(&record.id).unwrap();
    assert_eq!(lines.len(), 1);
    let l = &lines[0];
    assert_eq!(l.line_number, None);
    assert_eq!(l.supplier_sku, None);
    assert_eq!(l.mpn, None);
    assert_eq!(l.manufacturer, None);
    assert_eq!(l.description, None);
    assert_eq!(l.ordered_milli, None);
    assert_eq!(l.shipped_milli, None);
    assert_eq!(l.backordered_milli, None);
    assert_eq!(l.unit_price_micros, None);
    assert_eq!(l.extended_price_micros, None);
    assert_eq!(l.packaging, None);
    assert_eq!(l.customer_reference, None);
    assert_eq!(l.line_kind, "unknown");
    // `store_import` casts the parser's `f32` confidence to `f64` verbatim
    // (no rounding scheme of its own), so the exact expected value is the
    // same f32->f64 widening applied to the same source literal.
    assert_eq!(l.parse_confidence, 0.2_f32 as f64);
}

#[test]
fn get_import_round_trips() {
    let (_g, mut db, dir) = open();
    let parsed = sample_invoice();
    let stored = db.store_import(&dir, &parsed, b"bytes", "a.csv").unwrap();

    let fetched = db.get_import(&stored.id).unwrap().unwrap();
    assert_eq!(fetched, stored);

    assert_eq!(
        db.get_import(&inventory_core::ids::ImportId::new())
            .unwrap(),
        None
    );
}

#[test]
fn list_imports_orders_newest_first() {
    let (_g, mut db, dir) = open();
    let mut parsed_a = sample_invoice();
    parsed_a.order.order_number = Some("AAA".to_string());
    let mut parsed_b = sample_invoice();
    parsed_b.order.order_number = Some("BBB".to_string());
    let mut parsed_c = sample_invoice();
    parsed_c.order.order_number = Some("CCC".to_string());

    let a = db.store_import(&dir, &parsed_a, b"a", "a.csv").unwrap();
    let b = db.store_import(&dir, &parsed_b, b"b", "b.csv").unwrap();
    let c = db.store_import(&dir, &parsed_c, b"c", "c.csv").unwrap();

    let listed = db.list_imports().unwrap();
    let ids: Vec<_> = listed.iter().map(|r| r.id.clone()).collect();
    // Newest first: c, then b, then a (insertion order reversed).
    assert_eq!(ids, vec![c.id, b.id, a.id]);
}

#[test]
fn list_import_lines_orders_by_line_number_and_matches_count() {
    let (_g, mut db, dir) = open();
    let mut parsed = sample_invoice();
    // Shuffle so insertion order differs from line_number order, and add an
    // unnumbered (None) line that must sort last.
    parsed.lines.swap(0, 1);
    parsed.lines.push(ParsedLine {
        line_number: None,
        supplier_sku: Some("FEE".to_string()),
        mpn: None,
        manufacturer: None,
        description: Some("Shipping".to_string()),
        ordered: None,
        shipped: None,
        backordered: None,
        unit_price: None,
        extended_price: Money::parse("4.99", "USD"),
        packaging: None,
        customer_reference: None,
        kind: LineKind::Fee,
        confidence: 1.0,
        raw: serde_json::json!({"row": "SHIPPING"}),
    });

    let record = db.store_import(&dir, &parsed, b"x", "x.csv").unwrap();
    let lines = db.list_import_lines(&record.id).unwrap();

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].line_number, Some(1));
    assert_eq!(lines[1].line_number, Some(2));
    assert_eq!(lines[2].line_number, None);
    assert_eq!(lines[2].line_kind, "fee");
}

#[test]
fn find_duplicate_imports_flags_same_content_hash() {
    let (_g, mut db, dir) = open();
    let parsed = sample_invoice();
    let original = b"exact same bytes twice";
    let stored = db
        .store_import(&dir, &parsed, original, "first.csv")
        .unwrap();

    // A second parse of a DIFFERENT order (so the order-number path can't
    // also be why it matches), but from the SAME bytes -> same hash.
    let mut other = sample_invoice();
    other.order.order_number = Some("different-order".to_string());
    other.order.invoice_number = Some("different-invoice".to_string());
    other.order.shipment_number = Some("different-shipment".to_string());

    let hash = inventory_db::imports::hash_bytes(original);
    let dupes = db.find_duplicate_imports(&other, &hash).unwrap();
    assert_eq!(dupes.len(), 1);
    assert_eq!(dupes[0].id, stored.id);
}

#[test]
fn find_duplicate_imports_flags_same_supplier_and_order_number_with_different_bytes() {
    let (_g, mut db, dir) = open();
    let parsed = sample_invoice();
    let stored = db
        .store_import(&dir, &parsed, b"original bytes", "first.csv")
        .unwrap();

    let mut resend = sample_invoice();
    // Same supplier + order_number, but different bytes entirely.
    assert_eq!(resend.supplier, parsed.supplier);
    assert_eq!(resend.order.order_number, parsed.order.order_number);
    resend.order.invoice_number = Some("a-new-invoice-number".to_string());

    let different_bytes_hash = inventory_db::imports::hash_bytes(b"totally different bytes");
    let dupes = db
        .find_duplicate_imports(&resend, &different_bytes_hash)
        .unwrap();
    assert_eq!(dupes.len(), 1);
    assert_eq!(dupes[0].id, stored.id);
}

#[test]
fn find_duplicate_imports_misses_different_order_and_different_bytes() {
    let (_g, mut db, dir) = open();
    let parsed = sample_invoice();
    db.store_import(&dir, &parsed, b"original bytes", "first.csv")
        .unwrap();

    let mut unrelated = sample_invoice();
    unrelated.order.order_number = Some("unrelated-order".to_string());
    unrelated.order.invoice_number = Some("unrelated-invoice".to_string());
    unrelated.order.shipment_number = Some("unrelated-shipment".to_string());

    let different_bytes_hash = inventory_db::imports::hash_bytes(b"nothing alike here");
    let dupes = db
        .find_duplicate_imports(&unrelated, &different_bytes_hash)
        .unwrap();
    assert!(dupes.is_empty());
}

#[test]
fn find_duplicate_imports_does_not_match_on_null_identifiers() {
    let (_g, mut db, dir) = open();
    // Store an import with every order identifier absent.
    let sparse = sparse_invoice();
    db.store_import(&dir, &sparse, b"sparse bytes", "s.pdf")
        .unwrap();

    // A second, also-sparse invoice with different bytes must NOT be
    // flagged as a duplicate: absent identifiers never match each other.
    let mut also_sparse = sparse_invoice();
    also_sparse.order.currency = "USD".to_string();
    let hash = inventory_db::imports::hash_bytes(b"different sparse bytes");
    let dupes = db.find_duplicate_imports(&also_sparse, &hash).unwrap();
    assert!(dupes.is_empty());
}

#[test]
fn store_import_does_not_touch_inventory_tables() {
    let (_g, mut db, dir) = open();
    assert_eq!(table_count(&db, "parts"), 0);
    assert_eq!(table_count(&db, "transactions"), 0);
    assert_eq!(table_count(&db, "part_stock"), 0);

    let parsed = sample_invoice();
    db.store_import(&dir, &parsed, b"bytes", "a.csv").unwrap();

    // Still zero: importing never mutates inventory (5b's job).
    assert_eq!(table_count(&db, "parts"), 0);
    assert_eq!(table_count(&db, "transactions"), 0);
    assert_eq!(table_count(&db, "part_stock"), 0);
    // Sanity: the import itself DID persist.
    assert_eq!(table_count(&db, "imports"), 1);
    assert_eq!(table_count(&db, "import_lines"), 2);
}
