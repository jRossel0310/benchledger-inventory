use inventory_core::ids::PartId;
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::parts::PartDraft;
use inventory_db::{Database, DbError, MISC_CATEGORY_ID};

pub fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

pub fn make_part(db: &mut Database, name: &str) -> PartId {
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

pub fn q(n: i64) -> Quantity {
    Quantity::from_whole(n).unwrap()
}

pub fn receive(db: &mut Database, part: &PartId, n: i64) {
    db.apply(&LedgerOp::Receive {
        part_id: part.clone(),
        quantity: q(n),
        note: String::new(),
    })
    .unwrap();
}

#[test]
fn receive_then_receive_accumulates() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "10k 0603");
    receive(&mut db, &part, 30);
    receive(&mut db, &part, 10);
    let stock = db.get_stock(&part).unwrap();
    assert_eq!(stock.available, q(40));
    assert_eq!(stock.lifetime_received, q(40));
    assert_eq!(db.list_transactions(&part).unwrap().len(), 2);
}

#[test]
fn consume_available_reduces_stock_and_bumps_lifetime() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "consume me");
    receive(&mut db, &part, 40);
    db.apply(&LedgerOp::ConsumeAvailable {
        part_id: part.clone(),
        quantity: q(5),
        project_id: None,
        note: "LED driver".into(),
    })
    .unwrap();
    let stock = db.get_stock(&part).unwrap();
    assert_eq!(stock.available, q(35));
    assert_eq!(stock.lifetime_consumed, q(5));
    assert_eq!(stock.lifetime_received, q(40));
}

#[test]
fn negative_stock_is_impossible() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "scarce");
    receive(&mut db, &part, 3);
    let err = db
        .apply(&LedgerOp::ConsumeAvailable {
            part_id: part.clone(),
            quantity: q(5),
            project_id: None,
            note: String::new(),
        })
        .unwrap_err();
    assert!(matches!(err, DbError::InsufficientStock(_)), "got {err:?}");
    // and the failed attempt left no ledger row and no stock change
    assert_eq!(db.get_stock(&part).unwrap().available, q(3));
    assert_eq!(db.list_transactions(&part).unwrap().len(), 1);
}

#[test]
fn adjustments_change_available_only_with_note() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "recounted");
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::AdjustDown {
        part_id: part.clone(),
        quantity: q(2),
        note: "recount: 2 bent".into(),
    })
    .unwrap();
    let stock = db.get_stock(&part).unwrap();
    assert_eq!(stock.available, q(8));
    assert_eq!(stock.lifetime_received, q(10));
    assert_eq!(stock.lifetime_consumed, Quantity::ZERO);

    let err = db
        .apply(&LedgerOp::AdjustUp {
            part_id: part.clone(),
            quantity: q(1),
            note: "".into(),
        })
        .unwrap_err();
    assert!(matches!(err, DbError::Ledger(_)));
}

#[test]
fn unknown_part_is_rejected() {
    let (_g, mut db) = open();
    let err = db
        .apply(&LedgerOp::Receive {
            part_id: PartId::new(),
            quantity: q(1),
            note: String::new(),
        })
        .unwrap_err();
    assert!(matches!(err, DbError::PartNotFound));
}

#[test]
fn ledger_rows_record_state_movement() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "movement");
    receive(&mut db, &part, 5);
    let txns = db.list_transactions(&part).unwrap();
    assert_eq!(txns[0].txn_type, "receive");
    assert_eq!(txns[0].from_state, None);
    assert_eq!(txns[0].to_state.as_deref(), Some("available"));
    assert_eq!(txns[0].quantity, q(5));
}

#[test]
fn list_projects_returns_every_project_alphabetically() {
    let (_g, mut db) = open();
    assert_eq!(db.list_projects().unwrap(), Vec::new());

    let blinky = db.create_project("Blinky Board").unwrap();
    let bench = db.create_project("Bench PSU Rebuild").unwrap();

    let projects = db.list_projects().unwrap();
    assert_eq!(projects.len(), 2);
    // "Bench PSU Rebuild" sorts before "Blinky Board".
    assert_eq!(projects[0].id, bench);
    assert_eq!(projects[0].name, "Bench PSU Rebuild");
    assert_eq!(projects[1].id, blinky);
    assert_eq!(projects[1].name, "Blinky Board");
}
