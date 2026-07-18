use inventory_core::ids::{BomItemId, CategoryId, PartId, ProjectId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::bom::BomItemDraft;
use inventory_db::parts::PartDraft;
use inventory_db::projects::ProjectDraft;
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
        category_id: CategoryId::from_string(MISC_CATEGORY_ID.into()).unwrap(),
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

fn make_project(db: &mut Database, name: &str, build_quantity: i64) -> ProjectId {
    let draft = ProjectDraft {
        name: name.to_string(),
        description: String::new(),
        build_quantity,
        repo_link: None,
        notes: String::new(),
    };
    db.create_project_full(&draft).unwrap().id
}

fn q(n: i64) -> Quantity {
    Quantity::from_whole(n).unwrap()
}

fn item_draft(part_id: &PartId, qty_per_build: i64) -> BomItemDraft {
    BomItemDraft {
        part_id: part_id.clone(),
        quantity_per_build: q(qty_per_build),
        reference_designators: "R1".into(),
        required: true,
        notes: String::new(),
    }
}

fn receive(db: &mut Database, part: &PartId, n: i64) {
    db.apply(&LedgerOp::Receive {
        part_id: part.clone(),
        quantity: q(n),
        note: String::new(),
    })
    .unwrap();
}

// --- add / get / update / remove ---------------------------------------

#[test]
fn add_bom_item_round_trips_through_get_and_list() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Blinky", 1);
    let part = make_part(&mut db, "10k resistor");

    let added = db.add_bom_item(&project, &item_draft(&part, 2)).unwrap();
    assert_eq!(added.project_id, project);
    assert_eq!(added.part_id, part);
    assert_eq!(added.part_display_name, "10k resistor");
    assert_eq!(added.quantity_per_build, q(2));
    assert_eq!(added.total_required, q(2), "build_quantity=1");
    assert_eq!(added.reference_designators, "R1");
    assert!(added.required);
    assert!(added.substitutes.is_empty());
    assert_eq!(added.reserved, Quantity::ZERO);
    assert_eq!(added.consumed, Quantity::ZERO);
    assert_eq!(added.available, Quantity::ZERO);
    assert_eq!(added.missing, q(2));

    let got = db.get_bom_item(&added.id).unwrap().unwrap();
    assert_eq!(got.id, added.id);
    assert_eq!(got.part_display_name, "10k resistor");

    let list = db.list_bom(&project).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, added.id);
}

#[test]
fn get_missing_bom_item_returns_none() {
    let (_g, db) = open();
    assert!(db.get_bom_item(&BomItemId::new()).unwrap().is_none());
}

#[test]
fn add_bom_item_rejects_missing_project() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "orphan part");
    let err = db
        .add_bom_item(&ProjectId::new(), &item_draft(&part, 1))
        .unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}

#[test]
fn add_bom_item_rejects_missing_part() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "No Part", 1);
    let err = db
        .add_bom_item(&project, &item_draft(&PartId::new(), 1))
        .unwrap_err();
    assert!(matches!(err, DbError::PartNotFound));
}

#[test]
fn duplicate_project_part_is_rejected() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Dup", 1);
    let part = make_part(&mut db, "shared part");
    db.add_bom_item(&project, &item_draft(&part, 1)).unwrap();
    let err = db
        .add_bom_item(&project, &item_draft(&part, 3))
        .unwrap_err();
    assert!(matches!(err, DbError::BomItemExists), "got {err:?}");
}

#[test]
fn update_bom_item_persists_fields_but_not_part_id() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Update Me", 1);
    let part = make_part(&mut db, "original part");
    let other_part = make_part(&mut db, "other part");
    let added = db.add_bom_item(&project, &item_draft(&part, 1)).unwrap();

    let mut new_draft = item_draft(&other_part, 5);
    new_draft.reference_designators = "R2,R3".into();
    new_draft.required = false;
    new_draft.notes = "updated".into();
    let updated = db.update_bom_item(&added.id, &new_draft).unwrap();

    assert_eq!(updated.quantity_per_build, q(5));
    assert_eq!(updated.reference_designators, "R2,R3");
    assert!(!updated.required);
    assert_eq!(updated.notes, "updated");
    assert_eq!(
        updated.part_id, part,
        "update_bom_item must not change part_id"
    );
}

#[test]
fn update_missing_bom_item_is_not_found() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "ghost part");
    let err = db
        .update_bom_item(&BomItemId::new(), &item_draft(&part, 1))
        .unwrap_err();
    assert!(matches!(err, DbError::BomItemNotFound));
}

