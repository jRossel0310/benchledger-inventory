//! Phase 6 Task 1 gate tests for the public snapshot builder. The exclusion
//! test is the most important test in this file (and arguably in the whole
//! phase): the snapshot it builds from is committed to a PUBLIC GitHub repo,
//! so anything that leaks here leaks to the world.

use inventory_core::ids::{CategoryId, PartId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::dimensions::{DimensionDraft, DimensionGroup, DimensionSource};
use inventory_db::parts::{ListingDraft, PartDraft, VariantDraft};
use inventory_db::{dev_seed, Database, MISC_CATEGORY_ID};
use inventory_import::{LineKind, Money, ParsedInvoice, ParsedLine, ParsedOrderMeta, SourceFormat};
use inventory_sync::snapshot::{build_snapshot, content_digest, to_canonical_json};

fn open_seeded_db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let mut db =
        Database::open_and_migrate(&dir.path().join("t.sqlite"), &dir.path().join("backups"))
            .unwrap();
    dev_seed::run(&mut db).unwrap();
    (dir, db)
}

/// Everything this file's exclusion test must prove never leaks: a part
/// whose `private_notes` carries a marker string, a supplier listing with a
/// price, an archived part, and a full raw-invoice import row (order number,
/// line items, the works). Added on top of `dev_seed`'s representative
/// dataset via the same public `Database` methods the app itself uses — no
/// direct SQL — so this can never construct a state the real app couldn't
/// also reach.
struct PrivacySensitiveExtras {
    private_notes_part_id: PartId,
    archived_part_id: PartId,
    archived_part_name: &'static str,
}

