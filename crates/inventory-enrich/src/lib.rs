//! Pure/data-only part enrichment: the `EnrichmentProvider` trait, the
//! shared candidate/result model, the always-available offline description
//! parser (spec §11, Phase 5c Task 3), and the DigiKey Product Information
//! V4 API client (Task 4).
//!
//! Everything in this crate takes an `EnrichInput` and returns structured
//! `Enrichment` candidates. The DigiKey network client (`digikey` module) is
//! the one place in this crate that does network I/O (via
//! `reqwest::blocking`) and touches disk (an on-disk response cache under
//! `DataLayout.cache` — never the OAuth token, which lives only in memory);
//! it still only *produces* `Enrichment` values — writing them to the
//! database via the provenance-aware compare-and-apply lives in
//! `inventory-db` (Task 5).

pub mod description;
pub mod digikey;
pub mod model;
pub mod provider;

pub use description::DescriptionParser;
pub use digikey::{map_product_response, DigiKeyClient, DigiKeyConfig, DigiKeyEnv};
pub use model::{EnrichInput, EnrichSource, Enrichment, FieldCandidate, ImageRef};
pub use provider::{run_chain, ChainResult, EnrichError, EnrichmentProvider};
