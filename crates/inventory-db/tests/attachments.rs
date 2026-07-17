use inventory_core::quantity::QuantityUnit;
use inventory_db::attachment_store::{AttachmentKind, AttachmentRef};
use inventory_db::dimensions::{DimensionDraft, DimensionGroup, DimensionSource};
use inventory_db::parts::PartDraft;
use inventory_db::{Database, DbError, MISC_CATEGORY_ID};

/// Open a database plus a separate, initially-empty attachments directory, so
/// tests can count files in it without the sqlite files getting in the way.
fn open() -> (tempfile::TempDir, Database, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    let attachments = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments).unwrap();
    (dir, db, attachments)
}

fn part(db: &mut Database) -> inventory_core::ids::PartId {
    let draft = PartDraft {
        display_name: "part with attachments".into(),
        category_id: inventory_core::ids::CategoryId::from_string(MISC_CATEGORY_ID.into()).unwrap(),
        description: String::new(),
        bin_label: None,
        usage_behavior: "ask".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    db.create_part(&draft).unwrap().id
}

fn file_count(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

#[test]
fn sha256_hash_is_stable_and_correct() {
    let (_g, db, dir) = open();
    // Known-answer test: SHA-256("hello") is a fixed, well-published digest.
    let a = db
        .store_attachment(&dir, b"hello", Some("txt"), AttachmentKind::Other, None, "")
        .unwrap();
    assert_eq!(
        a.content_hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    assert_eq!(a.size_bytes, 5);
    assert_eq!(a.ext.as_deref(), Some("txt"));
}

#[test]
fn identical_bytes_dedupe_to_one_file_and_one_row() {
    let (_g, db, dir) = open();
    let bytes = b"the same picture bytes";
    let first = db
        .store_attachment(
            &dir,
            bytes,
            Some("png"),
            AttachmentKind::Photo,
            Some("a.png"),
            "upload",
        )
        .unwrap();
    // Store the identical bytes a second time — even with a different extension
    // and kind. The content hash is the identity, so this must NOT fork a new
    // file or row; the first writer's metadata stays canonical.
    let second = db
        .store_attachment(
            &dir,
            bytes,
            Some("jpg"),
            AttachmentKind::Datasheet,
            Some("b.jpg"),
            "other",
        )
        .unwrap();

    assert_eq!(first.content_hash, second.content_hash);
    assert_eq!(
        second.ext.as_deref(),
        Some("png"),
        "first writer's ext wins"
    );
    assert_eq!(second.kind, AttachmentKind::Photo);
    assert_eq!(second.original_name.as_deref(), Some("a.png"));

    assert_eq!(file_count(&dir), 1, "identical bytes must yield one file");
    let rows: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1, "identical bytes must yield one metadata row");
}

#[test]
fn distinct_bytes_produce_distinct_files() {
    let (_g, db, dir) = open();
    db.store_attachment(&dir, b"one", Some("bin"), AttachmentKind::Other, None, "")
        .unwrap();
    db.store_attachment(&dir, b"two", Some("bin"), AttachmentKind::Other, None, "")
        .unwrap();
    assert_eq!(file_count(&dir), 2);
}

#[test]
fn read_attachment_round_trips_the_bytes() {
    let (_g, db, dir) = open();
    let bytes: &[u8] = &[0u8, 1, 2, 3, 255, 128, 64];
    let stored = db
        .store_attachment(&dir, bytes, Some("dat"), AttachmentKind::Other, None, "")
        .unwrap();
    let read = db.read_attachment(&dir, &stored.content_hash).unwrap();
    assert_eq!(read, bytes);

    // The file lands under attachments/<hash>.<ext>.
    let path = db.attachment_path(&dir, &stored.content_hash).unwrap();
    assert_eq!(
        path.file_name().unwrap().to_str().unwrap(),
        format!("{}.dat", stored.content_hash)
    );
    assert!(path.exists());
}

#[test]
fn no_extension_stores_and_reads_by_bare_hash() {
    let (_g, db, dir) = open();
    let stored = db
        .store_attachment(&dir, b"no ext here", None, AttachmentKind::Other, None, "")
        .unwrap();
    assert_eq!(stored.ext, None);
    let path = db.attachment_path(&dir, &stored.content_hash).unwrap();
    assert_eq!(
        path.file_name().unwrap().to_str().unwrap(),
        stored.content_hash
    );
    assert_eq!(
        db.read_attachment(&dir, &stored.content_hash).unwrap(),
        b"no ext here"
    );
}

#[test]
fn attach_list_and_remove_part_links() {
    let (_g, mut db, dir) = open();
    let p = part(&mut db);
    let a = db
        .store_attachment(
            &dir,
            b"datasheet pdf bytes",
            Some("pdf"),
            AttachmentKind::Datasheet,
            Some("ds.pdf"),
            "upload",
        )
        .unwrap();

    db.attach_to_part(&p, &a.content_hash).unwrap();
    // Attaching the same blob to the same part again is idempotent.
    db.attach_to_part(&p, &a.content_hash).unwrap();

    let listed: Vec<AttachmentRef> = db.list_part_attachments(&p).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].content_hash, a.content_hash);
    assert_eq!(listed[0].kind, AttachmentKind::Datasheet);
    assert_eq!(listed[0].original_name.as_deref(), Some("ds.pdf"));

    db.remove_part_attachment(&p, &a.content_hash).unwrap();
    assert!(db.list_part_attachments(&p).unwrap().is_empty());
    // Removing the link leaves the shared blob row (and file) intact.
    let rows: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
    assert!(db.read_attachment(&dir, &a.content_hash).is_ok());
}

