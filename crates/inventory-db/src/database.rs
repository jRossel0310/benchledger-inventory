use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

/// Highest schema version this build of the application understands.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Ordered embedded migrations: (version, name, sql).
const MIGRATIONS: &[(u32, &str, &str)] = &[(
    1,
    "create_settings",
    include_str!("../migrations/0001_create_settings.sql"),
)];

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
        let conn = Connection::open(db_path)?;
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
                apply_migration(&conn, *version, name, sql)?;
            }
        }

        Ok(Database { conn })
    }

    pub fn schema_version(&self) -> Result<u32, DbError> {
        schema_version_of(&self.conn)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

fn schema_version_of(conn: &Connection) -> Result<u32, DbError> {
    let v: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    Ok(v)
}

fn apply_migration(conn: &Connection, version: u32, name: &str, sql: &str) -> Result<(), DbError> {
    let wrap = |source| DbError::Migration { version, name: name.to_string(), source };
    conn.execute_batch("BEGIN").map_err(wrap)?;
    let result = (|| {
        conn.execute_batch(sql)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, name, applied_at)
             VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![version, name],
        )?;
        conn.pragma_update(None, "user_version", version)?;
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(wrap),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(wrap(e))
        }
    }
}

fn write_safety_backup(conn: &Connection, backup_dir: &Path, from_version: u32) -> Result<(), DbError> {
    std::fs::create_dir_all(backup_dir)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let dest_path = backup_dir.join(format!("pre-migration-v{from_version}-{stamp}.sqlite"));
    let mut dest = Connection::open(&dest_path)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut dest)?;
    backup.run_to_completion(64, std::time::Duration::from_millis(50), None)?;
    Ok(())
}
