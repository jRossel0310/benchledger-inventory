//! Task 4 (Phase 2c): FTS-backed search execution with structured filters.
//!
//! One seeded scenario is shared by every test: 3 resistors (one archived,
//! one low-stock-thresholded, one reserved for a project), 1 op amp with a
//! manufacturer variant + supplier SKU + datasheet, 1 Meter-unit wire part
//! with a "Height" dimension, and 1 capacitor with a voltage_rating
//! attribute. Each test opens its own fresh database (existing repo
//! convention) and reseeds the scenario.

use inventory_core::ids::{CategoryId, PartId, ProjectId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::dimensions::{DimensionDraft, DimensionGroup, DimensionSource};
use inventory_db::parts::{ListingDraft, PartDraft, VariantDraft};
use inventory_db::search::SearchHit;
use inventory_db::{Database, DbError};

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
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

fn q(n: i64) -> Quantity {
    Quantity::from_whole(n).unwrap()
}

fn receive(db: &mut Database, part: &PartId, n: i64) {
    db.apply(&LedgerOp::Receive {
        part_id: part.clone(),
        quantity: q(n),
        note: String::new(),
    })
    .unwrap();
}

fn ids(hits: &[SearchHit]) -> Vec<PartId> {
    hits.iter().map(|h| h.part_id.clone()).collect()
}

/// Minimal-draft part creation for tests that need an extra part beyond the
/// shared `Scenario` (range/has/stock-column coverage below), mirroring
/// `seed`'s own `draft` closure fields.
fn make_part(db: &mut Database, name: &str, category: &str) -> PartId {
    let cat = category_id(db, category);
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

struct Scenario {
    resistor_10k: PartId, // bin A12, reserved for project, available ends up at 5
    resistor_4k7: PartId, // low-stock thresholded
    resistor_1k: PartId,  // archived
    op_amp: PartId,       // TLV9002IDDFR, SKU 296-TLV9002IDDFRCT-ND, datasheet
    wire: PartId,         // Height dimension, 5 mm
    capacitor: PartId,    // voltage_rating 35V
    #[allow(dead_code)]
    project_id: ProjectId,
}

fn seed(db: &mut Database) -> Scenario {
    let draft = |db: &Database,
                 name: &str,
                 category: &str,
                 unit: QuantityUnit,
                 bin: Option<&str>|
     -> PartDraft {
        PartDraft {
            display_name: name.to_string(),
            category_id: category_id(db, category),
            description: String::new(),
            bin_label: bin.map(|s| s.to_string()),
            usage_behavior: "usually_consumed".into(),
            quantity_unit: unit,
            low_stock_threshold: None,
            public_notes: String::new(),
            private_notes: String::new(),
        }
    };

    let resistor_10k = db
        .create_part(&draft(
            db,
            "10k 0603",
            "Resistor",
            QuantityUnit::Each,
            Some("A12"),
        ))
        .unwrap()
        .id;
    db.set_attribute(&resistor_10k, "resistance", "10k")
        .unwrap();

    let mut r2_draft = draft(db, "4k7 0603", "Resistor", QuantityUnit::Each, None);
    r2_draft.low_stock_threshold = Some(q(100));
    let resistor_4k7 = db.create_part(&r2_draft).unwrap().id;
    db.set_attribute(&resistor_4k7, "resistance", "4k7")
        .unwrap();

    let resistor_1k = db
        .create_part(&draft(db, "1k 0603", "Resistor", QuantityUnit::Each, None))
        .unwrap()
        .id;
    db.set_attribute(&resistor_1k, "resistance", "1k").unwrap();

    let op_amp = db
        .create_part(&draft(
            db,
            "Dual op amp",
            "Op amp",
            QuantityUnit::Each,
            None,
        ))
        .unwrap()
        .id;
    let variant = db
        .add_variant(
            &op_amp,
            &VariantDraft {
                manufacturer: "Texas Instruments".into(),
                mpn: "TLV9002IDDFR".into(),
                description: String::new(),
                package: Some("SOT-23-8".into()),
                datasheet_url: Some("https://www.ti.com/lit/ds/symlink/tlv9002.pdf".into()),
                product_url: None,
                lifecycle: None,
                notes: String::new(),
            },
        )
        .unwrap();
    db.add_supplier_listing(
        &variant.id,
        &ListingDraft {
            supplier: "Digikey".into(),
            supplier_sku: "296-TLV9002IDDFRCT-ND".into(),
            product_url: None,
            packaging: None,
            typical_order: None,
            last_unit_price_micros: None,
            currency: None,
            last_purchase_date: None,
        },
    )
    .unwrap();

    let wire = db
        .create_part(&draft(db, "Hookup wire", "Wire", QuantityUnit::Meter, None))
        .unwrap()
        .id;
    db.add_dimension(
        &wire,
        &DimensionDraft {
            group: DimensionGroup::Overall,
            name: "Height".into(),
            raw_value: "5 mm".into(),
            source: DimensionSource::Measured,
            notes: String::new(),
            measured_date: None,
        },
    )
    .unwrap();

    let capacitor = db
        .create_part(&draft(
            db,
            "100nF ceramic cap",
            "Capacitor",
            QuantityUnit::Each,
            None,
        ))
        .unwrap()
        .id;
    db.set_attribute(&capacitor, "voltage_rating", "35V")
        .unwrap();

    receive(db, &resistor_10k, 15);
    receive(db, &resistor_4k7, 50);
    receive(db, &op_amp, 1000);
    receive(db, &wire, 20);
    receive(db, &capacitor, 20);

    let project_id = db.create_project("Blinky Board").unwrap();
    db.apply(&LedgerOp::Reserve {
        part_id: resistor_10k.clone(),
        quantity: q(10),
        project_id: project_id.clone(),
    })
    .unwrap();

    db.set_part_archived(&resistor_1k, true).unwrap();

    Scenario {
        resistor_10k,
        resistor_4k7,
        resistor_1k,
        op_amp,
        wire,
        capacitor,
        project_id,
    }
}

#[test]
fn free_text_finds_by_display_name_not_unrelated_parts() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("10k").unwrap();
    let found = ids(&hits);
    assert!(found.contains(&s.resistor_10k));
    assert!(!found.contains(&s.op_amp));
    assert_eq!(
        found.len(),
        1,
        "only the 10k resistor should match, got {found:?}"
    );
}

#[test]
fn free_text_matches_mpn() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("TLV9002").unwrap();
    assert_eq!(ids(&hits), vec![s.op_amp]);
}

