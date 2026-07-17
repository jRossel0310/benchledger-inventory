//! Phase 3 Task 4: the tiny key-value settings store backing the Inventory
//! browser's saved-search views (`settings` table, migration 0001).

use inventory_db::Database;

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

#[test]
fn missing_key_returns_none() {
    let (_g, db) = open();
    assert_eq!(db.get_setting("nope").unwrap(), None);
}

#[test]
fn set_then_get_round_trips() {
    let (_g, mut db) = open();
    db.set_setting("saved_views", "[]").unwrap();
    assert_eq!(
        db.get_setting("saved_views").unwrap(),
        Some("[]".to_string())
    );
}

#[test]
fn set_overwrites_an_existing_value_rather_than_erroring() {
    let (_g, mut db) = open();
    db.set_setting("k", "one").unwrap();
    db.set_setting("k", "two").unwrap();
    assert_eq!(db.get_setting("k").unwrap(), Some("two".to_string()));
}

#[test]
fn different_keys_are_independent() {
    let (_g, mut db) = open();
    db.set_setting("a", "1").unwrap();
    db.set_setting("b", "2").unwrap();
    assert_eq!(db.get_setting("a").unwrap(), Some("1".to_string()));
    assert_eq!(db.get_setting("b").unwrap(), Some("2".to_string()));
}

#[test]
fn value_can_be_an_empty_string_distinct_from_unset() {
    let (_g, mut db) = open();
    db.set_setting("k", "").unwrap();
    // An explicitly-set empty string is `Some("")`, not `None` — distinct
    // from the key never having been written at all.
    assert_eq!(db.get_setting("k").unwrap(), Some(String::new()));
}
