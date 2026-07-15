use inventory_db::{Database, DbError, SUPPORTED_SCHEMA_VERSION};

fn temp_dirs() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("inventory.sqlite");
    let backups = dir.path().join("local-backups");
    std::fs::create_dir_all(&backups).unwrap();
    (dir, db, backups)
}

#[test]
fn fresh_database_migrates_to_latest() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    // settings table exists and is usable
    db.raw_conn()
        .execute(
            "INSERT INTO settings (key, value) VALUES ('theme', 'dark')",
            [],
        )
        .unwrap();
    let v: String = db
        .raw_conn()
        .query_row("SELECT value FROM settings WHERE key = 'theme'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(v, "dark");
}

#[test]
fn reopening_is_idempotent() {
    let (_g, db_path, backups) = temp_dirs();
    drop(Database::open_and_migrate(&db_path, &backups).unwrap());
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    let count: i64 = db
        .raw_conn()
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, SUPPORTED_SCHEMA_VERSION as i64);
}

#[test]
fn required_pragmas_are_active() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    let journal: String = db
        .raw_conn()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal.to_lowercase(), "wal");
    let fk: i64 = db
        .raw_conn()
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1);
    let timeout: i64 = db
        .raw_conn()
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    assert_eq!(timeout, 5000);
}

#[test]
fn newer_schema_is_refused() {
    let (_g, db_path, backups) = temp_dirs();
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
    }
    let err = Database::open_and_migrate(&db_path, &backups).unwrap_err();
    match err {
        DbError::NewerSchema { found, supported } => {
            assert_eq!(found, 999);
            assert_eq!(supported, SUPPORTED_SCHEMA_VERSION);
        }
        other => panic!("expected NewerSchema, got {other:?}"),
    }
}

#[test]
fn existing_file_gets_pre_migration_backup() {
    let (_g, db_path, backups) = temp_dirs();
    {
        // simulate an existing (old, un-migrated) database file
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE legacy (x INTEGER)")
            .unwrap();
    }
    drop(Database::open_and_migrate(&db_path, &backups).unwrap());
    let backup_files: Vec<_> = std::fs::read_dir(&backups).unwrap().collect();
    assert_eq!(
        backup_files.len(),
        1,
        "expected exactly one pre-migration backup"
    );
}

#[test]
fn fresh_database_creates_no_backup() {
    let (_g, db_path, backups) = temp_dirs();
    drop(Database::open_and_migrate(&db_path, &backups).unwrap());
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 0);
}

#[test]
fn migrations_are_sorted_and_contiguous_from_one() {
    for (i, (version, _name, _sql)) in inventory_db::MIGRATIONS.iter().enumerate() {
        assert_eq!(
            *version,
            (i + 1) as u32,
            "migration versions must be contiguous starting at 1"
        );
    }
    assert_eq!(
        inventory_db::MIGRATIONS.last().map(|(v, _, _)| *v),
        Some(inventory_db::SUPPORTED_SCHEMA_VERSION),
        "SUPPORTED_SCHEMA_VERSION must equal the last migration version"
    );
}

#[test]
fn v2_schema_has_all_inventory_tables_strict() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), 2);
    let tables: Vec<String> = {
        let conn = db.raw_conn();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    for required in [
        "categories",
        "manufacturer_variants",
        "part_stock",
        "part_tags",
        "parts",
        "projects",
        "settings",
        "schema_migrations",
        "supplier_listings",
        "transaction_groups",
        "transactions",
    ] {
        assert!(
            tables.iter().any(|t| t == required),
            "missing table {required}"
        );
    }
}

#[test]
fn miscellaneous_category_is_seeded_deterministically() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    let (id, name, built_in): (String, String, i64) = db
        .raw_conn()
        .query_row("SELECT id, name, built_in FROM categories", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(id, inventory_db::MISC_CATEGORY_ID);
    assert_eq!(name, "Miscellaneous");
    assert_eq!(built_in, 1);
}

#[test]
fn v1_database_upgrades_to_v2_with_backup() {
    let (_g, db_path, backups) = temp_dirs();
    {
        // Build a v1 database the long way: open, then roll user_version back is not
        // possible — instead simulate by creating a fresh db and checking upgrade
        // path via MIGRATIONS slice bounds. Real prior-version fixtures start in 2b.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
             CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;
             INSERT INTO schema_migrations VALUES (1, 'create_settings', datetime('now'));
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), 2);
    assert_eq!(
        std::fs::read_dir(&backups).unwrap().count(),
        1,
        "expected pre-migration backup"
    );
    // settings from v1 must survive
    db.raw_conn()
        .execute(
            "INSERT INTO settings (key, value) VALUES ('probe', 'x')",
            [],
        )
        .unwrap();
}
