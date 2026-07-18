use inventory_core::ids::{CategoryId, PartId};
use inventory_core::quantity::QuantityUnit;
use inventory_db::parts::PartDraft;
use inventory_db::projects::{ProjectDraft, ProjectStatus};
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

fn draft(name: &str) -> ProjectDraft {
    ProjectDraft {
        name: name.to_string(),
        description: "a nifty gadget".into(),
        build_quantity: 1,
        repo_link: Some("https://example.com/repo".into()),
        notes: "some notes".into(),
    }
}

#[test]
fn create_project_full_defaults_to_planned() {
    let (_g, mut db) = open();
    let project = db.create_project_full(&draft("Blinky")).unwrap();
    assert_eq!(project.name, "Blinky");
    assert_eq!(project.status, ProjectStatus::Planned);
    assert_eq!(project.description, "a nifty gadget");
    assert_eq!(project.build_quantity, 1);
    assert_eq!(
        project.repo_link.as_deref(),
        Some("https://example.com/repo")
    );
    assert_eq!(project.notes, "some notes");
    assert!(project.completed_at.is_none());
}

#[test]
fn get_project_round_trips() {
    let (_g, mut db) = open();
    let created = db.create_project_full(&draft("Get Me")).unwrap();
    let got = db.get_project(&created.id).unwrap().unwrap();
    assert_eq!(got.id, created.id);
    assert_eq!(got.name, "Get Me");
}

#[test]
fn get_missing_project_returns_none() {
    let (_g, db) = open();
    assert!(db
        .get_project(&inventory_core::ids::ProjectId::new())
        .unwrap()
        .is_none());
}

#[test]
fn list_projects_full_orders_by_name_and_filters_by_status() {
    let (_g, mut db) = open();
    let zeta = db.create_project_full(&draft("Zeta")).unwrap();
    let alpha = db.create_project_full(&draft("Alpha")).unwrap();
    db.set_project_status(&zeta.id, ProjectStatus::Active)
        .unwrap();

    let all = db.list_projects_full(None).unwrap();
    assert_eq!(
        all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["Alpha", "Zeta"],
        "expected alphabetical order"
    );

    let planned = db.list_projects_full(Some(ProjectStatus::Planned)).unwrap();
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].id, alpha.id);

    let active = db.list_projects_full(Some(ProjectStatus::Active)).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, zeta.id);
}

#[test]
fn update_project_persists_fields_but_not_status() {
    let (_g, mut db) = open();
    let mut project = db.create_project_full(&draft("Rename Me")).unwrap();
    db.set_project_status(&project.id, ProjectStatus::Active)
        .unwrap();
    project = db.get_project(&project.id).unwrap().unwrap();

    project.name = "Renamed".into();
    project.description = "updated description".into();
    project.build_quantity = 5;
    project.repo_link = Some("https://example.com/other".into());
    project.notes = "updated notes".into();
    // update_project must not be able to smuggle a status change through
    project.status = ProjectStatus::Completed;

    db.update_project(&project).unwrap();
    let got = db.get_project(&project.id).unwrap().unwrap();
    assert_eq!(got.name, "Renamed");
    assert_eq!(got.description, "updated description");
    assert_eq!(got.build_quantity, 5);
    assert_eq!(got.repo_link.as_deref(), Some("https://example.com/other"));
    assert_eq!(got.notes, "updated notes");
    assert_eq!(
        got.status,
        ProjectStatus::Active,
        "update_project must not change status"
    );
}

#[test]
fn update_missing_project_is_not_found() {
    let (_g, mut db) = open();
    let mut ghost = db.create_project_full(&draft("Temp")).unwrap();
    db.raw_conn()
        .execute("DELETE FROM projects WHERE id = ?1", [ghost.id.as_str()])
        .unwrap();
    ghost.name = "gone".into();
    let err = db.update_project(&ghost).unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}

