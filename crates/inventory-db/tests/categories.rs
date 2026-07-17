use inventory_core::ids::CategoryId;
use inventory_db::{Database, DbError};

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

fn resistor_id(db: &Database) -> CategoryId {
    let raw: String = db
        .raw_conn()
        .query_row(
            "SELECT id FROM categories WHERE name = 'Resistor'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    CategoryId::from_string(raw).unwrap()
}

#[test]
fn create_and_list_custom_categories() {
    let (_g, mut db) = open();
    let created = db
        .create_category("Vacuum tube", "Mechanical and miscellaneous")
        .unwrap();
    assert!(!created.built_in);
    let all = db.list_categories().unwrap();
    assert!(all.iter().any(|c| c.name == "Vacuum tube" && !c.built_in));
    assert!(matches!(
        db.create_category("Vacuum tube", "Mechanical and miscellaneous")
            .unwrap_err(),
        DbError::CategoryNameTaken
    ));
}

#[test]
fn duplicate_category_copies_attribute_links() {
    let (_g, mut db) = open();
    let resistor = resistor_id(&db);
    let copy = db
        .duplicate_category(&resistor, "Precision resistor")
        .unwrap();
    let src = db.category_attributes(&resistor).unwrap();
    let dst = db.category_attributes(&copy.id).unwrap();
    assert_eq!(src.len(), dst.len());
    assert!(!copy.built_in);
}

#[test]
fn custom_attributes_attach_reorder_and_hide() {
    let (_g, mut db) = open();
    let resistor = resistor_id(&db);
    db.create_custom_attribute(
        "pulse_rating",
        "Pulse rating",
        "number_unit",
        Some("power"),
        false,
    )
    .unwrap();
    assert!(matches!(
        db.create_custom_attribute("pulse_rating", "Again", "text", None, false)
            .unwrap_err(),
        DbError::AttributeKeyTaken
    ));
    assert!(matches!(
        db.create_custom_attribute("bad_type", "Bad", "blob", None, false)
            .unwrap_err(),
        DbError::InvalidAttributeValue { .. }
    ));
    db.attach_attribute(&resistor, "pulse_rating", 99).unwrap();
    let attrs = db.category_attributes(&resistor).unwrap();
    let last = attrs.last().unwrap();
    assert_eq!(last.0, "pulse_rating");
    db.reorder_attribute(&resistor, "pulse_rating", 0).unwrap();
    let attrs = db.category_attributes(&resistor).unwrap();
    assert_eq!(attrs.first().unwrap().0, "pulse_rating");
    db.set_attribute_hidden(&resistor, "temp_coefficient", true)
        .unwrap();
    let hidden = db
        .category_attributes(&resistor)
        .unwrap()
        .into_iter()
        .find(|(k, _, _, _)| k == "temp_coefficient")
        .unwrap();
    assert!(hidden.3, "temp_coefficient should be hidden");
}

#[test]
fn unit_kind_combinations_are_validated() {
    let (_g, mut db) = open();
    assert!(matches!(
        db.create_custom_attribute("nu_missing", "NU", "number_unit", None, false)
            .unwrap_err(),
        DbError::InvalidAttributeValue { .. }
    ));
    assert!(matches!(
        db.create_custom_attribute("txt_with_unit", "T", "text", Some("power"), false)
            .unwrap_err(),
        DbError::InvalidAttributeValue { .. }
    ));
    db.create_custom_attribute("nu_ok", "NU", "number_unit", Some("power"), false)
        .unwrap();
}

#[test]
fn category_attribute_defs_carries_data_type_unit_kind_identity_and_choices() {
    let (_g, db) = open();
    let resistor = resistor_id(&db);
    let defs = db.category_attribute_defs(&resistor).unwrap();

    // Same set/order as the thin `category_attributes` tuple.
    let thin = db.category_attributes(&resistor).unwrap();
    assert_eq!(defs.len(), thin.len());
    assert_eq!(
        defs.iter().map(|d| d.key.clone()).collect::<Vec<_>>(),
        thin.iter().map(|(k, ..)| k.clone()).collect::<Vec<_>>()
    );

    let resistance = defs.iter().find(|d| d.key == "resistance").unwrap();
    assert_eq!(resistance.data_type, "number_unit");
    assert_eq!(resistance.unit_kind.as_deref(), Some("resistance"));
    assert!(resistance.identity);
    assert!(resistance.choices.is_empty());

    let mounting = defs.iter().find(|d| d.key == "mounting_style").unwrap();
    assert_eq!(mounting.data_type, "choice");
    assert_eq!(mounting.unit_kind, None);
    assert!(!mounting.identity);
    assert_eq!(
        mounting.choices,
        vec!["SMD", "THT", "Panel mount", "Free hanging", "Chassis"]
    );
}

#[test]
fn unknown_category_and_attribute_are_typed_errors() {
    let (_g, mut db) = open();
    assert!(matches!(
        db.duplicate_category(&CategoryId::new(), "X").unwrap_err(),
        DbError::CategoryNotFound
    ));
    let resistor = resistor_id(&db);
    assert!(matches!(
        db.attach_attribute(&resistor, "no_such_attr", 1)
            .unwrap_err(),
        DbError::AttributeNotFound(_)
    ));
}
