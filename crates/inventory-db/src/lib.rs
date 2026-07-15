//! SQLite integration and versioned migrations.
mod database;
pub mod parts;
pub use database::{Database, DbError, MIGRATIONS, MISC_CATEGORY_ID, SUPPORTED_SCHEMA_VERSION};