#[test]
fn status_lifecycle_stamps_and_clears_completed_at() {
    let (_g, mut db) = open();
    let project = db.create_project_full(&draft("Lifecycle")).unwrap();
    assert_eq!(project.status, ProjectStatus::Planned);
    assert!(project.completed_at.is_none());

    db.set_project_status(&project.id, ProjectStatus::Active)
        .unwrap();
    let active = db.get_project(&project.id).unwrap().unwrap();
    assert_eq!(active.status, ProjectStatus::Active);
    assert!(active.completed_at.is_none());

    db.set_project_status(&project.id, ProjectStatus::Completed)
        .unwrap();
    let completed = db.get_project(&project.id).unwrap().unwrap();
    assert_eq!(completed.status, ProjectStatus::Completed);
    assert!(
        completed.completed_at.is_some(),
        "entering Completed must stamp completed_at"
    );

    db.set_project_status(&project.id, ProjectStatus::Active)
        .unwrap();
    let reactivated = db.get_project(&project.id).unwrap().unwrap();
    assert_eq!(reactivated.status, ProjectStatus::Active);
    assert!(
        reactivated.completed_at.is_none(),
        "leaving Completed must clear completed_at"
    );
}

#[test]
fn archiving_a_completed_project_preserves_completed_at() {
    let (_g, mut db) = open();
    let project = db
        .create_project_full(&draft("Archive After Complete"))
        .unwrap();
    db.set_project_status(&project.id, ProjectStatus::Completed)
        .unwrap();
    let completed = db.get_project(&project.id).unwrap().unwrap();
    let stamp = completed.completed_at.clone().unwrap();

    db.set_project_status(&project.id, ProjectStatus::Archived)
        .unwrap();
    let archived = db.get_project(&project.id).unwrap().unwrap();
    assert_eq!(archived.status, ProjectStatus::Archived);
    assert_eq!(
        archived.completed_at.as_deref(),
        Some(stamp.as_str()),
        "archiving a completed project must leave completed_at untouched"
    );
}

#[test]
fn archiving_a_never_completed_project_leaves_completed_at_null() {
    let (_g, mut db) = open();
    let project = db.create_project_full(&draft("Archive Fresh")).unwrap();
    db.set_project_status(&project.id, ProjectStatus::Archived)
        .unwrap();
    let archived = db.get_project(&project.id).unwrap().unwrap();
    assert_eq!(archived.status, ProjectStatus::Archived);
    assert!(archived.completed_at.is_none());
}

#[test]
fn set_status_on_missing_project_is_not_found() {
    let (_g, mut db) = open();
    let err = db
        .set_project_status(
            &inventory_core::ids::ProjectId::new(),
            ProjectStatus::Active,
        )
        .unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}

#[test]
fn archive_project_is_a_thin_status_wrapper() {
    let (_g, mut db) = open();
    let project = db.create_project_full(&draft("Archive Wrapper")).unwrap();
    db.archive_project(&project.id).unwrap();
    let got = db.get_project(&project.id).unwrap().unwrap();
    assert_eq!(got.status, ProjectStatus::Archived);
}

// --- duplicate_project -------------------------------------------------

#[test]
fn duplicate_project_copies_fields_with_reset_status() {
    let (_g, mut db) = open();
    let original = db.create_project_full(&draft("Original")).unwrap();
    db.set_project_status(&original.id, ProjectStatus::Completed)
        .unwrap();

    let dup = db.duplicate_project(&original.id, "Copy").unwrap();
    assert_ne!(dup.id, original.id);
    assert_eq!(dup.name, "Copy");
    assert_eq!(dup.status, ProjectStatus::Planned);
    assert!(dup.completed_at.is_none());
    assert_eq!(dup.description, "a nifty gadget");
    assert_eq!(dup.build_quantity, 1);
    assert_eq!(dup.repo_link.as_deref(), Some("https://example.com/repo"));
    assert_eq!(dup.notes, "some notes");
}

