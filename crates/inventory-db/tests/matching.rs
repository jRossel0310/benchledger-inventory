//! Task 5 (Phase 2c): duplicate matching engine and decision memory.
//!
//! Scenario helpers build resistors/capacitors/op-amps with their identity
//! attributes set directly (mirrors `tests/identity.rs`'s pattern) so each
//! test can control exactly which identity fields match, conflict, or are
//! missing.

use inventory_core::ids::{CategoryId, PartId};
use inventory_core::quantity::QuantityUnit;
use inventory_db::matching::MatchCandidate;
use inventory_db::parts::{ListingDraft, PartDraft, VariantDraft};
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

fn resistor(
    db: &mut Database,
    name: &str,
    resistance: &str,
    tolerance: &str,
    power_rating: &str,
    package: &str,
) -> PartId {
    let id = make_part(db, name, "Resistor");
    db.set_attribute(&id, "resistance", resistance).unwrap();
    db.set_attribute(&id, "tolerance", tolerance).unwrap();
    db.set_attribute(&id, "power_rating", power_rating).unwrap();
    db.set_attribute(&id, "package", package).unwrap();
    id
}

#[allow(clippy::too_many_arguments)]
fn capacitor(
    db: &mut Database,
    name: &str,
    capacitance: &str,
    dielectric: &str,
    voltage_rating: Option<&str>,
    tolerance: &str,
    package: &str,
) -> PartId {
    let id = make_part(db, name, "Capacitor");
    db.set_attribute(&id, "capacitance", capacitance).unwrap();
    db.set_attribute(&id, "dielectric", dielectric).unwrap();
    if let Some(v) = voltage_rating {
        db.set_attribute(&id, "voltage_rating", v).unwrap();
    }
    db.set_attribute(&id, "tolerance", tolerance).unwrap();
    db.set_attribute(&id, "package", package).unwrap();
    id
}

fn op_amp(db: &mut Database, name: &str, channel_count: &str, package: &str) -> PartId {
    let id = make_part(db, name, "Op amp");
    db.set_attribute(&id, "channel_count", channel_count)
        .unwrap();
    db.set_attribute(&id, "package", package).unwrap();
    id
}

fn variant(
    db: &mut Database,
    part_id: &PartId,
    manufacturer: &str,
    mpn: &str,
) -> inventory_db::parts::VariantRecord {
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

// --- Passive exact identity -------------------------------------------------

#[test]
fn exact_passive_match_across_manufacturers_reaches_exact_identity() {
    let (_g, mut db) = open();
    let a = resistor(&mut db, "10k 0603 A", "10k", "1%", "1/4 W", "0603");
    let b = resistor(
        &mut db,
        "10k 0603 B",
        "10000 ohm",
        "1 %",
        "0.25W",
        "1608 metric",
    );
    variant(&mut db, &a, "Yageo", "RC0603FR-0710KL");
    variant(&mut db, &b, "Panasonic", "ERJ-3EKF1002V");

    let matches = db.suggest_duplicates(&a).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == b)
        .expect("b should be suggested as a duplicate of a");
    assert_eq!(hit.verdict_kind, "exact_identity");
    assert_eq!(hit.rank, 4);
    let lower = hit.explanation.to_lowercase();
    assert!(
        lower.contains("all match"),
        "explanation: {}",
        hit.explanation
    );
    for word in ["resistance", "tolerance", "power", "package"] {
        assert!(
            lower.contains(word),
            "explanation missing '{word}': {}",
            hit.explanation
        );
    }
}

// --- Probable equivalent: one missing field ---------------------------------

#[test]
fn missing_identity_field_yields_probable_equivalent() {
    let (_g, mut db) = open();
    let a = capacitor(
        &mut db,
        "100nF A",
        "100nF",
        "X7R",
        Some("25V"),
        "10%",
        "0603",
    );
    let b = capacitor(&mut db, "100nF B", "100nF", "X7R", None, "10%", "0603");

    let matches = db.suggest_duplicates(&a).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == b)
        .expect("b should be suggested as a probable duplicate of a");
    assert_eq!(hit.verdict_kind, "probable_equivalent");
    assert_eq!(hit.rank, 5);
    let lower = hit.explanation.to_lowercase();
    assert!(
        lower.contains("voltage"),
        "explanation: {}",
        hit.explanation
    );
    assert!(
        lower.contains("missing"),
        "explanation: {}",
        hit.explanation
    );
}

// --- Similar: one conflicting field, rest matching --------------------------

#[test]
fn different_voltage_rating_yields_similar() {
    let (_g, mut db) = open();
    let a = capacitor(
        &mut db,
        "100nF A",
        "100nF",
        "X7R",
        Some("25V"),
        "10%",
        "0603",
    );
    let b = capacitor(
        &mut db,
        "100nF B",
        "100nF",
        "X7R",
        Some("50V"),
        "10%",
        "0603",
    );

    let matches = db.suggest_duplicates(&a).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == b)
        .expect("b should be suggested as similar to a");
    assert_eq!(hit.verdict_kind, "similar");
    assert_eq!(hit.rank, 6);
    let lower = hit.explanation.to_lowercase();
    assert!(
        lower.contains("voltage"),
        "explanation: {}",
        hit.explanation
    );
    assert!(lower.contains("differ"), "explanation: {}", hit.explanation);
}

