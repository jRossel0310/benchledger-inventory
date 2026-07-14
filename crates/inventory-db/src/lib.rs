//! SQLite integration and versioned migrations.
mod database;
pub use database::{Database, DbError, SUPPORTED_SCHEMA_VERSION};
