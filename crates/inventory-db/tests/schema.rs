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

fn insert_project(db: &Database, id: &str, name: &str) {
    db.raw_conn()
        .execute(
            "INSERT INTO projects (id, name) VALUES (?1, ?2)",
            rusqlite::params![id, name],
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
    assert!(
        ins("00000000000000000000000006", 1).is_err(),
        "second preferred variant must be rejected"
    );
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
    assert!(
        rev("0000000000000000000000000C").is_err(),
        "double reversal must violate unique index"
    );
}

#[test]
fn attribute_defs_reject_unknown_data_types() {
    let (_g, db) = open();
    let err = db.raw_conn().execute(
        "INSERT INTO attribute_defs (id, key, label, data_type) VALUES ('0000000000000000000000000D', 'x', 'X', 'blob')",
        [],
    );
    assert!(err.is_err());
}

#[test]
fn part_attribute_values_are_unique_per_part_and_attribute() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    // Task 4 seeds a built-in attribute with key 'resistance' via
    // `ensure_builtins` inside `open_and_migrate`, and `attribute_defs.key`
    // is UNIQUE, so this test-local fixture uses a non-colliding key; the
    // test only exercises the (part_id, attribute_id) uniqueness on
    // part_attribute_values, not the specific key name.
    db.raw_conn()
        .execute(
            "INSERT INTO attribute_defs (id, key, label, data_type) VALUES ('0000000000000000000000000E', 'test_resistance', 'Resistance', 'number_unit')",
            [],
        )
        .unwrap();
    let ins = || {
        db.raw_conn().execute(
            "INSERT INTO part_attribute_values (part_id, attribute_id, original_text, value_num)
             VALUES ('00000000000000000000000001', '0000000000000000000000000E', '10k', 10000.0)",
            [],
        )
    };
    ins().unwrap();
    assert!(
        ins().is_err(),
        "duplicate (part, attribute) must be rejected"
    );
}

#[test]
fn alias_values_are_unique_per_kind() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    let ins = |id: &str, kind: &str| {
        db.raw_conn().execute(
            "INSERT INTO part_aliases (id, alias_kind, alias_value, part_id)
             VALUES (?1, ?2, 'ABC-123', '00000000000000000000000001')",
            rusqlite::params![id, kind],
        )
    };
    ins("0000000000000000000000000H", "supplier_sku").unwrap();
    assert!(
        ins("0000000000000000000000000J", "supplier_sku").is_err(),
        "duplicate (kind, value) must be rejected"
    );
    ins("0000000000000000000000000K", "mpn")
        .expect("same value under a different kind must be accepted");
    let bad_kind = db.raw_conn().execute(
        "INSERT INTO part_aliases (id, alias_kind, alias_value, part_id)
         VALUES ('0000000000000000000000000L', 'nickname', 'ABC-124', '00000000000000000000000001')",
        [],
    );
    assert!(bad_kind.is_err(), "unknown alias_kind must be rejected");
}

#[test]
fn equivalence_pairs_are_canonical_and_unique() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    insert_part(&db, "00000000000000000000000002");
    let ins = |id: &str, a: &str, b: &str| {
        db.raw_conn().execute(
            "INSERT INTO equivalence_decisions (id, part_a, part_b, decision)
             VALUES (?1, ?2, ?3, 'approved')",
            rusqlite::params![id, a, b],
        )
    };
    let inverted = ins(
        "0000000000000000000000000M",
        "00000000000000000000000002",
        "00000000000000000000000001",
    );
    assert!(
        inverted.is_err(),
        "CHECK (part_a < part_b) must reject an inverted pair"
    );
    ins(
        "0000000000000000000000000N",
        "00000000000000000000000001",
        "00000000000000000000000002",
    )
    .unwrap();
    let dup = ins(
        "0000000000000000000000000P",
        "00000000000000000000000001",
        "00000000000000000000000002",
    );
    assert!(dup.is_err(), "duplicate canonical pair must be rejected");
}