#[test]
fn attach_to_missing_part_or_attachment_is_a_typed_error() {
    let (_g, mut db, dir) = open();
    let p = part(&mut db);
    let a = db
        .store_attachment(&dir, b"x", Some("bin"), AttachmentKind::Other, None, "")
        .unwrap();

    let missing_part = inventory_core::ids::PartId::new();
    assert!(matches!(
        db.attach_to_part(&missing_part, &a.content_hash),
        Err(DbError::PartNotFound)
    ));
    assert!(matches!(
        db.attach_to_part(&p, "not-a-real-hash"),
        Err(DbError::AttachmentNotFound)
    ));
    assert!(matches!(
        db.read_attachment(&dir, "not-a-real-hash"),
        Err(DbError::AttachmentNotFound)
    ));
}

#[test]
fn deleting_a_part_cascades_its_attachment_links_but_keeps_blobs() {
    let (_g, db, dir) = open();
    // A bare `parts` row (no part_stock/search_text) is enough for `get_part`
    // and `attach_to_part`, and lets the raw DELETE below exercise the
    // `part_attachments` cascade in isolation without other parts-referencing
    // rows (which don't all cascade) blocking it.
    let pid = "00000000000000000000000001";
    db.raw_conn()
        .execute(
            "INSERT INTO parts (id, display_name, category_id) VALUES (?1, 'cascade part', ?2)",
            rusqlite::params![pid, MISC_CATEGORY_ID],
        )
        .unwrap();
    let p = inventory_core::ids::PartId::from_string(pid.into()).unwrap();
    let a = db
        .store_attachment(
            &dir,
            b"shared blob",
            Some("bin"),
            AttachmentKind::Other,
            None,
            "",
        )
        .unwrap();
    db.attach_to_part(&p, &a.content_hash).unwrap();

    db.raw_conn()
        .execute("DELETE FROM parts WHERE id = ?1", [p.as_str()])
        .unwrap();

    let links: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM part_attachments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(links, 0, "part delete must cascade the link");
    // Blob metadata + file survive so any other reference stays valid.
    assert!(db.read_attachment(&dir, &a.content_hash).is_ok());
}

#[test]
fn dimension_attachment_can_be_wired_to_a_blob() {
    let (_g, mut db, dir) = open();
    let p = part(&mut db);
    let dim = db
        .add_dimension(
            &p,
            &DimensionDraft {
                group: DimensionGroup::Overall,
                name: "Length".into(),
                raw_value: "5 mm".into(),
                source: DimensionSource::Measured,
                notes: String::new(),
                measured_date: None,
            },
        )
        .unwrap();
    let photo = db
        .store_attachment(
            &dir,
            b"measurement photo bytes",
            Some("jpg"),
            AttachmentKind::MeasurementPhoto,
            Some("caliper.jpg"),
            "upload",
        )
        .unwrap();

    db.set_dimension_attachment(&dim.id, &photo.content_hash)
        .unwrap();

    let stored_hash: String = db
        .raw_conn()
        .query_row(
            "SELECT attachment_id FROM dimensions WHERE id = ?1",
            [&dim.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored_hash, photo.content_hash);

    // A bad dimension id or a bad hash is a typed error, not a silent no-op.
    assert!(matches!(
        db.set_dimension_attachment("no-such-dim", &photo.content_hash),
        Err(DbError::DimensionNotFound)
    ));
    assert!(matches!(
        db.set_dimension_attachment(&dim.id, "no-such-hash"),
        Err(DbError::AttachmentNotFound)
    ));
}
