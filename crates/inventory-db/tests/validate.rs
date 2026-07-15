use inventory_core::ids::PartId;
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::parts::PartDraft;
use inventory_db::{Database, MISC_CATEGORY_ID};

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

fn make_part(db: &mut Database, name: &str) -> PartId {
    let draft = PartDraft {
        display_name: name.to_string(),
        category_id: inventory_core::ids::CategoryId::from_string(MISC_CATEGORY_ID.into()).unwrap(),
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

fn q(n: i64) -> Quantity {
    Quantity::from_whole(n).unwrap()
}

fn receive(db: &mut Database, part: &PartId, n: i64) {
    db.apply(&LedgerOp::Receive { part_id: part.clone(), quantity: q(n), note: String::new() })
        .unwrap();
}

#[test]
fn clean_ledger_validates_clean() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "clean");
    receive(&mut db, &part, 30);
    let project = db.create_project("p").unwrap();
    db.apply(&LedgerOp::Reserve { part_id: part.clone(), quantity: q(5), project_id: project }).unwrap();
    db.apply(&LedgerOp::ConsumeReserved { part_id: part.clone(), quantity: q(2), project_id: None, note: String::new() }).unwrap();
    let consume = db
        .apply(&LedgerOp::ConsumeAvailable { part_id: part.clone(), quantity: q(1), project_id: None, note: String::new() })
        .unwrap();
    db.reverse_transaction(&consume.id, "oops").unwrap();

    let report = db.validate_invariants().unwrap();
    assert!(report.is_clean(), "{:?}", report.discrepancies);
    assert_eq!(report.parts_checked, 1);
}

#[test]
fn tampered_aggregates_are_detected() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "tampered");
    receive(&mut db, &part, 30);
    // simulate corruption: bypass the ledger and poke the aggregate directly
    db.raw_conn()
        .execute("UPDATE part_stock SET available_milli = 99000 WHERE part_id = ?1", [part.as_str()])
        .unwrap();
    let report = db.validate_invariants().unwrap();
    assert!(!report.is_clean());
    let d = &report.discrepancies[0];
    assert_eq!(d.field, "available_milli");
    assert_eq!(d.stored, 99_000);
    assert_eq!(d.recomputed, 30_000);
}

#[test]
fn empty_database_validates_clean() {
    let (_g, db) = open();
    let report = db.validate_invariants().unwrap();
    assert!(report.is_clean());
    assert_eq!(report.parts_checked, 0);
}
