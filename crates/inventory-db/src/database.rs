use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use inventory_core::ids::PartId;

/// Highest schema version this build of the application understands.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 8;

/// Ordered embedded migrations: (version, name, sql).
/// Exposed for validation in tests; not part of the stable API.
pub const MIGRATIONS: &[(u32, &str, &str)] = &[
    (
        1,
        "create_settings",
        include_str!("../migrations/0001_create_settings.sql"),
    ),
    (
        2,
        "inventory_schema",
        include_str!("../migrations/0002_inventory_schema.sql"),
    ),
    (
        3,
        "attributes_dimensions",
        include_str!("../migrations/0003_attributes_dimensions.sql"),
    ),
    (
        4,
        "search_matching",
        include_str!("../migrations/0004_search_matching.sql"),
    ),
    (
        5,
        "attachments",
        include_str!("../migrations/0005_attachments.sql"),
    ),
    (
        6,
        "projects_boms",
        include_str!("../migrations/0006_projects_boms.sql"),
    ),
    (7, "imports", include_str!("../migrations/0007_imports.sql")),
    (
        8,
        "import_commit",
        include_str!("../migrations/0008_import_commit.sql"),
    ),
];

/// Deterministic id of the built-in Miscellaneous category (all-zero ULID).
pub const MISC_CATEGORY_ID: &str = "00000000000000000000000000";

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database schema v{found} is newer than this app supports (v{supported}); refusing write access")]
    NewerSchema { found: u32, supported: u32 },
    #[error("migration {version} ({name}) failed")]
    Migration {
        version: u32,
        name: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("part not found")]
    PartNotFound,
    #[error("database content is corrupt: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Domain(#[from] inventory_core::quantity::QuantityError),
    #[error("insufficient stock: {0}")]
    InsufficientStock(String),
    #[error("part is archived; only release, return, and reversals are allowed")]
    PartArchived,
    #[error(transparent)]
    Ledger(#[from] inventory_core::ledger::LedgerError),
    #[error("a transaction group must contain at least one operation")]
    EmptyGroup,
    #[error("transaction not found")]
    TransactionNotFound,
    #[error("transaction was already reversed")]
    AlreadyReversed,
    #[error("reversal transactions cannot be reversed; reverse the original instead")]
    CannotReverseReversal,
    #[error("group not found")]
    GroupNotFound,
    #[error("transaction belongs to a group; reverse the whole group instead")]
    TransactionInGroup,
    #[error("attribute '{0}' not found")]
    AttributeNotFound(String),
    #[error("invalid value for attribute '{key}': {reason}")]
    InvalidAttributeValue { key: String, reason: String },
    #[error("manufacturer variant not found")]
    VariantNotFound,
    #[error("project not found")]
    ProjectNotFound,
    #[error("BOM item not found")]
    BomItemNotFound,
    #[error("a BOM item for this part already exists on this project")]
    BomItemExists,
    #[error("quantity unit cannot change once the part has transactions")]
    UnitChangeBlocked,
    #[error("invalid dimension value: {0}")]
    InvalidDimension(String),
    #[error("invalid bin label: {0}")]
    InvalidBinLabel(String),
    #[error("dimension not found")]
    DimensionNotFound,
    #[error("category name is already in use")]
    CategoryNameTaken,
    #[error("attribute key is already in use")]
    AttributeKeyTaken,
    #[error("category not found")]
    CategoryNotFound,
    #[error("unknown search key '{0}'")]
    UnknownSearchKey(String),
    #[error("unsupported search key: {0}")]
    UnsupportedSearchKey(String),
    #[error("alias is already registered to another part")]
    AliasTaken,
    #[error("attachment not found")]
    AttachmentNotFound,
    #[error("invalid attachment: {0}")]
    InvalidAttachment(String),
    #[error("import not found")]
    ImportNotFound,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("import cannot be committed in its current status (must be 'parsed')")]
    ImportNotCommittable,
    #[error("import cannot be reversed in its current status (must be 'committed')")]
    ImportNotReversible,
    #[error("import line not found on this import")]
    ImportLineNotFound,
    #[error("import line is a '{line_kind}' line, not a part, and cannot create a part or receive inventory")]
    NonPartLineNotReceivable { line_kind: String },
}