fn add_privacy_sensitive_extras(
    attachments_root: &std::path::Path,
    db: &mut Database,
) -> PrivacySensitiveExtras {
    let misc = CategoryId::from_string(MISC_CATEGORY_ID.to_string()).unwrap();

    // A part carrying a private note that must never be published.
    let private_part = db
        .create_part(&PartDraft {
            display_name: "Confidential test fixture part".into(),
            category_id: misc.clone(),
            description: "part used to prove a confidential field never leaks".into(),
            bin_label: Some("Z1".into()),
            usage_behavior: "usually_consumed".into(),
            quantity_unit: QuantityUnit::Each,
            low_stock_threshold: None,
            public_notes: "a totally public note".into(),
            private_notes: "SECRET-PRIVATE-NOTE-XYZ".into(),
        })
        .unwrap();

    // A supplier listing with a price attached to a fresh variant on that
    // same part -- proves prices are excluded even for an otherwise-included
    // (non-archived) part. The variant also carries a marker in its `notes`
    // (variant notes are internal working text, excluded from the snapshot)
    // so the value scan below proves the exclusion holds by value, not just
    // by the `notes` field being absent from `SnapshotVariant`.
    let priced_variant = db
        .add_variant(
            &private_part.id,
            &VariantDraft {
                manufacturer: "Acme".into(),
                mpn: "ACME-001".into(),
                description: String::new(),
                package: None,
                datasheet_url: None,
                product_url: None,
                lifecycle: None,
                notes: "SECRET-VARIANT-NOTE-QQQ".into(),
            },
        )
        .unwrap();
    db.add_supplier_listing(
        &priced_variant.id,
        &ListingDraft {
            supplier: "DigiKey".into(),
            supplier_sku: "296-LISTING-ND".into(),
            product_url: None,
            packaging: Some("Cut Tape".into()),
            typical_order: None,
            last_unit_price_micros: Some(1_234_567_890),
            currency: Some("USD".into()),
            last_purchase_date: Some("2026-01-01".into()),
        },
    )
    .unwrap();

    // A dimension with a marker in its `notes` (dimension notes record
    // internal measurement context, excluded from the snapshot) -- same
    // by-value proof as the variant note above. The dimension itself
    // (name/value) IS public and must appear.
    db.add_dimension(
        &private_part.id,
        &DimensionDraft {
            group: DimensionGroup::Overall,
            name: "Depth".into(),
            raw_value: "5 mm".into(),
            source: DimensionSource::Measured,
            notes: "SECRET-DIMENSION-NOTE-RRR".into(),
            measured_date: None,
        },
    )
    .unwrap();

    // An archived part -- must be entirely absent from the snapshot, name
    // and id both.
    let archived_part_name = "ARCHIVED-PART-ZZZ";
    let archived = db
        .create_part(&PartDraft {
            display_name: archived_part_name.into(),
            category_id: misc,
            description: String::new(),
            bin_label: Some("Z9".into()),
            usage_behavior: "usually_consumed".into(),
            quantity_unit: QuantityUnit::Each,
            low_stock_threshold: None,
            public_notes: String::new(),
            private_notes: String::new(),
        })
        .unwrap();
    db.set_part_archived(&archived.id, true).unwrap();

    // A raw invoice import -- nothing import-related should ever appear.
    let attachments_dir = attachments_root.join("attachments");
    std::fs::create_dir_all(&attachments_dir).unwrap();
    let invoice = ParsedInvoice {
        supplier: "DigiKey".into(),
        source_format: SourceFormat::Csv,
        order: ParsedOrderMeta {
            order_number: Some("SECRET-ORDER-99".into()),
            invoice_number: None,
            shipment_number: None,
            order_date: Some("2026-01-01".into()),
            currency: "USD".into(),
            subtotal: Money::parse("1.00", "USD"),
            shipping: None,
            tax: None,
            tariff: None,
            total: Money::parse("1.00", "USD"),
            web_order_id: None,
        },
        lines: vec![ParsedLine {
            line_number: Some(1),
            supplier_sku: Some("296-1234-ND".into()),
            mpn: Some("TPS7A4700RGWR".into()),
            manufacturer: Some("Texas Instruments".into()),
            description: Some("secret invoice line".into()),
            ordered: Some(Quantity::from_whole(1).unwrap()),
            shipped: Some(Quantity::from_whole(1).unwrap()),
            backordered: None,
            unit_price: Money::parse("1.00", "USD"),
            extended_price: Money::parse("1.00", "USD"),
            packaging: None,
            customer_reference: None,
            kind: LineKind::Part,
            confidence: 1.0,
            raw: serde_json::json!({"note": "raw invoice text"}),
        }],
        warnings: vec![],
    };
    db.store_import(&attachments_dir, &invoice, b"raw pdf bytes", "invoice.csv")
        .unwrap();

    PrivacySensitiveExtras {
        private_notes_part_id: private_part.id,
        archived_part_id: archived.id,
        archived_part_name,
    }
}

#[test]
fn snapshot_excludes_all_private_and_pricing_data() {
    let (dir, mut db) = open_seeded_db();
    let extras = add_privacy_sensitive_extras(dir.path(), &mut db);

    let snapshot = build_snapshot(&db).unwrap();
    let digest_json = to_canonical_json(&snapshot, None);
    let publish_json = to_canonical_json(&snapshot, Some("2026-07-19T00:00:00Z"));

    for json in [&digest_json, &publish_json] {
        let lower = json.to_lowercase();
        // (a) key denylist -- scanned as plain substrings, not just JSON
        // keys, so this also catches the forbidden word appearing inside a
        // value.
        for forbidden in [
            "private_notes",
            "price",
            "micros",
            "token",
            "client_id",
            "secret",
            "import",
            "invoice",
            "attachment",
            "provenance",
            "last_purchase",
        ] {
            assert!(
                !lower.contains(forbidden),
                "snapshot must never contain the substring '{forbidden}':\n{json}"
            );
        }

        // (b) value scan -- known-private seeded values.
        assert!(!json.contains("SECRET-PRIVATE-NOTE-XYZ"));
        assert!(
            !json.contains("SECRET-VARIANT-NOTE-QQQ"),
            "variant notes must not be published"
        );
        assert!(
            !json.contains("SECRET-DIMENSION-NOTE-RRR"),
            "dimension notes must not be published"
        );
        assert!(!json.contains(extras.archived_part_name));
        assert!(!json.contains("SECRET-ORDER-99"));
        assert!(
            !json.contains("1234567890"),
            "the planted price's digits must not appear"
        );
        assert!(!json.contains("C:\\"));
        assert!(!json.contains("AppData"));

        // (c) the archived part's id is absent.
        assert!(!json.contains(extras.archived_part_id.as_str()));
    }

    // Sanity: the private-notes-bearing part IS included (just not its
    // private_notes value) -- proves the exclusion above isn't vacuously
    // true because build_snapshot dropped the whole part.
    assert!(digest_json.contains(extras.private_notes_part_id.as_str()));
    assert!(digest_json.contains("a totally public note"));
    assert!(
        digest_json.contains("296-LISTING-ND"),
        "the SKU itself is public"
    );
    assert!(
        digest_json.contains("\"Depth\""),
        "the note-bearing dimension itself is public -- only its notes are excluded"
    );
    assert!(
        digest_json.contains("ACME-001"),
        "the note-bearing variant itself is public -- only its notes are excluded"
    );
}