#[test]
fn different_dielectric_yields_similar() {
    let (_g, mut db) = open();
    let a = capacitor(
        &mut db,
        "100nF A",
        "100nF",
        "X7R",
        Some("25V"),
        "10%",
        "0603",
    );
    let b = capacitor(
        &mut db,
        "100nF B",
        "100nF",
        "X5R",
        Some("25V"),
        "10%",
        "0603",
    );

    let matches = db.suggest_duplicates(&a).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == b)
        .expect("b should be suggested as similar to a");
    assert_eq!(hit.verdict_kind, "similar");
    assert_eq!(hit.rank, 6);
    let lower = hit.explanation.to_lowercase();
    assert!(
        lower.contains("dielectric"),
        "explanation: {}",
        hit.explanation
    );
    assert!(lower.contains("differ"), "explanation: {}", hit.explanation);
}

#[test]
fn different_package_yields_similar() {
    let (_g, mut db) = open();
    let a = capacitor(
        &mut db,
        "100nF A",
        "100nF",
        "X7R",
        Some("25V"),
        "10%",
        "0603",
    );
    let b = capacitor(
        &mut db,
        "100nF B",
        "100nF",
        "X7R",
        Some("25V"),
        "10%",
        "0805",
    );

    let matches = db.suggest_duplicates(&a).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == b)
        .expect("b should be suggested as similar to a");
    assert_eq!(hit.verdict_kind, "similar");
    assert_eq!(hit.rank, 6);
    assert_eq!(hit.explanation, "Similar device, but package differs");
}

// --- Actives never auto-combine ---------------------------------------------

#[test]
fn active_category_never_reaches_exact_identity() {
    let (_g, mut db) = open();
    let a = op_amp(&mut db, "Dual op amp A", "2", "SOIC-8");
    let b = op_amp(&mut db, "Dual op amp B", "2", "SOIC-8");

    let matches = db.suggest_duplicates(&a).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == b)
        .expect("b should be suggested for a, capped below exact identity");
    assert_ne!(
        hit.verdict_kind, "exact_identity",
        "active/IC categories must never auto-combine on identity match"
    );
    assert_eq!(hit.verdict_kind, "probable_equivalent");
    assert_eq!(hit.rank, 5);
}

// --- Decision memory ---------------------------------------------------------

#[test]
fn approved_equivalence_ranks_as_known_alias() {
    let (_g, mut db) = open();
    // Deliberately differ on package so, absent the approval, this would
    // only be "similar" — proves the approved decision short-circuits
    // identity computation rather than merely tagging its result.
    let a = resistor(&mut db, "10k A", "10k", "1%", "1/4 W", "0603");
    let b = resistor(&mut db, "10k B", "10k", "1%", "1/4 W", "0805");
    db.record_equivalence(&a, &b, "approved", "confirmed same by datasheet")
        .unwrap();

    let matches = db.suggest_duplicates(&a).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == b)
        .expect("approved pair should be suggested");
    assert_eq!(hit.verdict_kind, "known_alias");
    assert_eq!(hit.rank, 3);
    assert_eq!(hit.explanation, "Previously approved as equivalent");
}

#[test]
fn rejected_equivalence_is_never_suggested() {
    let (_g, mut db) = open();
    // a and b are a full exact-identity match; without the rejection this
    // would be rank 4.
    let a = resistor(&mut db, "10k A", "10k", "1%", "1/4 W", "0603");
    let b = resistor(&mut db, "10k B", "10k", "1%", "1/4 W", "0603");
    db.record_equivalence(&a, &b, "rejected", "different manufacturer, keep separate")
        .unwrap();

    let matches = db.suggest_duplicates(&a).unwrap();
    assert!(
        matches.iter().all(|m| m.part_id != b),
        "a rejected pair must never be suggested, even though it's a full identity match: {matches:?}"
    );
}

// --- find_matches hierarchy ---------------------------------------------------

#[test]
fn candidate_exact_sku_beats_identity() {
    let (_g, mut db) = open();
    let p = resistor(&mut db, "10k 0603", "10k", "1%", "1/4 W", "0603");
    let v = variant(&mut db, &p, "Yageo", "RC0603FR-0710KL");
    db.add_supplier_listing(
        &v.id,
        &ListingDraft {
            supplier: "DigiKey".into(),
            supplier_sku: "311-10.0KLRCT-ND".into(),
            product_url: None,
            packaging: None,
            typical_order: None,
            last_unit_price_micros: None,
            currency: None,
            last_purchase_date: None,
        },
    )
    .unwrap();

    // Candidate matches p both by SKU (case-insensitive) and by a fully
    // exact identity match; SKU (rank 1) must win over identity (rank 4).
    let candidate = MatchCandidate {
        supplier: Some("DigiKey".into()),
        supplier_sku: Some("311-10.0klrct-nd".into()),
        category_id: Some(category_id(&db, "Resistor")),
        attributes: vec![
            ("resistance".into(), "10k".into()),
            ("tolerance".into(), "1%".into()),
            ("power_rating".into(), "1/4 W".into()),
            ("package".into(), "0603".into()),
        ],
        ..Default::default()
    };

    let matches = db.find_matches(&candidate).unwrap();
    assert_eq!(
        matches.len(),
        1,
        "candidate reaches p through two levels but must be reported once: {matches:?}"
    );
    assert_eq!(matches[0].part_id, p);
    assert_eq!(matches[0].verdict_kind, "exact_sku");
    assert_eq!(matches[0].rank, 1);
}

