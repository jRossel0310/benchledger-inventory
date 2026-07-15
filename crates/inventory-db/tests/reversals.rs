use inventory_core::ids::PartId;
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::parts::PartDraft;
use inventory_db::{Database, DbError, MISC_CATEGORY_ID};

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
    db.apply(&LedgerOp::Receive {
        part_id: part.clone(),
        quantity: q(n),
        note: String::new(),
    })
    .unwrap();
}

#[test]
fn reversing_a_consume_restores_stock_and_lifetime() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "restore me");
    receive(&mut db, &part, 40);
    let consume = db
        .apply(&LedgerOp::ConsumeAvailable {
            part_id: part.clone(),
            quantity: q(5),
            project_id: None,
            note: String::new(),
        })
        .unwrap();
    assert_eq!(db.get_stock(&part).unwrap().available, q(35));

    let reversal = db.reverse_transaction(&consume.id, "mis-click").unwrap();
    assert_eq!(reversal.txn_type, "reverse");
    assert_eq!(reversal.reversed_txn_id.as_ref().unwrap(), &consume.id);
    let s = db.get_stock(&part).unwrap();
    assert_eq!(s.available, q(40));
    assert_eq!(s.lifetime_consumed, q(0));
}

#[test]
fn reversing_a_receive_subtracts_lifetime_received() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "unreceive");
    receive(&mut db, &part, 10);
    let receive_txn = &db.list_transactions(&part).unwrap()[0];
    db.reverse_transaction(&receive_txn.id.clone(), "wrong part")
        .unwrap();
    let s = db.get_stock(&part).unwrap();
    assert_eq!(s.available, q(0));
    assert_eq!(s.lifetime_received, q(0));
}

#[test]
fn a_transaction_cannot_be_reversed_twice() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "once only");
    receive(&mut db, &part, 10);
    let consume = db
        .apply(&LedgerOp::ConsumeAvailable {
            part_id: part.clone(),
            quantity: q(1),
            project_id: None,
            note: String::new(),
        })
        .unwrap();
    db.reverse_transaction(&consume.id, "").unwrap();
    assert!(matches!(
        db.reverse_transaction(&consume.id, "").unwrap_err(),
        DbError::AlreadyReversed
    ));
}

#[test]
fn a_reversal_cannot_be_reversed() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "no meta-reversal");
    receive(&mut db, &part, 10);
    let consume = db
        .apply(&LedgerOp::ConsumeAvailable {
            part_id: part.clone(),
            quantity: q(1),
            project_id: None,
            note: String::new(),
        })
        .unwrap();
    let reversal = db.reverse_transaction(&consume.id, "").unwrap();
    assert!(matches!(
        db.reverse_transaction(&reversal.id, "").unwrap_err(),
        DbError::CannotReverseReversal
    ));
}

#[test]
fn reversal_fails_if_stock_since_moved_away() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "gone already");
    receive(&mut db, &part, 10);
    let receive_txn = db.list_transactions(&part).unwrap()[0].clone();
    db.apply(&LedgerOp::ConsumeAvailable {
        part_id: part.clone(),
        quantity: q(8),
        project_id: None,
        note: String::new(),
    })
    .unwrap();
    // reversing the receive would need available >= 10, but only 2 remain
    assert!(matches!(
        db.reverse_transaction(&receive_txn.id, "").unwrap_err(),
        DbError::InsufficientStock(_)
    ));
    // and the failed reversal left no trace
    assert_eq!(db.get_stock(&part).unwrap().available, q(2));
    assert_eq!(db.list_transactions(&part).unwrap().len(), 2);
}

