//! Task 4 (Phase 5b): atomic import commit + reversal -- the crux of the
//! Match -> Review -> Commit pipeline (spec §10 "Confirm"). `commit_import`
//! is the ONLY mutation anywhere in the import pipeline; every test here
//! either exercises a real commit end-to-end or proves the atomicity/
//! reversibility guarantees hold under failure. Scenario helpers mirror
//! `tests/import_review.rs`'s pattern: persist a crafted `ParsedInvoice` via
//! `store_import`, then drive `commit_import`/`reverse_import` over the real
//! persisted lines.

use std::path::PathBuf;

use inventory_core::ids::{CategoryId, ImportId, ImportLineId, PartId, VariantId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::import_commit::LineDecision;
use inventory_db::imports::ImportLineRecord;
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

fn part_draft(name: &str, category_id: CategoryId, unit: QuantityUnit) -> PartDraft {
    PartDraft {
        display_name: name.to_string(),
        category_id,
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: unit,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    }
}

fn variant_draft(manufacturer: &str, mpn: &str) -> VariantDraft {
    VariantDraft {
        manufacturer: manufacturer.to_string(),
        mpn: mpn.to_string(),
        description: String::new(),
        package: None,
        datasheet_url: None,
        product_url: None,
        lifecycle: None,
        notes: String::new(),
    }
}

fn listing_draft(supplier: &str, supplier_sku: &str) -> ListingDraft {
    ListingDraft {
        supplier: supplier.to_string(),
        supplier_sku: supplier_sku.to_string(),
        product_url: None,
        packaging: None,
        typical_order: None,
        last_unit_price_micros: None,
        currency: None,
        last_purchase_date: None,
    }
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
/// ordered/shipped whole-unit counts, and a $1.00/unit price so
/// `unit_price_micros` is always `Some(1_000_000)`.
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

fn invoice(supplier: &str, lines: Vec<ParsedLine>) -> ParsedInvoice {
    ParsedInvoice {
        supplier: supplier.to_string(),
        source_format: SourceFormat::Csv,
        order: order_meta(),
        lines,
        warnings: vec![],
    }
}

/// The persisted line id whose `supplier_sku` is `sku` -- lets tests build
/// `(ImportLineId, LineDecision)` pairs against the real rows `store_import`
/// created, without hardcoding line ordering.
fn line_id_by_sku(db: &mut Database, import_id: &ImportId, sku: &str) -> ImportLineId {
    db.list_import_lines(import_id)
        .unwrap()
        .into_iter()
        .find(|l| l.supplier_sku.as_deref() == Some(sku))
        .unwrap_or_else(|| panic!("no persisted line with supplier_sku {sku}"))
        .id
}

// --- AddStock: existing part, alias remembered ------------------------------

#[test]
fn add_stock_receives_shipped_and_records_alias_for_next_match() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "10k resistor");
    variant(&mut db, &part, "Yageo", "RC0603FR-0710KL");
    // Deliberately NO supplier listing for this SKU -- this simulates the
    // user manually resolving a line `match_import` couldn't match on its
    // own (no SKU/MPN/identity hit) to an existing part via `AddStock`.
    let parsed = invoice(
        "DigiKey",
        vec![part_line(
            1,
            Some("NEW-SKU-1"),
            None,
            None,
            Some(10),
            Some(10),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let line_id = line_id_by_sku(&mut db, &record.id, "NEW-SKU-1");

    let group = db
        .commit_import(
            &record.id,
            &[(
                line_id,
                LineDecision::AddStock {
                    part_id: part.clone(),
                },
            )],
        )
        .unwrap();

    assert_eq!(group.transactions.len(), 1);
    assert_eq!(group.transactions[0].txn_type, "receive");
    assert_eq!(
        db.get_stock(&part).unwrap().available,
        Quantity::from_whole(10).unwrap()
    );
    assert_eq!(
        db.get_import(&record.id).unwrap().unwrap().status,
        "committed"
    );

    // A repeat sighting of the same SKU (a fresh, never-persisted line, as
    // `match_import_line` accepts any `ImportLineRecord`) now resolves via
    // the alias recorded during commit -- KnownAlias, not "no match".
    let repeat_line = ImportLineRecord {
        id: ImportLineId::new(),
        import_id: record.id.clone(),
        line_number: None,
        supplier_sku: Some("NEW-SKU-1".to_string()),
        mpn: None,
        manufacturer: None,
        description: None,
        packaging: None,
        customer_reference: None,
        ordered_milli: None,
        shipped_milli: None,
        backordered_milli: None,
        unit_price_micros: None,
        extended_price_micros: None,
        raw_json: "{}".to_string(),
        line_kind: "part".to_string(),
        parse_confidence: 1.0,
        created_at: String::new(),
    };
    let rematch = db.match_import_line(&repeat_line, "DigiKey").unwrap();
    let top = rematch.top.expect("alias must produce a match");
    assert_eq!(top.part_id, part);
    assert_eq!(top.verdict_kind, "known_alias");
}

// --- CreateNew: part+variant+listing created, receives shipped, price history

#[test]
fn create_new_creates_part_receives_shipped_and_writes_price_history() {
    let (_g, mut db, attachments) = open();
    let cat = category_id(&db, "Resistor");
    let parsed = invoice(
        "DigiKey",
        vec![part_line(
            1,
            Some("NEW-SKU"),
            Some("NEW-MPN"),
            Some("Acme"),
            Some(5),
            Some(5),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let line_id = line_id_by_sku(&mut db, &record.id, "NEW-SKU");

    let decision = LineDecision::CreateNew {
        draft: part_draft("New part", cat, QuantityUnit::Each),
        variant: variant_draft("Acme", "NEW-MPN"),
        listing: listing_draft("DigiKey", "NEW-SKU"),
    };
    let group = db
        .commit_import(&record.id, &[(line_id, decision)])
        .unwrap();
    assert_eq!(group.transactions.len(), 1);

    let part_id: String = db
        .raw_conn()
        .query_row(
            "SELECT part_id FROM part_aliases WHERE alias_kind='mpn' AND alias_value='NEW-MPN'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let part_id = PartId::from_string(part_id).unwrap();

    assert_eq!(
        db.get_stock(&part_id).unwrap().available,
        Quantity::from_whole(5).unwrap()
    );

    let (ph_count, ph_price): (i64, i64) = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*), MAX(unit_price_micros) FROM price_history WHERE part_id = ?1",
            [part_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(ph_count, 1);
    assert_eq!(ph_price, 1_000_000);

    let listing_price: Option<i64> = db
        .raw_conn()
        .query_row(
            "SELECT last_unit_price_micros FROM supplier_listings WHERE supplier_sku = 'NEW-SKU'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(listing_price, Some(1_000_000));
}

// --- AddAsVariant: second source on an existing part -------------------------

#[test]
fn add_as_variant_adds_second_source_and_receives_shipped() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "10k resistor");
    let v1 = variant(&mut db, &part, "Yageo", "RC0603FR-0710KL");
    listing(&mut db, &v1.id, "DigiKey", "311-10.0KLRCT-ND");

    let parsed = invoice(
        "Mouser",
        vec![part_line(
            1,
            Some("MOUSER-SKU"),
            Some("ALT-MPN"),
            Some("Vishay"),
            Some(6),
            Some(6),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let line_id = line_id_by_sku(&mut db, &record.id, "MOUSER-SKU");

    let decision = LineDecision::AddAsVariant {
        part_id: part.clone(),
        variant: variant_draft("Vishay", "ALT-MPN"),
        listing: listing_draft("Mouser", "MOUSER-SKU"),
    };
    db.commit_import(&record.id, &[(line_id, decision)])
        .unwrap();

    assert_eq!(
        db.get_stock(&part).unwrap().available,
        Quantity::from_whole(6).unwrap()
    );
    assert_eq!(db.list_variants(&part).unwrap().len(), 2);
}

// --- Skip / fully-backordered: no receive, part still created at zero stock -

#[test]
fn skip_and_fully_backordered_lines_create_no_receive() {
    let (_g, mut db, attachments) = open();
    let cat = category_id(&db, "Resistor");
    let existing = make_part(&mut db, "existing part");

    let parsed = invoice(
        "DigiKey",
        vec![
            part_line(1, Some("SKIP-SKU"), None, None, Some(3), Some(3)),
            part_line(
                2,
                Some("BACKORDER-SKU"),
                Some("BO-MPN"),
                Some("Acme"),
                Some(10),
                Some(0),
            ),
        ],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let skip_line = line_id_by_sku(&mut db, &record.id, "SKIP-SKU");
    let backorder_line = line_id_by_sku(&mut db, &record.id, "BACKORDER-SKU");

    let decisions = vec![
        (skip_line, LineDecision::Skip),
        (
            backorder_line,
            LineDecision::CreateNew {
                draft: part_draft("Backordered part", cat, QuantityUnit::Each),
                variant: variant_draft("Acme", "BO-MPN"),
                listing: listing_draft("DigiKey", "BACKORDER-SKU"),
            },
        ),
    ];
    let group = db.commit_import(&record.id, &decisions).unwrap();
    assert!(
        group.transactions.is_empty(),
        "no decided line shipped a positive quantity"
    );

    let commit_group_id: Option<String> = db
        .raw_conn()
        .query_row(
            "SELECT commit_group_id FROM imports WHERE id = ?1",
            [record.id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        commit_group_id.is_none(),
        "a receiveless commit must leave commit_group_id NULL"
    );

    let part_id: String = db
        .raw_conn()
        .query_row(
            "SELECT part_id FROM part_aliases WHERE alias_kind='mpn' AND alias_value='BO-MPN'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let part_id = PartId::from_string(part_id).unwrap();
    assert_eq!(
        db.get_stock(&part_id).unwrap().available,
        Quantity::ZERO,
        "a fully-backordered CreateNew line still creates its part, at zero stock"
    );
    assert_eq!(db.get_stock(&existing).unwrap().available, Quantity::ZERO);
    assert_eq!(
        db.get_import(&record.id).unwrap().unwrap().status,
        "committed"
    );
}

// --- Atomicity: a later failing line rolls back the ENTIRE commit -----------

#[test]
fn a_failing_line_rolls_back_the_entire_commit() {
    let (_g, mut db, attachments) = open();
    let cat = category_id(&db, "Resistor");
    let parsed = invoice(
        "DigiKey",
        vec![
            part_line(
                1,
                Some("GOOD-SKU"),
                Some("GOOD-MPN"),
                Some("Acme"),
                Some(4),
                Some(4),
            ),
            part_line(2, Some("BAD-SKU"), None, None, Some(2), Some(2)),
        ],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let good_line = line_id_by_sku(&mut db, &record.id, "GOOD-SKU");
    let bad_line = line_id_by_sku(&mut db, &record.id, "BAD-SKU");

    // The first decision is entirely valid (a real CreateNew); the SECOND
    // (not first, to prove genuine rollback, not a short-circuit) references
    // a part id that was never created -- AddStock must fail resolving it.
    let missing_part_id = PartId::new();
    let decisions = vec![
        (
            good_line,
            LineDecision::CreateNew {
                draft: part_draft("Should not persist", cat, QuantityUnit::Each),
                variant: variant_draft("Acme", "GOOD-MPN"),
                listing: listing_draft("DigiKey", "GOOD-SKU"),
            },
        ),
        (
            bad_line,
            LineDecision::AddStock {
                part_id: missing_part_id,
            },
        ),
    ];

    let err = db.commit_import(&record.id, &decisions).unwrap_err();
    assert!(matches!(err, DbError::PartNotFound));

    // Nothing from the batch persisted -- not even the earlier valid line.
    let part_count: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM parts WHERE display_name = 'Should not persist'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        part_count, 0,
        "the earlier CreateNew must not survive rollback"
    );
    let alias_count: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM part_aliases WHERE alias_value = 'GOOD-MPN'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(alias_count, 0);
    let txn_count: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(txn_count, 0, "no receive from either line may persist");
    let group_count: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM transaction_groups", [], |r| r.get(0))
        .unwrap();
    assert_eq!(group_count, 0);
    let ph_count: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ph_count, 0);

    assert_eq!(
        db.get_import(&record.id).unwrap().unwrap().status,
        "parsed",
        "a failed commit must leave the import exactly as it was"
    );
}

// --- Atomicity, second failure mode: a receive rejected mid-group -----------

#[test]
fn a_receive_against_an_archived_part_rolls_back_the_whole_commit() {
    let (_g, mut db, attachments) = open();
    let good_part = make_part(&mut db, "good part");
    let archived_part = make_part(&mut db, "archived part");
    db.set_part_archived(&archived_part, true).unwrap();

    let parsed = invoice(
        "DigiKey",
        vec![
            part_line(1, Some("GOOD-SKU-2"), None, None, Some(3), Some(3)),
            part_line(2, Some("ARCHIVED-SKU"), None, None, Some(1), Some(1)),
        ],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let good_line = line_id_by_sku(&mut db, &record.id, "GOOD-SKU-2");
    let archived_line = line_id_by_sku(&mut db, &record.id, "ARCHIVED-SKU");

    let decisions = vec![
        (
            good_line,
            LineDecision::AddStock {
                part_id: good_part.clone(),
            },
        ),
        (
            archived_line,
            LineDecision::AddStock {
                part_id: archived_part.clone(),
            },
        ),
    ];
    let err = db.commit_import(&record.id, &decisions).unwrap_err();
    assert!(matches!(err, DbError::PartArchived));

    assert_eq!(
        db.get_stock(&good_part).unwrap().available,
        Quantity::ZERO,
        "the earlier line's receive must roll back too"
    );
    assert_eq!(db.get_import(&record.id).unwrap().unwrap().status, "parsed");
}

// --- Reversal: stock returns, status flips, created parts survive at zero ---

#[test]
fn reverse_import_returns_stock_and_flips_status_but_keeps_created_parts() {
    let (_g, mut db, attachments) = open();
    let existing = make_part(&mut db, "10k resistor");
    let v = variant(&mut db, &existing, "Yageo", "RC0603FR-0710KL");
    listing(&mut db, &v.id, "DigiKey", "311-10.0KLRCT-ND");
    db.apply(&LedgerOp::Receive {
        part_id: existing.clone(),
        quantity: Quantity::from_whole(5).unwrap(),
        note: "pre-existing stock".into(),
    })
    .unwrap();

    let cat = category_id(&db, "Resistor");
    let parsed = invoice(
        "DigiKey",
        vec![
            part_line(1, Some("311-10.0KLRCT-ND"), None, None, Some(10), Some(10)),
            part_line(
                2,
                Some("NEW-SKU-3"),
                Some("NEW-MPN-3"),
                Some("Acme"),
                Some(3),
                Some(3),
            ),
        ],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let existing_line = line_id_by_sku(&mut db, &record.id, "311-10.0KLRCT-ND");
    let new_line = line_id_by_sku(&mut db, &record.id, "NEW-SKU-3");

    let decisions = vec![
        (
            existing_line,
            LineDecision::AddStock {
                part_id: existing.clone(),
            },
        ),
        (
            new_line,
            LineDecision::CreateNew {
                draft: part_draft("Created via import", cat, QuantityUnit::Each),
                variant: variant_draft("Acme", "NEW-MPN-3"),
                listing: listing_draft("DigiKey", "NEW-SKU-3"),
            },
        ),
    ];
    let group = db.commit_import(&record.id, &decisions).unwrap();
    assert_eq!(group.transactions.len(), 2);

    let new_part_id: String = db
        .raw_conn()
        .query_row(
            "SELECT part_id FROM part_aliases WHERE alias_kind='mpn' AND alias_value='NEW-MPN-3'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let new_part_id = PartId::from_string(new_part_id).unwrap();

    assert_eq!(
        db.get_stock(&existing).unwrap().available,
        Quantity::from_whole(15).unwrap()
    );
    assert_eq!(
        db.get_stock(&new_part_id).unwrap().available,
        Quantity::from_whole(3).unwrap()
    );

    let reversal = db.reverse_import(&record.id, "wrong shipment").unwrap();
    assert_eq!(reversal.transactions.len(), 2);
    assert_eq!(reversal.reversed_group_id.as_ref(), Some(&group.id));

    assert_eq!(
        db.get_stock(&existing).unwrap().available,
        Quantity::from_whole(5).unwrap(),
        "existing part's stock must return to its pre-import level"
    );
    assert_eq!(
        db.get_stock(&new_part_id).unwrap().available,
        Quantity::ZERO
    );

    // The part created by the import is NOT deleted -- it survives at zero.
    assert!(db.get_part(&new_part_id).unwrap().is_some());

    assert_eq!(
        db.get_import(&record.id).unwrap().unwrap().status,
        "reversed"
    );

    // The original commit group is itself marked reversed in history: a
    // direct reversal attempt on it now fails AlreadyReversed.
    let err = db
        .reverse_group(&group.id, "direct double reversal")
        .unwrap_err();
    assert!(matches!(err, DbError::AlreadyReversed));

    // And reversing the (already-reversed) import again is typed-rejected.
    let err = db.reverse_import(&record.id, "again").unwrap_err();
    assert!(matches!(err, DbError::ImportNotReversible));
}

#[test]
fn reversing_a_receiveless_commit_only_flips_status() {
    let (_g, mut db, attachments) = open();
    let cat = category_id(&db, "Resistor");
    let parsed = invoice(
        "DigiKey",
        vec![part_line(
            1,
            Some("BACKORDER-ONLY"),
            Some("BO-MPN-2"),
            Some("Acme"),
            Some(10),
            Some(0),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let line_id = line_id_by_sku(&mut db, &record.id, "BACKORDER-ONLY");

    let group = db
        .commit_import(
            &record.id,
            &[(
                line_id,
                LineDecision::CreateNew {
                    draft: part_draft("Backordered only", cat, QuantityUnit::Each),
                    variant: variant_draft("Acme", "BO-MPN-2"),
                    listing: listing_draft("DigiKey", "BACKORDER-ONLY"),
                },
            )],
        )
        .unwrap();
    assert!(group.transactions.is_empty());

    let reversal = db.reverse_import(&record.id, "never mind").unwrap();
    assert!(reversal.transactions.is_empty());
    assert!(reversal.reversed_group_id.is_none());
    assert_eq!(
        db.get_import(&record.id).unwrap().unwrap().status,
        "reversed"
    );
}

// --- Shipped-not-ordered ------------------------------------------------------

#[test]
fn commit_receives_shipped_not_ordered_quantity() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "partial ship");
    let v = variant(&mut db, &part, "Yageo", "MPN-1");
    listing(&mut db, &v.id, "DigiKey", "SKU-PARTIAL");

    let parsed = invoice(
        "DigiKey",
        vec![part_line(
            1,
            Some("SKU-PARTIAL"),
            None,
            None,
            Some(10),
            Some(8),
        )],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let line_id = line_id_by_sku(&mut db, &record.id, "SKU-PARTIAL");

    db.commit_import(
        &record.id,
        &[(
            line_id,
            LineDecision::AddStock {
                part_id: part.clone(),
            },
        )],
    )
    .unwrap();
    assert_eq!(
        db.get_stock(&part).unwrap().available,
        Quantity::from_whole(8).unwrap(),
        "must receive the SHIPPED amount, never the ordered amount"
    );
}

// --- Not-committable / not-reversible ----------------------------------------

#[test]
fn committing_an_already_committed_import_errors() {
    let (_g, mut db, attachments) = open();
    let part = make_part(&mut db, "p");
    let parsed = invoice(
        "DigiKey",
        vec![part_line(1, Some("SKU-A"), None, None, Some(1), Some(1))],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let line_id = line_id_by_sku(&mut db, &record.id, "SKU-A");
    db.commit_import(
        &record.id,
        &[(
            line_id.clone(),
            LineDecision::AddStock {
                part_id: part.clone(),
            },
        )],
    )
    .unwrap();

    let err = db
        .commit_import(
            &record.id,
            &[(line_id, LineDecision::AddStock { part_id: part })],
        )
        .unwrap_err();
    assert!(matches!(err, DbError::ImportNotCommittable));
}

#[test]
fn reversing_a_parsed_import_errors() {
    let (_g, mut db, attachments) = open();
    let parsed = invoice(
        "DigiKey",
        vec![part_line(1, Some("SKU-A"), None, None, Some(1), Some(1))],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let err = db.reverse_import(&record.id, "nope").unwrap_err();
    assert!(matches!(err, DbError::ImportNotReversible));
}

#[test]
fn committing_an_unknown_import_is_not_found() {
    let (_g, mut db, _attachments) = open();
    let err = db.commit_import(&ImportId::new(), &[]).unwrap_err();
    assert!(matches!(err, DbError::ImportNotFound));
}

#[test]
fn reversing_an_unknown_import_is_not_found() {
    let (_g, mut db, _attachments) = open();
    let err = db.reverse_import(&ImportId::new(), "").unwrap_err();
    assert!(matches!(err, DbError::ImportNotFound));
}

#[test]
fn a_decision_for_a_line_not_on_this_import_is_rejected() {
    let (_g, mut db, attachments) = open();
    let parsed = invoice(
        "DigiKey",
        vec![part_line(1, Some("SKU-A"), None, None, Some(1), Some(1))],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let part = make_part(&mut db, "p");
    let bogus_line = ImportLineId::new();
    let err = db
        .commit_import(
            &record.id,
            &[(bogus_line, LineDecision::AddStock { part_id: part })],
        )
        .unwrap_err();
    assert!(matches!(err, DbError::ImportLineNotFound));
}

// --- Alias dedup: a duplicate alias must NOT fail the whole commit ----------

#[test]
fn a_duplicate_alias_does_not_fail_the_commit() {
    let (_g, mut db, attachments) = open();
    let part_a = make_part(&mut db, "part a");
    // Simulates an earlier import already having claimed this SKU for part_a.
    db.add_alias("supplier_sku", "SKU-DUP", &part_a, "import")
        .unwrap();

    let part_b = make_part(&mut db, "part b");
    let parsed = invoice(
        "DigiKey",
        vec![part_line(1, Some("SKU-DUP"), None, None, Some(2), Some(2))],
    );
    let record = db
        .store_import(&attachments, &parsed, b"bytes", "a.csv")
        .unwrap();
    let line_id = line_id_by_sku(&mut db, &record.id, "SKU-DUP");

    // This commit resolves the line to a DIFFERENT part than the existing
    // alias -- it must still succeed; the alias insert is silently ignored.
    let group = db
        .commit_import(
            &record.id,
            &[(
                line_id,
                LineDecision::AddStock {
                    part_id: part_b.clone(),
                },
            )],
        )
        .unwrap();
    assert_eq!(group.transactions.len(), 1);
    assert_eq!(
        db.get_stock(&part_b).unwrap().available,
        Quantity::from_whole(2).unwrap()
    );

    let alias_part: String = db
        .raw_conn()
        .query_row(
            "SELECT part_id FROM part_aliases WHERE alias_kind='supplier_sku' AND alias_value='SKU-DUP'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        alias_part,
        part_a.as_str(),
        "the pre-existing alias must remain untouched (first writer wins)"
    );
}
