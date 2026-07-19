use inventory_core::ids::CategoryId;
use inventory_core::quantity::QuantityUnit;
use inventory_db::parts::PartDraft;
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

fn resistor(db: &mut Database) -> inventory_core::ids::PartId {
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

#[test]
fn number_unit_attributes_normalize_and_preserve_original() {
    let (_g, mut db) = open();
    let part = resistor(&mut db);
    db.set_attribute(&part, "resistance", "10k").unwrap();
    let attrs = db.get_attributes(&part).unwrap();
    let (_, original, num) = attrs
        .iter()
        .find(|(k, _, _)| k == "resistance")
        .map(|(k, o, n)| (k.clone(), o.clone(), *n))
        .unwrap();
    assert_eq!(original, "10k");
    assert!((num.unwrap() - 10_000.0).abs() < 1e-9);
}

#[test]
fn equivalent_notations_store_equal_normalized_values() {
    let (_g, mut db) = open();
    let a = resistor(&mut db);
    let b = resistor(&mut db);
    db.set_attribute(&a, "resistance", "10k").unwrap();
    db.set_attribute(&b, "resistance", "10000 ohm").unwrap();
    let va = db.get_attributes(&a).unwrap()[0].2.unwrap();
    let vb = db.get_attributes(&b).unwrap()[0].2.unwrap();
    assert_eq!(va, vb);
}

#[test]
fn preview_unit_value_formats_without_touching_a_part() {
    assert_eq!(
        inventory_db::attributes::preview_unit_value("resistance", "10k").unwrap(),
        "10 kΩ"
    );
    assert_eq!(
        inventory_db::attributes::preview_unit_value("resistance", "10000 ohm").unwrap(),
        "10 kΩ"
    );
    assert_eq!(
        inventory_db::attributes::preview_unit_value("capacitance", "100nF").unwrap(),
        "100 nF"
    );
}

#[test]
fn preview_unit_value_rejects_unknown_kind_and_unparsable_value() {
    assert!(matches!(
        inventory_db::attributes::preview_unit_value("not_a_kind", "10k").unwrap_err(),
        DbError::InvalidAttributeValue { .. }
    ));
    assert!(matches!(
        inventory_db::attributes::preview_unit_value("resistance", "10 V").unwrap_err(),
        DbError::InvalidAttributeValue { .. }
    ));
    assert!(matches!(
        inventory_db::attributes::preview_unit_value("resistance", "  ").unwrap_err(),
        DbError::InvalidAttributeValue { .. }
    ));
}

#[test]
fn wrong_unit_and_unknown_choice_are_typed_errors() {
    let (_g, mut db) = open();
    let part = resistor(&mut db);
    let err = db.set_attribute(&part, "resistance", "10 V").unwrap_err();
    assert!(
        matches!(err, DbError::InvalidAttributeValue { .. }),
        "got {err:?}"
    );
    let err = db
        .set_attribute(&part, "mounting_style", "Orbital")
        .unwrap_err();
    assert!(matches!(err, DbError::InvalidAttributeValue { .. }));
    db.set_attribute(&part, "mounting_style", "SMD").unwrap();
    let err = db
        .set_attribute(&part, "nonexistent_attr", "x")
        .unwrap_err();
    assert!(matches!(err, DbError::AttributeNotFound(_)));
}

#[test]
fn identity_attributes_are_exposed_for_matching() {
    let (_g, mut db) = open();
    let part = resistor(&mut db);
    db.set_attribute(&part, "resistance", "4k7").unwrap();
    db.set_attribute(&part, "tolerance", "1%").unwrap();
    db.set_attribute(&part, "package", "0603").unwrap();
    let ids = db.identity_attributes(&part).unwrap();
    let keys: Vec<&str> = ids.iter().map(|(k, _, _)| k.as_str()).collect();
    assert!(keys.contains(&"resistance"));
    assert!(keys.contains(&"tolerance"));
    assert!(keys.contains(&"package"));
    let resistance = ids.iter().find(|(k, _, _)| k == "resistance").unwrap();
    assert!((resistance.1.unwrap() - 4700.0).abs() < 1e-9);
}

#[test]
fn set_attribute_overwrites_previous_value() {
    let (_g, mut db) = open();
    let part = resistor(&mut db);
    db.set_attribute(&part, "resistance", "10k").unwrap();
    db.set_attribute(&part, "resistance", "4k7").unwrap();
    let attrs = db.get_attributes(&part).unwrap();
    let (_, original, num) = attrs
        .iter()
        .find(|(k, _, _)| k == "resistance")
        .map(|(k, o, n)| (k.clone(), o.clone(), *n))
        .unwrap();
    assert_eq!(original, "4k7");
    assert!((num.unwrap() - 4700.0).abs() < 1e-9);
    assert_eq!(attrs.len(), 1, "upsert must not create a second row");
}

#[test]
fn range_and_multichoice_and_boolean_round_trip() {
    let (_g, mut db) = open();
    let draft = PartDraft {
        display_name: "IRLZ44N".into(),
        category_id: category_id(&db, "MOSFET"),
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    let part = db.create_part(&draft).unwrap().id;
    db.set_attribute(&part, "vgs_threshold", "1V..2V").unwrap();
    db.set_attribute(&part, "logic_level", "true").unwrap();
    db.set_attribute(&part, "channel_type", "N-channel")
        .unwrap();
    let attrs = db.get_attributes(&part).unwrap();
    assert_eq!(attrs.len(), 3);
    db.clear_attribute(&part, "logic_level").unwrap();
    assert_eq!(db.get_attributes(&part).unwrap().len(), 2);
}

#[test]
fn list_attribute_values_joins_label_and_canonical_unit() {
    let (_g, mut db) = open();
    let part = resistor(&mut db);
    db.set_attribute(&part, "resistance", "10k").unwrap();
    db.set_attribute(&part, "tolerance", "1%").unwrap();
    let rows = db.list_attribute_values(&part).unwrap();
    assert_eq!(rows.len(), 2);
    // Ordered by key, same as get_attributes.
    assert_eq!(rows[0].key, "resistance");
    let resistance = &rows[0];
    assert_eq!(resistance.label, "Resistance");
    assert_eq!(resistance.original_text, "10k");
    assert!((resistance.normalized_value.unwrap() - 10_000.0).abs() < 1e-9);
    assert_eq!(resistance.canonical_unit.as_deref(), Some("Ω"));
}

#[test]
fn list_attribute_values_is_empty_for_unset_part() {
    let (_g, mut db) = open();
    let part = resistor(&mut db);
    assert!(db.list_attribute_values(&part).unwrap().is_empty());
}