#[test]
fn reverse_group_undoes_every_member_atomically() {
    let (_g, mut db) = open();
    let a = make_part(&mut db, "ga");
    let b = make_part(&mut db, "gb");
    receive(&mut db, &a, 10);
    receive(&mut db, &b, 10);
    let project = db.create_project("undo me").unwrap();
    let group = db
        .apply_group(
            "reserve_bom",
            "",
            &[
                LedgerOp::Reserve {
                    part_id: a.clone(),
                    quantity: q(3),
                    project_id: project.clone(),
                },
                LedgerOp::Reserve {
                    part_id: b.clone(),
                    quantity: q(4),
                    project_id: project,
                },
            ],
        )
        .unwrap();

    let reversal = db.reverse_group(&group.id, "changed plans").unwrap();
    assert_eq!(reversal.kind, "reverse:reserve_bom");
    assert_eq!(reversal.reversed_group_id.as_ref().unwrap(), &group.id);
    assert_eq!(reversal.transactions.len(), 2);
    assert_eq!(db.get_stock(&a).unwrap().reserved, q(0));
    assert_eq!(db.get_stock(&b).unwrap().reserved, q(0));
    assert_eq!(db.get_stock(&a).unwrap().available, q(10));

    assert!(matches!(
        db.reverse_group(&group.id, "").unwrap_err(),
        DbError::AlreadyReversed
    ));
}

#[test]
fn reversing_a_transfer_swaps_project_direction() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "transferred");
    let p1 = db.create_project("From").unwrap();
    let p2 = db.create_project("To").unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(6),
        project_id: p1.clone(),
    })
    .unwrap();
    let transfer = db
        .apply(&LedgerOp::TransferReservation {
            part_id: part.clone(),
            quantity: q(2),
            from_project: p1.clone(),
            to_project: p2.clone(),
        })
        .unwrap();
    let reversal = db
        .reverse_transaction(&transfer.id, "wrong project")
        .unwrap();
    assert_eq!(
        reversal.project_id.as_ref().unwrap().as_str(),
        p2.as_str(),
        "reversal must read B->A"
    );
    assert_eq!(
        reversal.to_project_id.as_ref().unwrap().as_str(),
        p1.as_str()
    );
    let s = db.get_stock(&part).unwrap();
    assert_eq!((s.available, s.reserved), (q(4), q(6)));
}

#[test]
fn reversing_unknown_transaction_is_not_found() {
    let (_g, mut db) = open();
    let err = db
        .reverse_transaction(&inventory_core::ids::TransactionId::new(), "")
        .unwrap_err();
    assert!(matches!(err, DbError::TransactionNotFound));
}

#[test]
fn reversing_a_noncommuting_group_succeeds_via_reverse_order() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "noncommuting");
    // [receive 10, consume 10] — reversing the receive first would drive available negative
    let group = db
        .apply_group(
            "receive_and_consume",
            "",
            &[
                LedgerOp::Receive {
                    part_id: part.clone(),
                    quantity: q(10),
                    note: String::new(),
                },
                LedgerOp::ConsumeAvailable {
                    part_id: part.clone(),
                    quantity: q(10),
                    project_id: None,
                    note: String::new(),
                },
            ],
        )
        .unwrap();
    db.reverse_group(&group.id, "undo").unwrap();
    let s = db.get_stock(&part).unwrap();
    assert_eq!(s.available, q(0));
    assert_eq!(s.lifetime_received, q(0));
    assert_eq!(s.lifetime_consumed, q(0));
}

#[test]
fn group_members_cannot_be_reversed_individually() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "grouped member");
    receive(&mut db, &part, 10);
    let group = db
        .apply_group(
            "batch",
            "",
            &[LedgerOp::AdjustDown {
                part_id: part.clone(),
                quantity: q(1),
                note: "recount".into(),
            }],
        )
        .unwrap();
    let member_id = group.transactions[0].id.clone();
    let err = db.reverse_transaction(&member_id, "").unwrap_err();
    assert!(matches!(err, DbError::TransactionInGroup));
    // and the group remains atomically reversible
    db.reverse_group(&group.id, "undo batch").unwrap();
    assert_eq!(db.get_stock(&part).unwrap().available, q(10));
}