#[test]
fn remove_bom_item_deletes_it() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Remove Me", 1);
    let part = make_part(&mut db, "removable part");
    let added = db.add_bom_item(&project, &item_draft(&part, 1)).unwrap();

    db.remove_bom_item(&added.id).unwrap();
    assert!(db.get_bom_item(&added.id).unwrap().is_none());
    assert!(db.list_bom(&project).unwrap().is_empty());
}

#[test]
fn remove_missing_bom_item_is_not_found() {
    let (_g, mut db) = open();
    let err = db.remove_bom_item(&BomItemId::new()).unwrap_err();
    assert!(matches!(err, DbError::BomItemNotFound));
}

// --- total_required recomputation ---------------------------------------

#[test]
fn total_required_recomputes_when_build_quantity_changes() {
    let (_g, mut db) = open();
    let project_ref = make_project(&mut db, "Batch", 1);
    let part = make_part(&mut db, "batched part");
    let added = db
        .add_bom_item(&project_ref, &item_draft(&part, 2))
        .unwrap();
    assert_eq!(added.total_required, q(2));

    let mut project = db.get_project(&project_ref).unwrap().unwrap();
    project.build_quantity = 5;
    db.update_project(&project).unwrap();

    let list = db.list_bom(&project_ref).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        list[0].total_required,
        q(10),
        "2 per build x 5 build_quantity"
    );
}

#[test]
fn total_required_recomputes_when_quantity_per_build_changes() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Requant", 3);
    let part = make_part(&mut db, "requant part");
    let added = db.add_bom_item(&project, &item_draft(&part, 1)).unwrap();
    assert_eq!(added.total_required, q(3));

    let updated = db
        .update_bom_item(&added.id, &item_draft(&part, 4))
        .unwrap();
    assert_eq!(
        updated.total_required,
        q(12),
        "4 per build x 3 build_quantity"
    );
}

// --- substitutes ----------------------------------------------------------

#[test]
fn set_bom_substitutes_replaces_the_full_list() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Subs", 1);
    let part = make_part(&mut db, "primary part");
    let sub_a = make_part(&mut db, "sub a");
    let sub_b = make_part(&mut db, "sub b");
    let added = db.add_bom_item(&project, &item_draft(&part, 1)).unwrap();

    db.set_bom_substitutes(&added.id, &[sub_a.clone(), sub_b.clone()])
        .unwrap();
    let got = db.get_bom_item(&added.id).unwrap().unwrap();
    let mut subs = got.substitutes.clone();
    subs.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    let mut expected = vec![sub_a.clone(), sub_b.clone()];
    expected.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    assert_eq!(subs, expected);

    // replacing drops the old set entirely
    db.set_bom_substitutes(&added.id, &[sub_a.clone()]).unwrap();
    let got2 = db.get_bom_item(&added.id).unwrap().unwrap();
    assert_eq!(got2.substitutes, vec![sub_a]);
}

#[test]
fn set_bom_substitutes_on_missing_item_is_not_found() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "lonely sub");
    let err = db
        .set_bom_substitutes(&BomItemId::new(), &[part])
        .unwrap_err();
    assert!(matches!(err, DbError::BomItemNotFound));
}

#[test]
fn set_bom_substitutes_rejects_missing_part() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Bad Sub", 1);
    let part = make_part(&mut db, "primary part 2");
    let added = db.add_bom_item(&project, &item_draft(&part, 1)).unwrap();
    let err = db
        .set_bom_substitutes(&added.id, &[PartId::new()])
        .unwrap_err();
    assert!(matches!(err, DbError::PartNotFound));
}

// --- derived reserved / consumed -----------------------------------------

#[test]
fn derived_reserved_reflects_reserve_and_release_for_the_project() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reserve Flow", 1);
    let part = make_part(&mut db, "reserved part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 20);

    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(10),
        project_id: project.clone(),
    })
    .unwrap();
    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].reserved, q(10));
    assert_eq!(list[0].available, q(10), "20 received - 10 reserved");

    db.apply(&LedgerOp::ReleaseReservation {
        part_id: part.clone(),
        quantity: q(4),
        project_id: project.clone(),
    })
    .unwrap();
    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].reserved,
        q(6),
        "release must drop the derived reserved amount"
    );
    assert_eq!(list[0].available, q(14));
}