#[test]
fn exact_sku_beats_exact_mpn() {
    let (_g, mut db) = open();
    let p = resistor(&mut db, "10k 0603", "10k", "1%", "1/4 W", "0603");
    let v = variant(&mut db, &p, "Yageo", "RC0603FR-0710KL");
    db.add_supplier_listing(
        &v.id,
        &ListingDraft {
            supplier: "DigiKey".into(),
            supplier_sku: "311-10.0KLRCT-ND".into(),
            product_url: None,
            packaging: None,
            typical_order: None,
            last_unit_price_micros: None,
            currency: None,
            last_purchase_date: None,
        },
    )
    .unwrap();

    // Candidate matches the same variant both by SKU and by MPN; SKU
    // (rank 1) must win over MPN (rank 2).
    let candidate = MatchCandidate {
        supplier: Some("DigiKey".into()),
        supplier_sku: Some("311-10.0KLRCT-ND".into()),
        manufacturer: Some("Yageo".into()),
        mpn: Some("RC0603FR-0710KL".into()),
        ..Default::default()
    };

    let matches = db.find_matches(&candidate).unwrap();
    assert_eq!(
        matches.len(),
        1,
        "candidate reaches p through two levels but must be reported once: {matches:?}"
    );
    assert_eq!(matches[0].part_id, p);
    assert_eq!(matches[0].verdict_kind, "exact_sku");
    assert_eq!(matches[0].rank, 1);
}

#[test]
fn candidate_exact_mpn_reaches_rank_2() {
    let (_g, mut db) = open();
    let p = resistor(&mut db, "10k 0603", "10k", "1%", "1/4 W", "0603");
    variant(&mut db, &p, "Yageo", "RC0603FR-0710KL");

    // Candidate carries the MPN in a different case (proving the lookup is
    // case-insensitive) and the matching manufacturer, but no supplier_sku
    // and no category/attributes, so only the MPN level can fire.
    let candidate = MatchCandidate {
        manufacturer: Some("Yageo".into()),
        mpn: Some("rc0603fr-0710kl".into()),
        ..Default::default()
    };

    let matches = db.find_matches(&candidate).unwrap();
    assert_eq!(
        matches.len(),
        1,
        "only the MPN level should fire: {matches:?}"
    );
    assert_eq!(matches[0].part_id, p);
    assert_eq!(matches[0].verdict_kind, "exact_mpn");
    assert_eq!(matches[0].rank, 2);
}

#[test]
fn known_alias_hit_in_find_matches() {
    let (_g, mut db) = open();
    let p = make_part(&mut db, "Some part", "Resistor");
    db.add_alias("supplier_sku", "OLD-SKU-1", &p, "legacy import")
        .unwrap();

    let candidate = MatchCandidate {
        supplier: Some("DigiKey".into()),
        supplier_sku: Some("old-sku-1".into()),
        ..Default::default()
    };

    let matches = db.find_matches(&candidate).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == p)
        .expect("known alias should surface p");
    assert_eq!(hit.verdict_kind, "known_alias");
    assert_eq!(hit.rank, 3);
}

#[test]
fn mpn_alias_matches() {
    let (_g, mut db) = open();
    let p = make_part(&mut db, "Some part", "Resistor");
    db.add_alias("mpn", "OLD-MPN-1", &p, "legacy import")
        .unwrap();

    // No variant anywhere carries this MPN, so the only level that can fire
    // is the mpn alias itself.
    let candidate = MatchCandidate {
        mpn: Some("old-mpn-1".into()),
        ..Default::default()
    };

    let matches = db.find_matches(&candidate).unwrap();
    let hit = matches
        .iter()
        .find(|m| m.part_id == p)
        .expect("mpn alias should surface p");
    assert_eq!(hit.verdict_kind, "known_alias");
    assert_eq!(hit.rank, 3);
}

#[test]
fn duplicate_alias_returns_alias_taken() {
    let (_g, mut db) = open();
    let a = make_part(&mut db, "Part A", "Resistor");
    let b = make_part(&mut db, "Part B", "Resistor");
    db.add_alias("supplier_sku", "SHARED-SKU", &a, "import")
        .unwrap();
    let err = db
        .add_alias("supplier_sku", "SHARED-SKU", &b, "import")
        .unwrap_err();
    assert!(matches!(err, DbError::AliasTaken));
}
