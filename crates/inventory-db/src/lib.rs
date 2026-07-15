//! SQLite integration and versioned migrations.
mod database;
pub use database::{Database, DbError, MIGRATIONS, SUPPORTED_SCHEMA_VERSION};
