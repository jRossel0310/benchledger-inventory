//! SQLite integration and versioned migrations.
pub mod attributes;
pub mod categories;
pub mod dashboard;
mod database;
pub mod dev_seed;
pub mod dimensions;
pub mod identity;
pub mod ledger;
pub mod matching;
pub mod parts;
pub mod search;
pub mod seed;
pub mod settings;
pub mod validate;
pub use database::{Database, DbError, MIGRATIONS, MISC_CATEGORY_ID, SUPPORTED_SCHEMA_VERSION};
