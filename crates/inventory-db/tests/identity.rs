use inventory_core::ids::{CategoryId, PartId};
use inventory_core::quantity::QuantityUnit;
use inventory_db::identity::{identity_signature, signatures_equal};
use inventory_db::parts::PartDraft;
use inventory_db::{Database, MISC_CATEGORY_ID};

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

fn resistor(db: &mut Database) -> PartId {
    let draft = PartDraft {
        display_name: "10k 0603".into(),
        category_id: category_id(db, "Resistor"),
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    db.create_part(&draft).unwrap().id
}

fn mosfet(db: &mut Database) -> PartId {
    let draft = PartDraft {
        display_name: "IRLZ44N".into(),
        category_id: category_id(db, "MOSFET"),
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    db.create_part(&draft).unwrap().id
}

fn misc_part(db: &mut Database) -> PartId {
    let draft = PartDraft {
        display_name: "Assorted screws".into(),
        category_id: CategoryId::from_string(MISC_CATEGORY_ID.into()).unwrap(),
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    db.create_part(&draft).unwrap().id
}

/// THE ParsedValue-not-f64 test: equivalent notations across every identity
/// data shape a resistor exercises (number_unit resistance, number_unit
/// tolerance, number_unit power with a fraction vs decimal, and a
/// package imperial/metric pair) must produce equal signatures.
#[test]
fn equivalent_notations_have_equal_signatures() {
    let (_g, mut db) = open();
    let a = resistor(&mut db);
    let b = resistor(&mut db);
    db.set_attribute(&a, "resistance", "10k").unwrap();
    db.set_attribute(&a, "tolerance", "1%").unwrap();
    db.set_attribute(&a, "power_rating", "1/4 W").unwrap();
    db.set_attribute(&a, "package", "0603").unwrap();

    db.set_attribute(&b, "resistance", "10000 ohm").unwrap();
    db.set_attribute(&b, "tolerance", "1 %").unwrap();
    db.set_attribute(&b, "power_rating", "0.25W").unwrap();
    db.set_attribute(&b, "package", "1608 metric").unwrap();

    let sig_a = identity_signature(&db, &a).unwrap();
    let sig_b = identity_signature(&db, &b).unwrap();
    assert!(sig_a.is_some());
    assert!(sig_b.is_some());
    assert!(signatures_equal(&sig_a.unwrap(), &sig_b.unwrap()));
}

#[test]
fn different_values_differ() {
    let (_g, mut db) = open();
    let a = resistor(&mut db);
    let b = resistor(&mut db);
    db.set_attribute(&a, "resistance", "10k").unwrap();
    db.set_attribute(&a, "tolerance", "1%").unwrap();
    db.set_attribute(&a, "power_rating", "1/4 W").unwrap();
    db.set_attribute(&a, "package", "0603").unwrap();

    db.set_attribute(&b, "resistance", "4k7").unwrap();
    db.set_attribute(&b, "tolerance", "1%").unwrap();
    db.set_attribute(&b, "power_rating", "1/4 W").unwrap();
    db.set_attribute(&b, "package", "0603").unwrap();

    let sig_a = identity_signature(&db, &a).unwrap().unwrap();
    let sig_b = identity_signature(&db, &b).unwrap().unwrap();
    assert!(!signatures_equal(&sig_a, &sig_b));
}

#[test]
fn missing_identity_attribute_yields_none() {
    let (_g, mut db) = open();
    let part = resistor(&mut db);
    db.set_attribute(&part, "resistance", "10k").unwrap();
    // tolerance, power_rating, package are identity-flagged but unset.
    let sig = identity_signature(&db, &part).unwrap();
    assert!(sig.is_none());
}

#[test]
fn category_without_identity_attributes_yields_none() {
    let (_g, mut db) = open();
    let part = misc_part(&mut db);
    let sig = identity_signature(&db, &part).unwrap();
    assert!(sig.is_none());
}

/// The range-comparison test: a `range` identity attribute (MOSFET's
/// vgs_threshold) must re-parse both bounds under its unit kind and compare
/// via `ParsedValue` equality, not string equality, so "1V..2V" and
/// "1000mV to 2000mV" match. All other MOSFET identity attributes
/// (channel_type, vds_max, package) are set identically so they can't be the
/// source of any inequality.
#[test]
fn range_attributes_participate_exactly() {
    let (_g, mut db) = open();
    let a = mosfet(&mut db);
    let b = mosfet(&mut db);

    db.set_attribute(&a, "channel_type", "N-channel").unwrap();
    db.set_attribute(&a, "vds_max", "30V").unwrap();
    db.set_attribute(&a, "package", "TO-220").unwrap();
    db.set_attribute(&a, "vgs_threshold", "1V..2V").unwrap();

    db.set_attribute(&b, "channel_type", "N-channel").unwrap();
    db.set_attribute(&b, "vds_max", "30V").unwrap();
    db.set_attribute(&b, "package", "TO-220").unwrap();
    db.set_attribute(&b, "vgs_threshold", "1000mV to 2000mV")
        .unwrap();

    let sig_a = identity_signature(&db, &a).unwrap();
    let sig_b = identity_signature(&db, &b).unwrap();
    assert!(sig_a.is_some());
    assert!(sig_b.is_some());
    assert!(signatures_equal(&sig_a.unwrap(), &sig_b.unwrap()));
}
