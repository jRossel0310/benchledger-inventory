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
fn reserve_release_round_trip() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "reservable");
    let project = db.create_project("Lightning Detector").unwrap();
    receive(&mut db, &part, 20);

    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(8),
        project_id: project.clone(),
    })
    .unwrap();
    let s = db.get_stock(&part).unwrap();
    assert_eq!((s.available, s.reserved), (q(12), q(8)));
    assert_eq!(s.current_stock(), q(20));

    db.apply(&LedgerOp::ReleaseReservation {
        part_id: part.clone(),
        quantity: q(3),
        project_id: project,
    })
    .unwrap();
    let s = db.get_stock(&part).unwrap();
    assert_eq!((s.available, s.reserved), (q(15), q(5)));
}

#[test]
fn cannot_reserve_more_than_available() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "limited");
    let project = db.create_project("P").unwrap();
    receive(&mut db, &part, 5);
    let err = db
        .apply(&LedgerOp::Reserve {
            part_id: part.clone(),
            quantity: q(6),
            project_id: project,
        })
        .unwrap_err();
    assert!(matches!(err, DbError::InsufficientStock(_)));
}

#[test]
fn checkout_and_return_round_trip() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "dev board");
    let project = db.create_project("Bench").unwrap();
    receive(&mut db, &part, 2);

    db.apply(&LedgerOp::CheckOut {
        part_id: part.clone(),
        quantity: q(1),
        project_id: project.clone(),
    })
    .unwrap();
    let s = db.get_stock(&part).unwrap();
    assert_eq!((s.available, s.checked_out), (q(1), q(1)));

    db.apply(&LedgerOp::Return {
        part_id: part.clone(),
        quantity: q(1),
        project_id: project,
    })
    .unwrap();
    let s = db.get_stock(&part).unwrap();
    assert_eq!((s.available, s.checked_out), (q(2), q(0)));
}

#[test]
fn consume_reserved_and_checked_out() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "consumables");
    let project = db.create_project("Build").unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(4),
        project_id: project.clone(),
    })
    .unwrap();
    db.apply(&LedgerOp::CheckOut {
        part_id: part.clone(),
        quantity: q(2),
        project_id: project.clone(),
    })
    .unwrap();

    db.apply(&LedgerOp::ConsumeReserved {
        part_id: part.clone(),
        quantity: q(4),
        project_id: Some(project.clone()),
        note: String::new(),
    })
    .unwrap();
    db.apply(&LedgerOp::ConsumeCheckedOut {
        part_id: part.clone(),
        quantity: q(1),
        project_id: Some(project),
        note: "fried it".into(),
    })
    .unwrap();

    let s = db.get_stock(&part).unwrap();
    assert_eq!((s.available, s.reserved, s.checked_out), (q(4), q(0), q(1)));
    assert_eq!(s.lifetime_consumed, q(5));
    assert_eq!(s.current_stock(), q(5));
}

#[test]
fn transfer_reservation_records_both_projects_and_keeps_totals() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "shared res");
    let p1 = db.create_project("From").unwrap();
    let p2 = db.create_project("To").unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(6),
        project_id: p1.clone(),
    })
    .unwrap();
    db.apply(&LedgerOp::TransferReservation {
        part_id: part.clone(),
        quantity: q(2),
        from_project: p1.clone(),
        to_project: p2.clone(),
    })
    .unwrap();
    let s = db.get_stock(&part).unwrap();
    assert_eq!((s.available, s.reserved), (q(4), q(6)));
    let txns = db.list_transactions(&part).unwrap();
    let transfer = txns
        .iter()
        .find(|t| t.txn_type == "transfer_reservation")
        .unwrap();
    assert_eq!(transfer.project_id.as_ref().unwrap().as_str(), p1.as_str());
    assert_eq!(
        transfer.to_project_id.as_ref().unwrap().as_str(),
        p2.as_str()
    );
}

#[test]
fn transfer_cannot_exceed_reserved_quantity() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "bounded transfer");
    let p1 = db.create_project("From").unwrap();
    let p2 = db.create_project("To").unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(6),
        project_id: p1.clone(),
    })
    .unwrap();
    let err = db
        .apply(&LedgerOp::TransferReservation {
            part_id: part.clone(),
            quantity: q(7),
            from_project: p1.clone(),
            to_project: p2.clone(),
        })
        .unwrap_err();
    assert!(matches!(err, DbError::InsufficientStock(_)));
    // exact bound is fine
    db.apply(&LedgerOp::TransferReservation {
        part_id: part.clone(),
        quantity: q(6),
        from_project: p1,
        to_project: p2,
    })
    .unwrap();
}