#[test]
fn duplicate_project_copies_bom_items_and_substitutes_with_new_ids() {
    let (_g, mut db) = open();
    let original = db.create_project_full(&draft("Original With BOM")).unwrap();
    let resistor = make_part(&mut db, "10k resistor");
    let capacitor = make_part(&mut db, "100nF capacitor");
    let sub_resistor = make_part(&mut db, "10k resistor alt");

    let item_id = inventory_core::ids::BomItemId::new();
    db.raw_conn()
        .execute(
            "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli,
                                    reference_designators, required, notes)
             VALUES (?1, ?2, ?3, 3000, 'R1,R2,R3', 1, 'pull-ups')",
            rusqlite::params![item_id.as_str(), original.id.as_str(), resistor.as_str()],
        )
        .unwrap();
    db.raw_conn()
        .execute(
            "INSERT INTO bom_substitutes (bom_item_id, part_id) VALUES (?1, ?2)",
            rusqlite::params![item_id.as_str(), sub_resistor.as_str()],
        )
        .unwrap();
    let optional_item_id = inventory_core::ids::BomItemId::new();
    db.raw_conn()
        .execute(
            "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli,
                                    reference_designators, required, notes)
             VALUES (?1, ?2, ?3, 1000, 'C1', 0, '')",
            rusqlite::params![
                optional_item_id.as_str(),
                original.id.as_str(),
                capacitor.as_str()
            ],
        )
        .unwrap();

    let dup = db.duplicate_project(&original.id, "BOM Copy").unwrap();

    let mut stmt = db
        .raw_conn()
        .prepare(
            "SELECT id, part_id, quantity_per_build_milli, reference_designators, required, notes
             FROM bom_items WHERE project_id = ?1 ORDER BY quantity_per_build_milli DESC",
        )
        .unwrap();
    let rows: Vec<(String, String, i64, String, i64, String)> = stmt
        .query_map([dup.id.as_str()], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows.len(), 2, "both BOM lines must be copied");

    let (new_item_id, part_id, qty, refdes, required, notes) = &rows[0];
    assert_ne!(
        new_item_id,
        item_id.as_str(),
        "copied item must get a new id"
    );
    assert_eq!(part_id, resistor.as_str());
    assert_eq!(*qty, 3000);
    assert_eq!(refdes, "R1,R2,R3");
    assert_eq!(*required, 1);
    assert_eq!(notes, "pull-ups");

    let (_, opt_part_id, opt_qty, opt_refdes, opt_required, _) = &rows[1];
    assert_eq!(opt_part_id, capacitor.as_str());
    assert_eq!(*opt_qty, 1000);
    assert_eq!(opt_refdes, "C1");
    assert_eq!(*opt_required, 0);

    // substitutes followed the copied resistor line to its new id
    let sub_count: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM bom_substitutes WHERE bom_item_id = ?1 AND part_id = ?2",
            rusqlite::params![new_item_id, sub_resistor.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sub_count, 1);

    // the original project's BOM is untouched
    let original_count: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM bom_items WHERE project_id = ?1",
            [original.id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(original_count, 2);
}

#[test]
fn duplicate_project_copies_no_ledger_or_stock() {
    let (_g, mut db) = open();
    let original = db.create_project_full(&draft("Ledgered")).unwrap();
    let part = make_part(&mut db, "reserved part");
    db.apply(&inventory_core::ledger::LedgerOp::Receive {
        part_id: part.clone(),
        quantity: inventory_core::quantity::Quantity::from_whole(10).unwrap(),
        note: String::new(),
    })
    .unwrap();
    db.apply(&inventory_core::ledger::LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: inventory_core::quantity::Quantity::from_whole(4).unwrap(),
        project_id: original.id.clone(),
    })
    .unwrap();

    let dup = db
        .duplicate_project(&original.id, "No Ledger Copy")
        .unwrap();

    let txn_count: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE project_id = ?1 OR to_project_id = ?1",
            [dup.id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        txn_count, 0,
        "duplicate_project must not copy ledger/transaction rows"
    );
}

#[test]
fn duplicate_missing_project_is_not_found() {
    let (_g, mut db) = open();
    let err = db
        .duplicate_project(&inventory_core::ids::ProjectId::new(), "ghost")
        .unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}

// --- interop with the Phase 2a stub ------------------------------------

#[test]
fn stub_create_project_rows_are_readable_through_the_rich_api() {
    let (_g, mut db) = open();
    let stub_id = db.create_project("Stub Project").unwrap();
    let got = db.get_project(&stub_id).unwrap().unwrap();
    assert_eq!(got.name, "Stub Project");
    assert_eq!(got.status, ProjectStatus::Planned);
    assert_eq!(got.build_quantity, 1);

    // and the stub list_projects keeps working unchanged
    let refs = db.list_projects().unwrap();
    assert!(refs.iter().any(|r| r.id == stub_id));
}
