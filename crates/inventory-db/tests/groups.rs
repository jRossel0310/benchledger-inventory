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
fn group_applies_all_operations_atomically() {
    let (_g, mut db) = open();
    let a = make_part(&mut db, "part a");
    let b = make_part(&mut db, "part b");
    receive(&mut db, &a, 10);
    receive(&mut db, &b, 10);
    let project = db.create_project("BOM build").unwrap();

    let group = db
        .apply_group(
            "reserve_bom",
            "reserve for BOM build",
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
    assert_eq!(group.kind, "reserve_bom");
    assert_eq!(group.transactions.len(), 2);
    assert!(group
        .transactions
        .iter()
        .all(|t| t.group_id.as_ref() == Some(&group.id)));
    assert_eq!(db.get_stock(&a).unwrap().reserved, q(3));
    assert_eq!(db.get_stock(&b).unwrap().reserved, q(4));
}

#[test]
fn failing_member_rolls_back_the_entire_group() {
    let (_g, mut db) = open();
    let a = make_part(&mut db, "part a");
    let b = make_part(&mut db, "part b");
    receive(&mut db, &a, 10);
    receive(&mut db, &b, 2); // not enough for the group below
    let project = db.create_project("doomed").unwrap();

    let err = db
        .apply_group(
            "reserve_bom",
            "",
            &[
                LedgerOp::Reserve {
                    part_id: a.clone(),
                    quantity: q(5),
                    project_id: project.clone(),
                },
                LedgerOp::Reserve {
                    part_id: b.clone(),
                    quantity: q(5),
                    project_id: project,
                },
            ],
        )
        .unwrap_err();
    assert!(matches!(err, DbError::InsufficientStock(_)));

    // nothing moved, nothing recorded
    assert_eq!(db.get_stock(&a).unwrap().reserved, q(0));
    assert_eq!(db.get_stock(&a).unwrap().available, q(10));
    assert_eq!(db.list_transactions(&a).unwrap().len(), 1); // just the receive
    let group_count: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM transaction_groups", [], |r| r.get(0))
        .unwrap();
    assert_eq!(group_count, 0);
}

#[test]
fn empty_group_is_rejected() {
    let (_g, mut db) = open();
    assert!(matches!(
        db.apply_group("noop", "", &[]).unwrap_err(),
        DbError::EmptyGroup
    ));
}

#[test]
fn get_group_round_trips_with_members() {
    let (_g, mut db) = open();
    let a = make_part(&mut db, "grouped");
    receive(&mut db, &a, 10);
    let group = db
        .apply_group(
            "adjustment_batch",
            "annual recount",
            &[LedgerOp::AdjustDown {
                part_id: a.clone(),
                quantity: q(1),
                note: "recount".into(),
            }],
        )
        .unwrap();
    let got = db.get_group(&group.id).unwrap().unwrap();
    assert_eq!(got.kind, "adjustment_batch");
    assert_eq!(got.note, "annual recount");
    assert_eq!(got.transactions.len(), 1);
    assert!(db
        .get_group(&inventory_core::ids::GroupId::new())
        .unwrap()
        .is_none());
}

#[test]
fn get_group_returns_members_in_application_order() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "ordered");
    receive(&mut db, &part, 10);
    let project = db.create_project("ordering").unwrap();
    let group = db
        .apply_group(
            "ordering_test",
            "",
            &[
                LedgerOp::Reserve {
                    part_id: part.clone(),
                    quantity: q(1),
                    project_id: project.clone(),
                },
                LedgerOp::Reserve {
                    part_id: part.clone(),
                    quantity: q(2),
                    project_id: project.clone(),
                },
                LedgerOp::Reserve {
                    part_id: part.clone(),
                    quantity: q(3),
                    project_id: project,
                },
            ],
        )
        .unwrap();
    let fetched = db.get_group(&group.id).unwrap().unwrap();
    let applied_ids: Vec<&str> = group.transactions.iter().map(|t| t.id.as_str()).collect();
    let fetched_ids: Vec<&str> = fetched.transactions.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        fetched_ids, applied_ids,
        "get_group must return members in application order"
    );
    let quantities: Vec<i64> = fetched
        .transactions
        .iter()
        .map(|t| t.quantity.as_milli())
        .collect();
    assert_eq!(quantities, vec![1_000, 2_000, 3_000]);
}