#[test]
fn free_text_matches_supplier_sku() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("296-TLV9002IDDFRCT-ND").unwrap();
    assert_eq!(ids(&hits), vec![s.op_amp]);
}

#[test]
fn bin_filter_matches_exact_label() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("bin:A12").unwrap();
    assert_eq!(ids(&hits), vec![s.resistor_10k]);
}

#[test]
fn available_lt_filter() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("available:<10").unwrap();
    assert_eq!(ids(&hits), vec![s.resistor_10k]);
}

// NOTE: the seeded attribute key is `voltage_rating` (matching seed.rs's
// curated Capacitor attributes), not the shorthand `voltage` used in the
// task brief's prose — see report deviations.
#[test]
fn voltage_rating_ge_filter_on_capacitor() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("voltage_rating:>=25V").unwrap();
    assert_eq!(ids(&hits), vec![s.capacitor]);
}

#[test]
fn height_dimension_filter() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("height:<10mm").unwrap();
    assert_eq!(ids(&hits), vec![s.wire]);
    let hits = db.search("height:<3mm").unwrap();
    assert!(hits.is_empty(), "5mm is not < 3mm, got {hits:?}");
}

#[test]
fn low_stock_flag_matches_only_thresholded_part_below_threshold() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("low stock").unwrap();
    assert_eq!(ids(&hits), vec![s.resistor_4k7]);
}

#[test]
fn archived_excluded_by_default_and_included_via_is_archived() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("").unwrap();
    let found = ids(&hits);
    assert!(!found.contains(&s.resistor_1k));
    assert_eq!(found.len(), 5, "6 seeded parts minus 1 archived");

    let archived_hits = db.search("is:archived").unwrap();
    assert_eq!(ids(&archived_hits), vec![s.resistor_1k]);
}

#[test]
fn project_filter_finds_reserved_part() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("project:Blinky").unwrap();
    assert_eq!(ids(&hits), vec![s.resistor_10k]);
}

#[test]
fn has_datasheet_filter() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("has:datasheet").unwrap();
    assert_eq!(ids(&hits), vec![s.op_amp]);
}

#[test]
fn has_footprint_is_unsupported() {
    let (_g, mut db) = open();
    seed(&mut db);
    let err = db.search("has:footprint").unwrap_err();
    assert!(
        matches!(err, DbError::UnsupportedSearchKey(_)),
        "got {err:?}"
    );
}