#[test]
fn fts_stays_in_sync_with_search_text() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    let matches = |term: &str| -> Vec<String> {
        let conn = db.raw_conn();
        let mut stmt = conn
            .prepare("SELECT part_id FROM parts_fts WHERE parts_fts MATCH ?1")
            .unwrap();
        stmt.query_map([term], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    // INSERT propagates through the AFTER INSERT trigger.
    db.raw_conn()
        .execute(
            "INSERT INTO search_text (part_id, body)
             VALUES ('00000000000000000000000001', 'resistor 10k 0603 yageo')",
            [],
        )
        .unwrap();
    assert_eq!(matches("resistor"), vec!["00000000000000000000000001"]);
    // UPDATE propagates: the old body stops matching, the new one starts.
    db.raw_conn()
        .execute(
            "UPDATE search_text SET body = 'capacitor 100nF X7R'
             WHERE part_id = '00000000000000000000000001'",
            [],
        )
        .unwrap();
    assert!(
        matches("resistor").is_empty(),
        "old body must no longer match after UPDATE"
    );
    assert_eq!(matches("capacitor"), vec!["00000000000000000000000001"]);
    // DELETE propagates: nothing matches once the row is gone.
    db.raw_conn()
        .execute(
            "DELETE FROM search_text WHERE part_id = '00000000000000000000000001'",
            [],
        )
        .unwrap();
    assert!(
        matches("capacitor").is_empty(),
        "deleted row must no longer match"
    );
}

#[test]
fn attachments_dedupe_by_content_hash_and_are_strict() {
    let (_g, db) = open();
    let conn = db.raw_conn();
    // First insert of a hash succeeds.
    conn.execute(
        "INSERT INTO attachments (content_hash, ext, size_bytes, kind)
         VALUES ('deadbeef', 'png', 1234, 'photo')",
        [],
    )
    .unwrap();
    // A second row for the SAME hash violates the content_hash PRIMARY KEY:
    // one blob, one metadata row.
    let dup = conn.execute(
        "INSERT INTO attachments (content_hash, ext, size_bytes, kind)
         VALUES ('deadbeef', 'jpg', 5678, 'datasheet')",
        [],
    );
    assert!(dup.is_err(), "duplicate content_hash must be rejected");
    // STRICT: size_bytes is INTEGER — a non-numeric text value must be rejected
    // rather than silently coerced.
    let bad_type = conn.execute(
        "INSERT INTO attachments (content_hash, size_bytes, kind)
         VALUES ('cafef00d', 'not-a-number', 'photo')",
        [],
    );
    assert!(
        bad_type.is_err(),
        "STRICT must reject a non-integer size_bytes"
    );
    // The kind CHECK list rejects an unknown kind.
    let bad_kind = conn.execute(
        "INSERT INTO attachments (content_hash, size_bytes, kind)
         VALUES ('0badf00d', 10, 'hologram')",
        [],
    );
    assert!(
        bad_kind.is_err(),
        "unknown attachment kind must be rejected"
    );
}

#[test]
fn part_attachments_cascade_on_part_delete_and_dedupe_by_pk() {
    let (_g, db) = open();
    let conn = db.raw_conn();
    insert_part(&db, "00000000000000000000000001");
    conn.execute(
        "INSERT INTO attachments (content_hash, size_bytes, kind)
         VALUES ('abc123', 42, 'photo')",
        [],
    )
    .unwrap();
    let link = || {
        conn.execute(
            "INSERT INTO part_attachments (part_id, content_hash)
             VALUES ('00000000000000000000000001', 'abc123')",
            [],
        )
    };
    link().unwrap();
    assert!(
        link().is_err(),
        "duplicate (part_id, content_hash) link must be rejected by the PK"
    );
    // A link to a hash with no attachments row violates the FK.
    let dangling = conn.execute(
        "INSERT INTO part_attachments (part_id, content_hash)
         VALUES ('00000000000000000000000001', 'no-such-hash')",
        [],
    );
    assert!(
        dangling.is_err(),
        "FK to attachments(content_hash) must be enforced"
    );
    // Deleting the part cascades the link away; the shared blob row survives.
    conn.execute(
        "DELETE FROM parts WHERE id = '00000000000000000000000001'",
        [],
    )
    .unwrap();
    let links: i64 = conn
        .query_row("SELECT COUNT(*) FROM part_attachments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(links, 0, "part delete must cascade its attachment links");
    let blobs: i64 = conn
        .query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        blobs, 1,
        "the shared blob row must survive an unlink cascade"
    );
}

#[test]
fn project_status_and_build_quantity_checks_are_enforced() {
    let (_g, db) = open();
    let bad_status = db.raw_conn().execute(
        "INSERT INTO projects (id, name, status) VALUES ('00000000000000000000000010', 'P', 'lost')",
        [],
    );
    assert!(
        bad_status.is_err(),
        "unknown project status must be rejected"
    );
    let bad_qty = db.raw_conn().execute(
        "INSERT INTO projects (id, name, build_quantity) VALUES ('00000000000000000000000011', 'P', 0)",
        [],
    );
    assert!(bad_qty.is_err(), "build_quantity below 1 must be rejected");
    // A minimal insert (mirrors the Phase 2a stub `create_project`, which only
    // ever supplies id + name) must still succeed and pick up the new
    // defaults added by migration 0006.
    insert_project(&db, "00000000000000000000000012", "Default Project");
    let (status, build_quantity, description, notes): (String, i64, String, String) = db
        .raw_conn()
        .query_row(
            "SELECT status, build_quantity, description, notes FROM projects
             WHERE id = '00000000000000000000000012'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(status, "planned");
    assert_eq!(build_quantity, 1);
    assert_eq!(description, "");
    assert_eq!(notes, "");
}

#[test]
fn bom_items_reject_duplicate_project_part_bad_quantity_and_are_strict() {
    let (_g, db) = open();
    insert_project(&db, "00000000000000000000000010", "Blinky");
    insert_part(&db, "00000000000000000000000001");
    insert_part(&db, "00000000000000000000000002");

    let ins = |id: &str, part_id: &str, qty: i64| {
        db.raw_conn().execute(
            "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli)
             VALUES (?1, '00000000000000000000000010', ?2, ?3)",
            rusqlite::params![id, part_id, qty],
        )
    };
    ins(
        "00000000000000000000000020",
        "00000000000000000000000001",
        3000,
    )
    .unwrap();
    assert!(
        ins(
            "00000000000000000000000021",
            "00000000000000000000000001",
            1000
        )
        .is_err(),
        "duplicate (project_id, part_id) must be rejected"
    );
    assert!(
        ins(
            "00000000000000000000000022",
            "00000000000000000000000002",
            0
        )
        .is_err(),
        "quantity_per_build_milli must be > 0"
    );
    // STRICT: quantity_per_build_milli is INTEGER — a non-numeric text value
    // must be rejected rather than silently coerced.
    let bad_type = db.raw_conn().execute(
        "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli)
         VALUES ('00000000000000000000000023', '00000000000000000000000010', '00000000000000000000000002', 'lots')",
        [],
    );
    assert!(
        bad_type.is_err(),
        "STRICT must reject a non-integer quantity_per_build_milli"
    );
}

#[test]
fn bom_items_cascade_on_project_delete() {
    let (_g, db) = open();
    insert_project(&db, "00000000000000000000000010", "Blinky");
    insert_part(&db, "00000000000000000000000001");
    db.raw_conn()
        .execute(
            "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli)
             VALUES ('00000000000000000000000020', '00000000000000000000000010', '00000000000000000000000001', 3000)",
            [],
        )
        .unwrap();
    db.raw_conn()
        .execute(
            "DELETE FROM projects WHERE id = '00000000000000000000000010'",
            [],
        )
        .unwrap();
    let n: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM bom_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "deleting a project must cascade its BOM items");
}

#[test]
fn bom_substitutes_cascade_on_bom_item_delete_and_are_unique_by_pk() {
    let (_g, db) = open();
    insert_project(&db, "00000000000000000000000010", "Blinky");
    insert_part(&db, "00000000000000000000000001");
    insert_part(&db, "00000000000000000000000002");
    db.raw_conn()
        .execute(
            "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli)
             VALUES ('00000000000000000000000020', '00000000000000000000000010', '00000000000000000000000001', 3000)",
            [],
        )
        .unwrap();
    let link = || {
        db.raw_conn().execute(
            "INSERT INTO bom_substitutes (bom_item_id, part_id)
             VALUES ('00000000000000000000000020', '00000000000000000000000002')",
            [],
        )
    };
    link().unwrap();
    assert!(
        link().is_err(),
        "duplicate (bom_item_id, part_id) must be rejected by the PK"
    );
    db.raw_conn()
        .execute(
            "DELETE FROM bom_items WHERE id = '00000000000000000000000020'",
            [],
        )
        .unwrap();
    let n: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM bom_substitutes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "deleting a bom_item must cascade its substitutes");
}

#[test]
fn dimensions_reject_unknown_source_and_group() {
    let (_g, db) = open();
    insert_part(&db, "00000000000000000000000001");
    let bad_source = db.raw_conn().execute(
        "INSERT INTO dimensions (id, part_id, dim_group, name, value_num, display_unit, normalized_value, source)
         VALUES ('0000000000000000000000000F', '00000000000000000000000001', 'overall', 'Length', 5.0, 'mm', 5.0, 'guessed')",
        [],
    );
    assert!(bad_source.is_err());
    let bad_group = db.raw_conn().execute(
        "INSERT INTO dimensions (id, part_id, dim_group, name, value_num, display_unit, normalized_value, source)
         VALUES ('0000000000000000000000000G', '00000000000000000000000001', 'sideways', 'Length', 5.0, 'mm', 5.0, 'measured')",
        [],
    );
    assert!(bad_group.is_err());
}
