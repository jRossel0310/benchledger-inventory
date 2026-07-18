//! Supplier invoice parsers (DigiKey PDF/CSV/XLSX). Implemented in Phase 5.
//!
//! This crate is pure, testable parsing logic: the `ParsedInvoice` data
//! model (`model`) and the `InvoiceParser` trait + format detection
//! (`parser`) that every format-specific parser builds on. No `rusqlite`,
//! no network — persistence lives in `inventory-db`.

pub mod model;
pub mod parser;

pub use model::{LineKind, Money, ParsedInvoice, ParsedLine, ParsedOrderMeta, SourceFormat};
pub use parser::{detect_format, ImportError, InvoiceParser};
