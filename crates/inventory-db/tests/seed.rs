use inventory_db::Database;

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

#[test]
fn builtin_categories_are_seeded_on_open() {
    let (_g, db) = open();
    let n: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM categories WHERE built_in = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        n >= 68,
        "expected at least 68 built-in categories (67 seeded + Miscellaneous), got {n}"
    );
    for name in [
        "Resistor",
        "Capacitor",
        "MOSFET",
        "Op amp",
        "Connector",
        "Development board",
        "Miscellaneous",
        "Crystal",
        "Wire",
    ] {
        let c: i64 = db
            .raw_conn()
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = ?1",
                [name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(c, 1, "category {name} missing or duplicated");
    }
}

#[test]
fn curated_categories_have_identity_attributes() {
    let (_g, db) = open();
    // Resistor: resistance, tolerance, power rating, package are identity-defining
    let identity_count: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM category_attributes ca
             JOIN categories c ON c.id = ca.category_id
             JOIN attribute_defs a ON a.id = ca.attribute_id
             WHERE c.name = 'Resistor' AND a.identity = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        identity_count >= 4,
        "resistor needs >=4 identity attributes, got {identity_count}"
    );
    // Capacitor has capacitance with unit_kind capacitance
    let kind: String = db
        .raw_conn()
        .query_row(
            "SELECT a.unit_kind FROM category_attributes ca
             JOIN categories c ON c.id = ca.category_id
             JOIN attribute_defs a ON a.id = ca.attribute_id
             WHERE c.name = 'Capacitor' AND a.key = 'capacitance'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kind, "capacitance");
}

#[test]
fn seeding_is_idempotent_and_insert_only() {
    let (_g, mut db) = open();
    // rename a built-in category (user customization)
    db.raw_conn()
        .execute(
            "UPDATE categories SET name = 'My Resistors' WHERE name = 'Resistor'",
            [],
        )
        .unwrap();
    let before: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
        .unwrap();
    let report = inventory_db::seed::ensure_builtins(db.conn_mut_for_tests()).unwrap();
    assert_eq!(report.categories_inserted, 0, "re-seed must insert nothing");
    let after: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    // the user rename survives (matched by deterministic id, not name)
    let renamed: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM categories WHERE name = 'My Resistors'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(renamed, 1);
}

#[test]
fn name_collision_with_user_category_is_tolerated() {
    let (_g, mut db) = open();
    // simulate a user/restored category occupying a built-in name under a different id
    db.raw_conn()
        .execute(
            "DELETE FROM category_attributes WHERE category_id = (SELECT id FROM categories WHERE name = 'Crystal')",
            [],
        )
        .unwrap();
    db.raw_conn()
        .execute("DELETE FROM categories WHERE name = 'Crystal'", [])
        .unwrap();
    db.raw_conn()
        .execute(
            "INSERT INTO categories (id, name, group_name, built_in) VALUES (?1, 'Crystal', 'Custom', 0)",
            [inventory_core::ids::CategoryId::new().as_str()],
        )
        .unwrap();
    // re-seed must not error and must not duplicate
    let report = inventory_db::seed::ensure_builtins(db.conn_mut_for_tests()).unwrap();
    assert_eq!(report.categories_inserted, 0);
    let n: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM categories WHERE name = 'Crystal'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn choice_attributes_have_choices() {
    let (_g, db) = open();
    let n: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM attribute_choices ac
             JOIN attribute_defs a ON a.id = ac.attribute_id
             WHERE a.key = 'mounting_style'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(n >= 2, "mounting_style needs choices (SMD/THT at least)");
}