#[test]
fn derived_reserved_drops_and_consumed_rises_when_reserved_stock_is_consumed() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Consume Flow", 1);
    let part = make_part(&mut db, "consumed part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(10),
        project_id: project.clone(),
    })
    .unwrap();

    db.apply(&LedgerOp::ConsumeReserved {
        part_id: part.clone(),
        quantity: q(3),
        project_id: Some(project.clone()),
        note: "built 3".into(),
    })
    .unwrap();

    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].reserved,
        q(7),
        "consuming reserved stock ends that portion of the reservation"
    );
    assert_eq!(list[0].consumed, q(3));

    // consume_available also counts toward `consumed` (but not `reserved`)
    receive(&mut db, &part, 5);
    db.apply(&LedgerOp::ConsumeAvailable {
        part_id: part.clone(),
        quantity: q(2),
        project_id: Some(project.clone()),
        note: "built 2 more from spare stock".into(),
    })
    .unwrap();
    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].reserved,
        q(7),
        "consume_available must not touch reserved"
    );
    assert_eq!(
        list[0].consumed,
        q(5),
        "3 consume_reserved + 2 consume_available"
    );
}

#[test]
fn derived_reserved_and_consumed_are_scoped_per_project() {
    let (_g, mut db) = open();
    let project_a = make_project(&mut db, "Project A", 1);
    let project_b = make_project(&mut db, "Project B", 1);
    let part = make_part(&mut db, "shared across projects");
    db.add_bom_item(&project_a, &item_draft(&part, 5)).unwrap();
    db.add_bom_item(&project_b, &item_draft(&part, 5)).unwrap();
    receive(&mut db, &part, 20);

    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(8),
        project_id: project_a.clone(),
    })
    .unwrap();

    let list_a = db.list_bom(&project_a).unwrap();
    let list_b = db.list_bom(&project_b).unwrap();
    assert_eq!(list_a[0].reserved, q(8));
    assert_eq!(
        list_b[0].reserved,
        Quantity::ZERO,
        "project B never reserved this part"
    );
}

#[test]
fn reversed_reserve_no_longer_counts_toward_derived_reserved() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reversal Flow", 1);
    let part = make_part(&mut db, "reversed part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 10);
    let txn = db
        .apply(&LedgerOp::Reserve {
            part_id: part.clone(),
            quantity: q(6),
            project_id: project.clone(),
        })
        .unwrap();

    assert_eq!(db.list_bom(&project).unwrap()[0].reserved, q(6));

    db.reverse_transaction(&txn.id, "undo reserve").unwrap();
    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].reserved,
        Quantity::ZERO,
        "reversing the reserve must zero out the derived reserved amount"
    );
}

#[test]
fn reverse_of_consume_reserved_restores_reserved_and_clears_consumed() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reverse Consume Reserved", 1);
    let part = make_part(&mut db, "reverse consume reserved part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(10),
        project_id: project.clone(),
    })
    .unwrap();
    let consume_txn = db
        .apply(&LedgerOp::ConsumeReserved {
            part_id: part.clone(),
            quantity: q(4),
            project_id: Some(project.clone()),
            note: "built 4".into(),
        })
        .unwrap();

    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].reserved, q(6), "10 reserved - 4 consumed");
    assert_eq!(list[0].consumed, q(4));

    db.reverse_transaction(&consume_txn.id, "undo consume_reserved")
        .unwrap();
    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].consumed,
        Quantity::ZERO,
        "reversing consume_reserved must clear the derived consumed amount"
    );
    assert_eq!(
        list[0].reserved,
        q(10),
        "reversing consume_reserved must re-add the quantity back to reserved"
    );
}

#[test]
fn reverse_of_release_reservation_restores_reserved() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reverse Release", 1);
    let part = make_part(&mut db, "reverse release part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(10),
        project_id: project.clone(),
    })
    .unwrap();
    let release_txn = db
        .apply(&LedgerOp::ReleaseReservation {
            part_id: part.clone(),
            quantity: q(3),
            project_id: project.clone(),
        })
        .unwrap();

    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].reserved, q(7), "10 reserved - 3 released");

    db.reverse_transaction(&release_txn.id, "undo release")
        .unwrap();
    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].reserved,
        q(10),
        "reversing release_reservation must re-add the released quantity back to reserved"
    );
}

#[test]
fn reverse_of_consume_available_clears_consumed() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reverse Consume Available", 1);
    let part = make_part(&mut db, "reverse consume available part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 10);
    let consume_txn = db
        .apply(&LedgerOp::ConsumeAvailable {
            part_id: part.clone(),
            quantity: q(5),
            project_id: Some(project.clone()),
            note: "built 5 from spare stock".into(),
        })
        .unwrap();

    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].consumed, q(5));

    db.reverse_transaction(&consume_txn.id, "undo consume_available")
        .unwrap();
    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].consumed,
        Quantity::ZERO,
        "reversing consume_available must clear the derived consumed amount"
    );
}

// --- missing --------------------------------------------------------------

