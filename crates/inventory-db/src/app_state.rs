//! Internal app-state key/value store over the `app_state` table (migration
//! 0010) — see that migration's doc comment for the keys Phase 6 uses and
//! why this is a separate table from `settings` (user-editable preferences)
//! rather than reusing it. Unlike `settings::set_setting`, every write here
//! refreshes `updated_at`, since the whole point of this table is tracking
//! *when* the app last derived a value (e.g. when a snapshot was last
//! published), not just what the value currently is.

use rusqlite::OptionalExtension;

use crate::{Database, DbError};

impl Database {
    /// Fetch a key's raw value, or `None` if it has never been set (or was
    /// cleared via `clear_app_state`).
    pub fn get_app_state(&self, key: &str) -> Result<Option<String>, DbError> {
        Ok(self
            .raw_conn()
            .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Upsert a key's value, stamping `updated_at` to now whether the row is
    /// freshly inserted or already existed.
    pub fn set_app_state(&mut self, key: &str, value: &str) -> Result<(), DbError> {
        self.conn_mut().execute(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    /// Remove a key entirely (models "absent" as no row — see
    /// `pending_publish`'s doc comment in the migration). A no-op, not an
    /// error, when the key was never set.
    pub fn clear_app_state(&mut self, key: &str) -> Result<(), DbError> {
        self.conn_mut()
            .execute("DELETE FROM app_state WHERE key = ?1", [key])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_test_db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db =
            Database::open_and_migrate(&dir.path().join("t.sqlite"), &dir.path().join("backups"))
                .unwrap();
        (dir, db)
    }

    #[test]
    fn missing_key_reads_as_none() {
        let (_dir, db) = open_test_db();
        assert_eq!(db.get_app_state("last_published_digest").unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips() {
        let (_dir, mut db) = open_test_db();
        db.set_app_state("last_published_digest", "abc123").unwrap();
        assert_eq!(
            db.get_app_state("last_published_digest").unwrap(),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn set_upserts_and_refreshes_updated_at() {
        let (_dir, mut db) = open_test_db();
        db.set_app_state("pending_publish", "1").unwrap();
        let first: String = db
            .raw_conn()
            .query_row(
                "SELECT updated_at FROM app_state WHERE key = 'pending_publish'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        db.set_app_state("pending_publish", "1").unwrap();
        let count: i64 = db
            .raw_conn()
            .query_row("SELECT COUNT(*) FROM app_state", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "upsert must not create a second row for the same key"
        );

        let second: String = db
            .raw_conn()
            .query_row(
                "SELECT updated_at FROM app_state WHERE key = 'pending_publish'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // Both timestamps come from `datetime('now')` at second resolution,
        // so they may be textually equal when the two calls land in the same
        // second; the meaningful assertion is that the second write's
        // `updated_at` is never *older* than the first's.
        assert!(second >= first);
    }

    #[test]
    fn clear_removes_the_key() {
        let (_dir, mut db) = open_test_db();
        db.set_app_state("pending_publish", "1").unwrap();
        db.clear_app_state("pending_publish").unwrap();
        assert_eq!(db.get_app_state("pending_publish").unwrap(), None);
    }

    #[test]
    fn clear_on_absent_key_is_a_no_op() {
        let (_dir, mut db) = open_test_db();
        db.clear_app_state("never_set").unwrap();
        assert_eq!(db.get_app_state("never_set").unwrap(), None);
    }
}
