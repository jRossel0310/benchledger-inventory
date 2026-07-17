//! Tiny key-value settings store over the `settings` table (migration
//! 0001). Used by the desktop UI for user preferences that don't belong in
//! the domain schema — e.g. the Inventory browser's saved search views
//! (Phase 3 Task 4), persisted as a JSON blob under one key. Deliberately
//! minimal: no typed schema, no per-key validation — callers own the
//! meaning and encoding of their own key/value pairs.

use rusqlite::OptionalExtension;

use crate::{Database, DbError};

impl Database {
    /// Fetch a setting's raw value, or `None` if `key` has never been set.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        Ok(self
            .raw_conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Upsert a setting's value — creates the key if it doesn't exist yet,
    /// overwrites it if it does.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), DbError> {
        self.conn_mut().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }
}