#[test]
fn missing_accounts_for_available_and_reserved() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Missing Calc", 2);
    let part = make_part(&mut db, "missing calc part");
    let added = db.add_bom_item(&project, &item_draft(&part, 5)).unwrap();
    assert_eq!(
        added.total_required,
        q(10),
        "5 per build x build_quantity 2"
    );
    assert_eq!(added.missing, q(10), "nothing available or reserved yet");

    receive(&mut db, &part, 4);
    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].missing, q(6), "10 required - 4 available");

    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(4),
        project_id: project.clone(),
    })
    .unwrap();
    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].missing,
        q(6),
        "reserving moves stock from available to reserved; total covered stays 4"
    );

    receive(&mut db, &part, 6);
    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].missing,
        Quantity::ZERO,
        "fully covered, clamped at 0"
    );
}

#[test]
fn missing_never_goes_negative_when_overstocked() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Overstock", 1);
    let part = make_part(&mut db, "overstocked part");
    db.add_bom_item(&project, &item_draft(&part, 2)).unwrap();
    receive(&mut db, &part, 100);
    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].missing, Quantity::ZERO);
}

#[test]
fn missing_subtracts_consumed_for_a_fully_built_line() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Missing Fully Built", 1);
    let part = make_part(&mut db, "fully built part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 10);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(10),
        project_id: project.clone(),
    })
    .unwrap();

    // Build the whole line: every reserved unit gets consumed, nothing else
    // is held (reserved and available both drop to 0).
    db.apply(&LedgerOp::ConsumeReserved {
        part_id: part.clone(),
        quantity: q(10),
        project_id: Some(project.clone()),
        note: "built all 10".into(),
    })
    .unwrap();

    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].consumed, q(10));
    assert_eq!(list[0].reserved, Quantity::ZERO);
    assert_eq!(list[0].available, Quantity::ZERO);
    assert_eq!(
        list[0].missing,
        Quantity::ZERO,
        "10 required - 10 consumed - 0 reserved - 0 available = 0: a fully-built \
         line must not read as fully missing again"
    );
}

#[test]
fn missing_reflects_remaining_shortfall_for_a_partially_built_line() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Missing Partially Built", 1);
    let part = make_part(&mut db, "partially built part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 4);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(4),
        project_id: project.clone(),
    })
    .unwrap();

    // Only 4 of the 10 required have been built so far; the rest hasn't been
    // received, reserved, or consumed yet.
    db.apply(&LedgerOp::ConsumeReserved {
        part_id: part.clone(),
        quantity: q(4),
        project_id: Some(project.clone()),
        note: "built 4 of 10".into(),
    })
    .unwrap();

    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].consumed, q(4));
    assert_eq!(list[0].reserved, Quantity::ZERO);
    assert_eq!(list[0].available, Quantity::ZERO);
    assert_eq!(
        list[0].missing,
        q(6),
        "10 required - 4 consumed - 0 reserved - 0 available = 6 still needed"
    );
}

// --- list ordering ----------------------------------------------------

#[test]
fn list_bom_orders_by_part_display_name() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Ordering", 1);
    let zeta = make_part(&mut db, "Zeta part");
    let alpha = make_part(&mut db, "Alpha part");
    db.add_bom_item(&project, &item_draft(&zeta, 1)).unwrap();
    db.add_bom_item(&project, &item_draft(&alpha, 1)).unwrap();

    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list.iter()
            .map(|i| i.part_display_name.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha part", "Zeta part"]
    );
}

#[test]
fn list_bom_rejects_missing_project() {
    let (_g, db) = open();
    let err = db.list_bom(&ProjectId::new()).unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}

// --- import_bom (simple bulk-add stub) ------------------------------------

#[test]
fn import_bom_bulk_adds_and_skips_duplicates_and_missing_parts() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Import", 1);
    let existing_part = make_part(&mut db, "already on bom");
    let new_part = make_part(&mut db, "new from import");
    db.add_bom_item(&project, &item_draft(&existing_part, 1))
        .unwrap();

    let rows = vec![
        item_draft(&existing_part, 9), // duplicate (project, part) -> skipped
        item_draft(&new_part, 3),      // added
        item_draft(&PartId::new(), 1), // part doesn't exist -> skipped
    ];
    let added = db.import_bom(&project, rows).unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].part_id, new_part);
    assert_eq!(added[0].quantity_per_build, q(3));

    let list = db.list_bom(&project).unwrap();
    assert_eq!(list.len(), 2, "existing line untouched, one new line added");
}

#[test]
fn import_bom_rejects_missing_project() {
    let (_g, mut db) = open();
    let part = make_part(&mut db, "orphan import part");
    let err = db
        .import_bom(&ProjectId::new(), vec![item_draft(&part, 1)])
        .unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}
