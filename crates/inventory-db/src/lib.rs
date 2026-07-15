//! SQLite integration and versioned migrations.
pub mod attributes;
pub mod categories;
mod database;
pub mod dimensions;
pub mod ledger;
pub mod parts;
pub mod seed;
pub mod validate;
pub use database::{Database, DbError, MIGRATIONS, MISC_CATEGORY_ID, SUPPORTED_SCHEMA_VERSION};
