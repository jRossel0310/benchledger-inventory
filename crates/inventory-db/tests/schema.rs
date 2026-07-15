use inventory_db::{Database, MISC_CATEGORY_ID};

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

fn insert_part(db: &Database, id: &str) {
    db.raw_conn()
        .execute(
            "INSERT INTO parts (id, display_name, category_id) VALUES (?1, 'test part', ?2)",
            rusqlite::params![id, MISC_CATEGORY_ID],
        )
        .unwrap();
}

#[test]
fn part_stock_rejects_negative_values() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    let err = db.raw_conn().execute(
        "INSERT INTO part_stock (part_id, available_milli) VALUES ('00000000000000000000000001', -1)",
        [],
    );
    assert!(err.is_err(), "CHECK constraint must reject negative stock");
}

#[test]
fn transactions_reject_unknown_types_and_nonpositive_quantities() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    let bad_type = db.raw_conn().execute(
        "INSERT INTO transactions (id, part_id, txn_type, quantity_milli)
         VALUES ('00000000000000000000000002', '00000000000000000000000001', 'teleport', 1000)",
        [],
    );
    assert!(bad_type.is_err());
    let zero_qty = db.raw_conn().execute(
        "INSERT INTO transactions (id, part_id, txn_type, quantity_milli)
         VALUES ('00000000000000000000000003', '00000000000000000000000001', 'receive', 0)",
        [],
    );
    assert!(zero_qty.is_err());
}

#[test]
fn parts_require_existing_category() {
    let (_g, db) = open();
    let err = db.raw_conn().execute(
        "INSERT INTO parts (id, display_name, category_id)
         VALUES ('00000000000000000000000004', 'x', '11111111111111111111111111')",
        [],
    );
    assert!(err.is_err(), "FK to categories must be enforced");
}

#[test]
fn only_one_preferred_variant_per_part() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    let ins = |id: &str, pref: i64| {
        db.raw_conn().execute(
            "INSERT INTO manufacturer_variants (id, part_id, manufacturer, mpn, is_preferred)
             VALUES (?1, '00000000000000000000000001', 'M', ?1, ?2)",
            rusqlite::params![id, pref],
        )
    };
    ins("00000000000000000000000005", 1).unwrap();
    assert!(ins("00000000000000000000000006", 1).is_err(), "second preferred variant must be rejected");
    ins("00000000000000000000000007", 0).unwrap();
}

#[test]
fn a_transaction_can_only_be_reversed_once() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    let conn = db.raw_conn();
    conn.execute(
        "INSERT INTO transactions (id, part_id, txn_type, quantity_milli)
         VALUES ('0000000000000000000000000A', '00000000000000000000000001', 'receive', 1000)",
        [],
    )
    .unwrap();
    let rev = |id: &str| {
        conn.execute(
            "INSERT INTO transactions (id, part_id, txn_type, quantity_milli, reversed_txn_id)
             VALUES (?1, '00000000000000000000000001', 'reverse', 1000, '0000000000000000000000000A')",
            rusqlite::params![id],
        )
    };
    rev("0000000000000000000000000B").unwrap();
    assert!(rev("0000000000000000000000000C").is_err(), "double reversal must violate unique index");
}
