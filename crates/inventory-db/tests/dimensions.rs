use inventory_core::quantity::QuantityUnit;
use inventory_db::dimensions::{DimensionDraft, DimensionGroup, DimensionSource};
use inventory_db::parts::PartDraft;
use inventory_db::{Database, DbError, MISC_CATEGORY_ID};

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

fn part(db: &mut Database) -> inventory_core::ids::PartId {
    let draft = PartDraft {
        display_name: "measured thing".into(),
        category_id: inventory_core::ids::CategoryId::from_string(MISC_CATEGORY_ID.into()).unwrap(),
        description: String::new(),
        bin_label: None,
        usage_behavior: "ask".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    db.create_part(&draft).unwrap().id
}

#[test]
fn millimeter_and_inch_dimensions_normalize_to_mm() {
    let (_g, mut db) = open();
    let p = part(&mut db);
    let a = db
        .add_dimension(
            &p,
            &DimensionDraft {
                group: DimensionGroup::Overall,
                name: "Length".into(),
                raw_value: "5 mm".into(),
                source: DimensionSource::Datasheet,
                notes: String::new(),
                measured_date: None,
            },
        )
        .unwrap();
    assert!((a.normalized_value - 5.0).abs() < 1e-9);
    assert_eq!(a.display_unit, "mm");
    let b = db
        .add_dimension(
            &p,
            &DimensionDraft {
                group: DimensionGroup::Mounting,
                name: "Pin pitch".into(),
                raw_value: "0.1 in".into(),
                source: DimensionSource::Manufacturer,
                notes: String::new(),
                measured_date: None,
            },
        )
        .unwrap();
    assert!((b.normalized_value - 2.54).abs() < 1e-9);
    assert_eq!(b.display_unit, "in");
}

#[test]
fn mass_normalizes_to_grams_and_custom_dimensions_work() {
    let (_g, mut db) = open();
    let p = part(&mut db);
    let w = db
        .add_dimension(
            &p,
            &DimensionDraft {
                group: DimensionGroup::Overall,
                name: "Weight".into(),
                raw_value: "1.5 g".into(),
                source: DimensionSource::Measured,
                notes: "digital scale".into(),
                measured_date: Some("2026-07-14".into()),
            },
        )
        .unwrap();
    assert!((w.normalized_value - 1.5).abs() < 1e-9);
    let c = db
        .add_dimension(
            &p,
            &DimensionDraft {
                group: DimensionGroup::Custom,
                name: "Knob outer diameter".into(),
                raw_value: "16 mm".into(),
                source: DimensionSource::Measured,
                notes: "calipers, excludes shaft".into(),
                measured_date: None,
            },
        )
        .unwrap();
    assert_eq!(c.name, "Knob outer diameter");
    assert_eq!(db.list_dimensions(&p).unwrap().len(), 2);
}

#[test]
fn unparseable_and_wrong_kind_values_are_rejected() {
    let (_g, mut db) = open();
    let p = part(&mut db);
    for bad in ["banana", "10 V", ""] {
        let err = db
            .add_dimension(
                &p,
                &DimensionDraft {
                    group: DimensionGroup::Overall,
                    name: "Length".into(),
                    raw_value: bad.into(),
                    source: DimensionSource::Estimated,
                    notes: String::new(),
                    measured_date: None,
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, DbError::InvalidDimension(_)),
            "'{bad}' should fail, got {err:?}"
        );
    }
}

#[test]
fn remove_dimension_deletes_the_row() {
    let (_g, mut db) = open();
    let p = part(&mut db);
    let d = db
        .add_dimension(
            &p,
            &DimensionDraft {
                group: DimensionGroup::Body,
                name: "Body height".into(),
                raw_value: "3.2 mm".into(),
                source: DimensionSource::Datasheet,
                notes: String::new(),
                measured_date: None,
            },
        )
        .unwrap();
    db.remove_dimension(&d.id).unwrap();
    assert!(db.list_dimensions(&p).unwrap().is_empty());
    assert!(matches!(
        db.remove_dimension(&d.id).unwrap_err(),
        DbError::DimensionNotFound
    ));
}