#[derive(Debug)]
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open the database, apply pragmas, and run any pending migrations.
    /// If the file already existed and migrations are pending, a safety copy
    /// is written into `backup_dir` first (via SQLite's online backup API).
    pub fn open_and_migrate(db_path: &Path, backup_dir: &Path) -> Result<Self, DbError> {
        let existed = db_path.exists();
        let mut conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;

        let current = schema_version_of(&conn)?;
        if current > SUPPORTED_SCHEMA_VERSION {
            return Err(DbError::NewerSchema {
                found: current,
                supported: SUPPORTED_SCHEMA_VERSION,
            });
        }

        let pending: Vec<_> = MIGRATIONS.iter().filter(|(v, _, _)| *v > current).collect();
        if !pending.is_empty() {
            if existed {
                write_safety_backup(&conn, backup_dir, current)?;
            }
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version    INTEGER PRIMARY KEY,
                    name       TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                 ) STRICT",
            )?;
            for (version, name, sql) in pending {
                apply_migration(&mut conn, *version, name, sql)?;
            }
        }

        crate::seed::ensure_builtins(&mut conn)?;
        let mut db = Database { conn };
        db.backfill_search_text()?;
        Ok(db)
    }

    pub fn schema_version(&self) -> Result<u32, DbError> {
        schema_version_of(&self.conn)
    }

    /// Raw connection access. For integration tests and internal repository
    /// code only — application code must go through the typed APIs so every
    /// stock change flows through the ledger.
    #[doc(hidden)]
    pub fn raw_conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Test-only mutable connection access (seeding idempotency tests).
    #[doc(hidden)]
    pub fn conn_mut_for_tests(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Populate `search_text` for any part that doesn't already have a row.
    /// `search_text` is otherwise only kept in sync by `refresh_search_text`
    /// on mutation (see `search.rs`), so a part created before migration
    /// 0004 (or, in principle, any part whose search_text row is somehow
    /// missing) would stay invisible to `search` forever without this: it
    /// runs on every `open_and_migrate`, not just an upgrading one, so it's
    /// a plain, idempotent self-heal rather than a one-shot migration step.
    /// Steady state (every part already indexed) backfills zero parts.
    fn backfill_search_text(&mut self) -> Result<(), DbError> {
        let ids: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT p.id FROM parts p
                 WHERE NOT EXISTS (SELECT 1 FROM search_text s WHERE s.part_id = p.id)",
            )?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in ids {
            let part_id =
                PartId::from_string(id).map_err(|_| DbError::Corrupt("bad part id".into()))?;
            self.refresh_search_text(&part_id)?;
        }
        Ok(())
    }
}

fn schema_version_of(conn: &Connection) -> Result<u32, DbError> {
    let v: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    Ok(v)
}

fn apply_migration(
    conn: &mut Connection,
    version: u32,
    name: &str,
    sql: &str,
) -> Result<(), DbError> {
    let wrap = |source| DbError::Migration {
        version,
        name: name.to_string(),
        source,
    };
    let tx = conn.transaction().map_err(wrap)?;
    tx.execute_batch(sql).map_err(wrap)?;
    tx.execute(
        "INSERT INTO schema_migrations (version, name, applied_at)
         VALUES (?1, ?2, datetime('now'))",
        rusqlite::params![version, name],
    )
    .map_err(wrap)?;
    tx.pragma_update(None, "user_version", version)
        .map_err(wrap)?;
    tx.commit().map_err(wrap)
}

fn write_safety_backup(
    conn: &Connection,
    backup_dir: &Path,
    from_version: u32,
) -> Result<(), DbError> {
    std::fs::create_dir_all(backup_dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let dest_path = backup_dir.join(format!("pre-migration-v{from_version}-{stamp}.sqlite"));
    let mut dest = Connection::open(&dest_path)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut dest)?;
    backup.run_to_completion(64, std::time::Duration::from_millis(50), None)?;
    Ok(())
}