#[test]
fn build_is_byte_identical_across_two_builds() {
    let (_dir, db) = open_seeded_db();
    let snap1 = build_snapshot(&db).unwrap();
    let snap2 = build_snapshot(&db).unwrap();
    assert_eq!(snap1, snap2);
    assert_eq!(
        to_canonical_json(&snap1, None),
        to_canonical_json(&snap2, None)
    );
}

#[test]
fn different_published_at_yields_the_same_content_digest() {
    let (_dir, db) = open_seeded_db();
    let snapshot = build_snapshot(&db).unwrap();

    let json_a = to_canonical_json(&snapshot, Some("2026-01-01T00:00:00Z"));
    let json_b = to_canonical_json(&snapshot, Some("2099-12-31T23:59:59Z"));
    assert_ne!(json_a, json_b, "the publish forms themselves must differ");

    // content_digest never takes a published_at at all -- it is a pure
    // function of the snapshot's content, called twice here to also confirm
    // it's deterministic on repeat calls.
    assert_eq!(content_digest(&snapshot), content_digest(&snapshot));
}

#[test]
fn mutating_stock_changes_the_digest() {
    let (_dir, mut db) = open_seeded_db();
    let before = build_snapshot(&db).unwrap();
    let digest_before = content_digest(&before);

    let part_id = PartId::from_string(before.parts[0].id.clone()).unwrap();
    db.apply(&LedgerOp::Receive {
        part_id,
        quantity: Quantity::from_whole(1).unwrap(),
        note: "test receive".into(),
    })
    .unwrap();

    let after = build_snapshot(&db).unwrap();
    let digest_after = content_digest(&after);
    assert_ne!(
        digest_before, digest_after,
        "a stock mutation must change the published digest"
    );
}

#[test]
fn canonical_json_is_lf_only_with_exactly_one_trailing_newline() {
    let (_dir, db) = open_seeded_db();
    let snapshot = build_snapshot(&db).unwrap();

    for json in [
        to_canonical_json(&snapshot, None),
        to_canonical_json(&snapshot, Some("2026-07-19T00:00:00Z")),
    ] {
        assert!(!json.contains('\r'), "no CRLF anywhere");
        assert!(json.ends_with('\n'), "must end with a trailing LF");
        assert!(
            !json.ends_with("\n\n"),
            "exactly one trailing newline, not more"
        );
        // 2-space indent sanity check: the top-level `schema_version` key is
        // one level deep.
        assert!(json.contains("\n  \"schema_version\": "));
    }
}

#[test]
fn published_at_is_present_only_in_the_publish_form() {
    let (_dir, db) = open_seeded_db();
    let snapshot = build_snapshot(&db).unwrap();

    let digest_form = to_canonical_json(&snapshot, None);
    assert!(!digest_form.contains("published_at"));

    let publish_form = to_canonical_json(&snapshot, Some("2026-07-19T12:00:00Z"));
    assert!(publish_form.contains("\"published_at\": \"2026-07-19T12:00:00Z\""));
}

