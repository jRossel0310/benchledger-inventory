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
    // A fresh database migrates to the latest supported version, not
    // specifically v2; this test only cares that v2's tables are present
    // (v3 is additive and does not remove them).
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
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
    // Task 4 wires `seed::ensure_builtins` into `open_and_migrate`, which adds
    // 67 more built-in categories alongside Miscellaneous, so this can no
    // longer assume the table has exactly one row; scope to the row inserted
    // by migration 0002 (all-zero id) specifically.
    let (id, name, built_in): (String, String, i64) = db
        .raw_conn()
        .query_row(
            "SELECT id, name, built_in FROM categories WHERE id = '00000000000000000000000000'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(id, inventory_db::MISC_CATEGORY_ID);
    assert_eq!(name, "Miscellaneous");
    assert_eq!(built_in, 1);
}

#[test]
fn v1_database_upgrades_to_latest_with_backup() {
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
    // Opening a v1 database now replays migrations 2 and 3, landing on the
    // latest supported version rather than stopping at v2.
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
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

#[test]
fn v3_schema_adds_attribute_and_dimension_tables() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    // A fresh database migrates to the latest supported version, not
    // specifically v3; this test only cares that v3's tables are present
    // (v4 is additive and does not remove them).
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    for t in [
        "attribute_defs",
        "category_attributes",
        "attribute_choices",
        "part_attribute_values",
        "dimensions",
    ] {
        let n: i64 = db
            .raw_conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "missing table {t}");
    }
}

#[test]
fn v4_schema_adds_search_and_matching_tables() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    for t in ["search_text", "part_aliases", "equivalence_decisions"] {
        let n: i64 = db
            .raw_conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "missing table {t}");
    }
    // FTS5 virtual tables register in sqlite_master with a CREATE VIRTUAL
    // TABLE statement; presence by name is what matters here.
    let fts: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'parts_fts'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fts, 1, "missing virtual table parts_fts");
}

#[test]
fn v3_database_upgrades_to_latest_with_backup() {
    let (_g, db_path, backups) = temp_dirs();
    // Build a genuine v3 database by replaying migrations 1-3 manually, then
    // reopen: open_and_migrate replays every remaining migration and lands on
    // the latest supported version (the 4 -> 5 step is exercised in isolation
    // by `v4_database_upgrades_to_v5`).
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;",
        )
        .unwrap();
        for (v, name, sql) in inventory_db::MIGRATIONS.iter().take(3) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![v, name],
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
    }
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(
        std::fs::read_dir(&backups).unwrap().count(),
        1,
        "expected pre-migration backup"
    );
}

#[test]
fn v3_database_upgrade_backfills_search_text_for_existing_parts() {
    let (_g, db_path, backups) = temp_dirs();
    // Build a genuine v3 database (mirrors v3_database_upgrades_to_latest_with_backup)
    // and insert a part directly via raw SQL against the v2/v3 `parts` schema,
    // so it predates the search_text/FTS machinery migration 0004 adds and
    // never goes through `create_part`'s `refresh_search_text` call.
    let part_id = inventory_core::ids::PartId::new();
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;",
        )
        .unwrap();
        for (v, name, sql) in inventory_db::MIGRATIONS.iter().take(3) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![v, name],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO parts (id, display_name, category_id, quantity_unit)
             VALUES (?1, 'Backfill probe widget', '00000000000000000000000000', 'each')",
            [part_id.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part_stock (part_id) VALUES (?1)",
            [part_id.as_str()],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
    }
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);

    let hits = db.search("Backfill probe widget").unwrap();
    assert!(
        hits.iter().any(|h| h.part_id.as_str() == part_id.as_str()),
        "pre-existing v3 part must become searchable after the upgrade backfills search_text, got {hits:?}"
    );
}

#[test]
fn v5_schema_adds_attachment_tables() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    for t in ["attachments", "part_attachments"] {
        let n: i64 = db
            .raw_conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "missing table {t}");
    }
}