#[test]
fn unknown_filter_key_is_typed_error() {
    let (_g, mut db) = open();
    seed(&mut db);
    let err = db.search("nonsense:5").unwrap_err();
    assert!(matches!(err, DbError::UnknownSearchKey(_)), "got {err:?}");
}

#[test]
fn search_results_are_deterministic() {
    let (_g, mut db) = open();
    seed(&mut db);
    let a = db.search("").unwrap();
    let b = db.search("").unwrap();
    assert_eq!(ids(&a), ids(&b));
    // Also deterministic through the FTS/bm25 ranking path.
    let a = db.search("0603").unwrap();
    let b = db.search("0603").unwrap();
    assert_eq!(ids(&a), ids(&b));
}

/// Critical implementation note: FTS MATCH input must be sanitized so a
/// quote/paren-bearing free-text term cannot raise an FTS5 syntax error.
#[test]
fn special_characters_in_free_text_do_not_crash_the_query() {
    let (_g, mut db) = open();
    seed(&mut db);
    let result = db.search("foo\"bar (baz) 'qux' AND OR NOT *");
    assert!(
        result.is_ok(),
        "special characters must not break FTS MATCH parsing: {result:?}"
    );
}

// Bonus coverage beyond the brief's enumerated list, exercising the
// remaining documented-but-not-explicitly-tested filter kinds.
#[test]
fn category_filter_matches_case_insensitively() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("category:capacitor").unwrap();
    assert_eq!(ids(&hits), vec![s.capacitor]);
}

#[test]
fn stock_total_filter_sums_all_three_states() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    // op amp: 1000 available, everything else received <= 50.
    let hits = db.search("stock:>500").unwrap();
    assert_eq!(ids(&hits), vec![s.op_amp]);
}

// Named-spec §8 `..` RANGE operator, plus `has:dimensions` and the
// `reserved:`/`checked_out:` stock-column filters, which were implemented
// but had no end-to-end `search()` coverage.

#[test]
fn range_filter_matches_only_values_in_range() {
    let (_g, mut db) = open();
    seed(&mut db);
    let cap_in_range = make_part(&mut db, "100nF X7R cap", "Capacitor");
    db.set_attribute(&cap_in_range, "capacitance", "100nF")
        .unwrap();
    let cap_out_of_range = make_part(&mut db, "10pF C0G cap", "Capacitor");
    db.set_attribute(&cap_out_of_range, "capacitance", "10pF")
        .unwrap();

    let hits = db.search("capacitance:10nF..1uF").unwrap();
    assert_eq!(
        ids(&hits),
        vec![cap_in_range],
        "100nF is within 10nF..1uF, 10pF is not, got {hits:?}"
    );
}

#[test]
fn dimension_range_filter_works() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let big_wire = make_part(&mut db, "Thick wire", "Wire");
    db.add_dimension(
        &big_wire,
        &DimensionDraft {
            group: DimensionGroup::Overall,
            name: "Height".into(),
            raw_value: "20 mm".into(),
            source: DimensionSource::Measured,
            notes: String::new(),
            measured_date: None,
        },
    )
    .unwrap();

    let hits = db.search("height:1mm..8mm").unwrap();
    assert_eq!(
        ids(&hits),
        vec![s.wire],
        "5mm wire is within 1mm..8mm, 20mm wire is not, got {hits:?}"
    );
}

#[test]
fn has_dimensions_filter_works() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    let hits = db.search("has:dimensions").unwrap();
    assert_eq!(ids(&hits), vec![s.wire]);
}

#[test]
fn reserved_and_checked_out_stock_filters_work() {
    let (_g, mut db) = open();
    let s = seed(&mut db);
    // seed() already reserves 10 of resistor_10k for the Blinky Board
    // project; additionally check out some op-amp stock to a second
    // project so reserved: and checked_out: pick out different parts.
    let checkout_project = db.create_project("Checkout project").unwrap();
    db.apply(&LedgerOp::CheckOut {
        part_id: s.op_amp.clone(),
        quantity: q(5),
        project_id: checkout_project,
    })
    .unwrap();

    let reserved_hits = db.search("reserved:>0").unwrap();
    assert_eq!(ids(&reserved_hits), vec![s.resistor_10k]);

    let checked_out_hits = db.search("checked_out:>=1").unwrap();
    assert_eq!(ids(&checked_out_hits), vec![s.op_amp]);
}
