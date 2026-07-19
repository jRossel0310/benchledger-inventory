//! Enrichment domain model: what a provider produces, independent of any
//! particular provider implementation or of the database.

/// Where an enriched field's value came from. Mirrors migration 0009's
/// `field_provenance.source` CHECK set exactly — `as_db_str()` is the only
/// place that string mapping is allowed to live, so a variant added here
/// forces one deliberate edit instead of scattered string literals drifting
/// out of sync with the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrichSource {
    Invoice,
    Digikey,
    Manufacturer,
    Datasheet,
    Inferred,
    Manual,
    Measured,
}

impl EnrichSource {
    /// The exact string stored in `field_provenance.source` — must match
    /// migration 0009's CHECK constraint set verbatim:
    /// `'invoice' | 'digikey' | 'manufacturer' | 'datasheet' | 'inferred' |
    /// 'manual' | 'measured'`.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            EnrichSource::Invoice => "invoice",
            EnrichSource::Digikey => "digikey",
            EnrichSource::Manufacturer => "manufacturer",
            EnrichSource::Datasheet => "datasheet",
            EnrichSource::Inferred => "inferred",
            EnrichSource::Manual => "manual",
            EnrichSource::Measured => "measured",
        }
    }
}

/// One proposed field value from a provider.
///
/// `key` is a stable, provider-agnostic identifier shared with
/// `field_provenance.field_key` (migration 0009) and with the compare-and-
/// apply layer (Task 5): `category`, `description`, `variant.package`,
/// `variant.datasheet_url`, `variant.product_url`, `variant.lifecycle`, or
/// `attr.<attribute key>` (e.g. `attr.resistance`, using the same attribute
/// keys `Database::set_attribute` accepts).
///
/// `value` is text `Database::set_attribute` (or the equivalent variant-field
/// setter) can accept as-is — for a `number_unit` attribute that means the
/// original or lightly-normalized token (`"10k"`), not a pre-computed float;
/// the DB layer re-parses it through the same unit engine.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FieldCandidate {
    pub key: String,
    pub value: String,
    pub source: EnrichSource,
    pub confidence: f32,
}

/// A discovered image (e.g. a product photo) a provider found for a part.
/// Downloading it into an attachment is Task 5's concern, not this crate's.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImageRef {
    pub url: String,
}

/// The full result one provider produced for one `EnrichInput`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Enrichment {
    pub provider: String,
    pub candidates: Vec<FieldCandidate>,
    pub images: Vec<ImageRef>,
    pub notes: Vec<String>,
}

/// What's known about a part going into the enrichment chain — assembled by
/// the caller (Task 5) from the part's preferred variant, description, and
/// category. Every field is optional: a provider that needs one it wasn't
/// given returns `Ok(None)` rather than guessing.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnrichInput {
    pub manufacturer: Option<String>,
    pub mpn: Option<String>,
    pub supplier_sku: Option<String>,
    pub description: Option<String>,
    pub category_hint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_db_str_matches_the_0009_migration_check_set_exactly() {
        // Keep this list in lockstep with
        // crates/inventory-db/migrations/0009_provenance.sql's
        // `field_provenance.source` CHECK constraint.
        let expected = [
            (EnrichSource::Invoice, "invoice"),
            (EnrichSource::Digikey, "digikey"),
            (EnrichSource::Manufacturer, "manufacturer"),
            (EnrichSource::Datasheet, "datasheet"),
            (EnrichSource::Inferred, "inferred"),
            (EnrichSource::Manual, "manual"),
            (EnrichSource::Measured, "measured"),
        ];
        for (variant, expected_str) in expected {
            assert_eq!(variant.as_db_str(), expected_str);
        }
    }

    #[test]
    fn field_candidate_round_trips_through_json() {
        let candidate = FieldCandidate {
            key: "attr.resistance".to_string(),
            value: "10K".to_string(),
            source: EnrichSource::Inferred,
            confidence: 0.5,
        };
        let json = serde_json::to_string(&candidate).unwrap();
        let back: FieldCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(candidate, back);
        assert!(json.contains("\"inferred\""));
    }

    #[test]
    fn enrich_input_defaults_to_all_none() {
        let input = EnrichInput::default();
        assert_eq!(
            input,
            EnrichInput {
                manufacturer: None,
                mpn: None,
                supplier_sku: None,
                description: None,
                category_hint: None,
            }
        );
    }
}
