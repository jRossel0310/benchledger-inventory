//! Pure/data-only part enrichment: the `EnrichmentProvider` trait, the
//! shared candidate/result model, and the always-available offline
//! description parser (spec §11, Phase 5c Task 3).
//!
//! Everything in this crate takes an `EnrichInput` and returns structured
//! `Enrichment` candidates — no `rusqlite`, no network. The DigiKey network
//! client (Task 4) is a provider that still only *produces* `Enrichment`
//! values; writing them to the database via the provenance-aware
//! compare-and-apply lives in `inventory-db` (Task 5).

pub mod description;
pub mod model;
pub mod provider;

pub use description::DescriptionParser;
pub use model::{EnrichInput, EnrichSource, Enrichment, FieldCandidate, ImageRef};
pub use provider::{run_chain, ChainResult, EnrichError, EnrichmentProvider};