#[test]
fn v4_database_upgrades_to_latest_with_backup() {
    let (_g, db_path, backups) = temp_dirs();
    // Build a genuine v4 database by replaying migrations 1-4 manually, then
    // reopen: open_and_migrate replays every remaining migration and lands on
    // the latest supported version (the 5 -> 6 step is exercised in isolation
    // by `v5_database_upgrades_to_v6`).
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;",
        )
        .unwrap();
        for (v, name, sql) in inventory_db::MIGRATIONS.iter().take(4) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![v, name],
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 4).unwrap();
    }
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(
        std::fs::read_dir(&backups).unwrap().count(),
        1,
        "expected pre-migration backup"
    );
}

#[test]
fn v6_schema_adds_project_fields_and_bom_tables() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    for t in ["bom_items", "bom_substitutes"] {
        let n: i64 = db
            .raw_conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "missing table {t}");
    }
    let cols: Vec<String> = {
        let conn = db.raw_conn();
        let mut stmt = conn.prepare("PRAGMA table_info(projects)").unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    for required in [
        "status",
        "description",
        "build_quantity",
        "repo_link",
        "notes",
        "completed_at",
    ] {
        assert!(
            cols.iter().any(|c| c == required),
            "projects missing column {required}"
        );
    }
}

#[test]
fn v5_database_upgrades_to_v6() {
    let (_g, db_path, backups) = temp_dirs();
    // Build a genuine v5 database by replaying migrations 1-5 manually, then
    // reopen: open_and_migrate replays every remaining migration and lands on
    // the latest supported version rather than stopping at v6 (the 6 -> 7
    // step is exercised in isolation by `v6_database_upgrades_to_v7`).
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;",
        )
        .unwrap();
        for (v, name, sql) in inventory_db::MIGRATIONS.iter().take(5) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![v, name],
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
    }
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    let n: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = 'bom_items'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "v5 -> v6 upgrade must add the bom_items table");
    assert_eq!(
        std::fs::read_dir(&backups).unwrap().count(),
        1,
        "expected pre-migration backup"
    );
}

#[test]
fn v2_database_upgrades_to_latest_with_backup() {
    let (_g, db_path, backups) = temp_dirs();
    // Build a genuine v2 database by replaying migrations 1-2 manually.
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;",
        )
        .unwrap();
        for (v, name, sql) in inventory_db::MIGRATIONS.iter().take(2) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![v, name],
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 2).unwrap();
    }
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    // Opening a v2 database now replays migrations 3 and 4, landing on the
    // latest supported version rather than stopping at v3.
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1);
}

#[test]
fn v7_schema_adds_import_tables() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    for t in [
        "imports",
        "import_files",
        "import_lines",
        "price_history",
        "equivalence_families",
        "equivalence_family_members",
        "project_checkouts",
    ] {
        let n: i64 = db
            .raw_conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "missing table {t}");
    }
}

#[test]
fn v6_database_upgrades_to_v7() {
    let (_g, db_path, backups) = temp_dirs();
    // Build a genuine v6 database by replaying migrations 1-6 manually, then
    // reopen: open_and_migrate replays every remaining migration and lands on
    // the latest supported version rather than stopping at v7 (the 7 -> 8
    // step is exercised in isolation by `v7_database_upgrades_to_v8`).
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;",
        )
        .unwrap();
        for (v, name, sql) in inventory_db::MIGRATIONS.iter().take(6) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![v, name],
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 6).unwrap();
    }
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    let n: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = 'imports'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "v6 -> v7 upgrade must add the imports table");
    assert_eq!(
        std::fs::read_dir(&backups).unwrap().count(),
        1,
        "expected pre-migration backup"
    );
}

#[test]
fn v7_database_upgrades_to_v8() {
    let (_g, db_path, backups) = temp_dirs();
    // Build a genuine v7 database by replaying migrations 1-7 manually, then
    // reopen to exercise the 7 -> 8 upgrade step (and its safety backup).
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;",
        )
        .unwrap();
        for (v, name, sql) in inventory_db::MIGRATIONS.iter().take(7) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![v, name],
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 7).unwrap();
    }
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 8);
    let cols: Vec<String> = {
        let conn = db.raw_conn();
        let mut stmt = conn.prepare("PRAGMA table_info(imports)").unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert!(
        cols.iter().any(|c| c == "commit_group_id"),
        "v7 -> v8 upgrade must add imports.commit_group_id"
    );
    assert_eq!(
        std::fs::read_dir(&backups).unwrap().count(),
        1,
        "expected pre-migration backup"
    );
}