#[test]
fn zero_quantity_operations_are_rejected_with_typed_error() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "zero qty");
    let err = db
        .apply(&LedgerOp::Receive {
            part_id: part.clone(),
            quantity: inventory_core::quantity::Quantity::ZERO,
            note: String::new(),
        })
        .unwrap_err();
    assert!(matches!(
        err,
        DbError::Ledger(inventory_core::ledger::LedgerError::ZeroQuantity)
    ));
    assert_eq!(db.list_transactions(&part).unwrap().len(), 0);
}

#[test]
fn archived_part_rejects_new_allocation_but_allows_return_and_release() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "sunset part");
    let project = db.create_project("P").unwrap();
    receive(&mut db, &part, 5);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(2),
        project_id: project.clone(),
    })
    .unwrap();
    db.apply(&LedgerOp::CheckOut {
        part_id: part.clone(),
        quantity: q(1),
        project_id: project.clone(),
    })
    .unwrap();

    db.set_part_archived(&part, true).unwrap();

    let rejected = db.apply(&LedgerOp::Receive {
        part_id: part.clone(),
        quantity: q(1),
        note: String::new(),
    });
    assert!(matches!(rejected.unwrap_err(), DbError::PartArchived));
    let rejected = db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(1),
        project_id: project.clone(),
    });
    assert!(matches!(rejected.unwrap_err(), DbError::PartArchived));

    db.apply(&LedgerOp::ReleaseReservation {
        part_id: part.clone(),
        quantity: q(2),
        project_id: project.clone(),
    })
    .unwrap();
    db.apply(&LedgerOp::Return {
        part_id: part.clone(),
        quantity: q(1),
        project_id: project,
    })
    .unwrap();
    let s = db.get_stock(&part).unwrap();
    assert_eq!(s.available, q(5));
}

#[test]
fn unknown_project_is_a_typed_error() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "orphan project op");
    receive(&mut db, &part, 5);
    let err = db
        .apply(&LedgerOp::Reserve {
            part_id: part,
            quantity: q(1),
            project_id: inventory_core::ids::ProjectId::new(),
        })
        .unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}

#[test]
fn archived_part_rejects_consume_adjust_and_transfer() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "fully archived");
    let p1 = db.create_project("A").unwrap();
    let p2 = db.create_project("B").unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(2),
        project_id: p1.clone(),
    })
    .unwrap();
    db.set_part_archived(&part, true).unwrap();
    for op in [
        LedgerOp::ConsumeAvailable {
            part_id: part.clone(),
            quantity: q(1),
            project_id: None,
            note: String::new(),
        },
        LedgerOp::ConsumeReserved {
            part_id: part.clone(),
            quantity: q(1),
            project_id: Some(p1.clone()),
            note: String::new(),
        },
        LedgerOp::AdjustUp {
            part_id: part.clone(),
            quantity: q(1),
            note: "n".into(),
        },
        LedgerOp::AdjustDown {
            part_id: part.clone(),
            quantity: q(1),
            note: "n".into(),
        },
        LedgerOp::TransferReservation {
            part_id: part.clone(),
            quantity: q(1),
            from_project: p1,
            to_project: p2,
        },
    ] {
        let err = db.apply(&op).unwrap_err();
        assert!(
            matches!(err, DbError::PartArchived),
            "op {:?} should be rejected",
            op.txn_type_sql()
        );
    }
}

#[test]
fn transactions_read_back_with_real_quantity_unit() {
    let (_g, mut db) = open();
    // Meter part with fractional quantity must read back exactly
    let draft = inventory_db::parts::PartDraft {
        display_name: "hookup wire".into(),
        category_id: inventory_core::ids::CategoryId::from_string(MISC_CATEGORY_ID.into()).unwrap(),
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Meter,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    let part = db.create_part(&draft).unwrap().id;
    db.apply(&LedgerOp::Receive {
        part_id: part.clone(),
        quantity: Quantity::from_milli(2_500, QuantityUnit::Meter).unwrap(),
        note: String::new(),
    })
    .unwrap();
    let txns = db.list_transactions(&part).unwrap();
    assert_eq!(txns[0].quantity.as_milli(), 2_500);
}