#[test]
fn snapshot_includes_expected_part_and_project_shape() {
    let (_dir, db) = open_seeded_db();
    let snapshot = build_snapshot(&db).unwrap();

    let resistor = snapshot
        .parts
        .iter()
        .find(|p| p.display_name == "10k 0603 1% resistor")
        .expect("seeded resistor must be present");
    assert_eq!(resistor.category, "Resistor");
    assert_eq!(resistor.bin.as_deref(), Some("A10"));
    assert_eq!(resistor.quantity_unit, "each");
    assert_eq!(resistor.low_stock_threshold_milli, Some(100_000));
    // dev_seed receives 500, then reserves 50 for the Blinky Board project:
    // available drops by the reserved amount, reserved rises by it.
    assert_eq!(resistor.stock.available_milli, 450_000);
    assert_eq!(
        resistor.stock.reserved_milli, 50_000,
        "seeded reservation of 50"
    );

    let resistance = resistor
        .attributes
        .iter()
        .find(|a| a.key == "resistance")
        .expect("resistance attribute must be present");
    assert_eq!(resistance.label, "Resistance");
    assert_eq!(resistance.original_text, "10k");
    assert!((resistance.normalized_value.unwrap() - 10_000.0).abs() < 1e-9);
    assert_eq!(resistance.canonical_unit.as_deref(), Some("Ω"));

    let variant = resistor
        .variants
        .iter()
        .find(|v| v.mpn == "RC0603FR-0710KL")
        .expect("seeded variant must be present");
    assert_eq!(variant.manufacturer, "Yageo");
    assert!(variant.is_preferred);
    let listing = variant
        .listings
        .first()
        .expect("seeded listing must be present");
    assert_eq!(listing.supplier, "DigiKey");
    assert_eq!(listing.supplier_sku, "311-10.0KLRCT-ND");
    assert_eq!(listing.packaging.as_deref(), Some("Cut Tape"));

    let dev_board = snapshot
        .parts
        .iter()
        .find(|p| p.display_name == "Uno-style development board")
        .expect("seeded dev board must be present");
    assert!(dev_board
        .dimensions
        .iter()
        .any(|d| d.name == "Length" && d.display_unit == "mm"));

    let blinky = snapshot
        .projects
        .iter()
        .find(|p| p.name == "Blinky Board")
        .expect("seeded project must be present");
    assert_eq!(blinky.status, "active");
    assert_eq!(blinky.build_quantity, 5);
    assert!(!blinky.part_ids.is_empty());
    assert!(blinky.part_ids.contains(&resistor.id));
}

#[test]
fn parts_are_sorted_by_id_not_by_the_db_layer_display_name_order() {
    let (_dir, db) = open_seeded_db();
    let by_display_name = db.list_parts(false).unwrap(); // ORDER BY display_name
    let snapshot = build_snapshot(&db).unwrap();

    let snapshot_ids: Vec<&str> = snapshot.parts.iter().map(|p| p.id.as_str()).collect();
    let mut sorted_ids = snapshot_ids.clone();
    sorted_ids.sort();
    assert_eq!(snapshot_ids, sorted_ids, "snapshot parts must be id-sorted");

    let name_order_ids: Vec<&str> = by_display_name.iter().map(|p| p.id.as_str()).collect();
    assert_ne!(
        snapshot_ids, name_order_ids,
        "the id-sorted order must differ from the DB's own display_name order for this \
         dataset, proving build_snapshot actually re-sorts rather than passing list_parts' \
         ordering straight through"
    );
}

#[test]
fn archived_parts_and_reservations_do_not_break_the_snapshot() {
    let (_dir, db) = open_seeded_db();
    let snapshot = build_snapshot(&db).unwrap();
    // dev_seed archives "100uF radial electrolytic capacitor".
    assert!(!snapshot
        .parts
        .iter()
        .any(|p| p.display_name == "100uF radial electrolytic capacitor"));
}
