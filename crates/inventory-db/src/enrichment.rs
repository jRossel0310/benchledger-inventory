//! Provenance-aware enrichment compare-and-apply (Phase 5c Task 5).
//!
//! `enrich_part_preview` runs the enrichment provider chain (DigiKey, then
//! the always-available offline description parser — see
//! `inventory_enrich::run_chain`) against a part and turns the resulting
//! candidates into a reviewable [`FieldDiff`] list, comparing each proposed
//! value against the part's CURRENT value and that field's recorded
//! `field_provenance` source. It writes nothing. `apply_enrichment` takes
//! the caller-approved subset back (as [`AppliedField`], carrying the
//! value+source the caller already saw in the diff, so apply never has to
//! re-fetch from a provider) and writes every approved field in ONE
//! transaction, upserting `field_provenance` for each and updating
//! `parts.metadata_complete`.
//!
//! # `requires_review` rule
//!
//! A `FieldDiff` is flagged `requires_review = true` when either:
//! - the field's CURRENT recorded provenance source is `'manual'` (a human
//!   typed it in deliberately — never silently replaced), or
//! - the candidate's own source is `inferred` (the low-confidence
//!   description parser) AND the field already has a current value.
//!
//! This is the plan's "simpler defensible rule" alternative to consulting
//! each category's identity-attribute definitions: instead of asking "is
//! this key identity-defining for this category", it asks "would this
//! overwrite something already there with a guess" — simpler, and strictly
//! more conservative, since it protects every already-populated field from
//! a low-confidence guess, not only the ones a category's schema happens to
//! flag as identity-defining, and it protects a value with no
//! `field_provenance` row at all (never explicitly re-confirmed, but also
//! never a guess) exactly as well as one explicitly recorded `manual`.
//!
//! # Field-key dispatch
//!
//! Both directions — `current_value_for_key` (preview) and the `match` in
//! `write_field` (apply) — dispatch on the same `field_key` scheme
//! `inventory_enrich` candidates use: `variant.datasheet_url` /
//! `variant.product_url` / `variant.lifecycle` / `variant.package` (the
//! part's PREFERRED manufacturer variant — the first row `list_variants`
//! returns: marked preferred if any variant is, else the lexicographically
//! first by mpn), `attr.<key>` (a category attribute, written through the
//! same `set_attribute_in_tx` validation path a user-typed value goes
//! through), `description` (`parts.description`), and `category`
//! (`parts.category_id`, resolved from the candidate's category NAME).
//!
//! # Source validation
//!
//! `apply_enrichment` validates every `AppliedField.source` against
//! migration 0009's `field_provenance.source` CHECK set (`VALID_SOURCES`,
//! below) BEFORE any field is written — an unrecognized source string comes
//! back as `DbError::InvalidEnrichmentSource`, a typed error, rather than
//! surfacing as a raw SQLite CHECK-constraint failure out of
//! `upsert_provenance`'s INSERT partway through the transaction.

use std::path::Path;

use rusqlite::{OptionalExtension, Transaction};

use inventory_core::ids::{CategoryId, PartId};
use inventory_enrich::{
    run_chain, DescriptionParser, DigiKeyClient, DigiKeyConfig, DigiKeyEnv, EnrichInput,
    EnrichSource, EnrichmentProvider, FieldCandidate,
};

use crate::attributes::set_attribute_in_tx;
use crate::parts::{PartRecord, VariantRecord};
use crate::{Database, DbError};

/// The non-secret `settings` key selecting the DigiKey API environment
/// (`"sandbox"` | `"production"`; `DigiKeyEnv::from_setting_str` defaults
/// anything else, including unset, to sandbox).
///
/// `pub`, not private: Task 6's `get_digikey_status`/`set_digikey_environment`
/// Tauri commands (`apps/desktop/src-tauri/src/commands.rs`) read/write this
/// exact same `settings` key, so the string literal lives in exactly one
/// place rather than being duplicated (and risking drift) at the command
/// layer.
pub const DIGIKEY_ENVIRONMENT_SETTING: &str = "digikey_environment";

/// Valid `field_provenance.source` values — mirrors migration 0009's CHECK
/// constraint (`field_provenance.source`) verbatim, and matches every
/// `EnrichSource::as_db_str()` output. `AppliedField.source` is a plain
/// `String` (not the typed `EnrichSource`) since it round-trips through the
/// Tauri IPC boundary (Task 6) as JSON — validating it here, before any row
/// is written, turns a bad value into a typed `DbError` instead of letting
/// it reach `upsert_provenance`'s INSERT and blow up mid-transaction on the
/// raw SQLite CHECK constraint.
const VALID_SOURCES: [&str; 7] = [
    "invoice",
    "digikey",
    "manufacturer",
    "datasheet",
    "inferred",
    "manual",
    "measured",
];

fn validate_source(source: &str) -> Result<(), DbError> {
    if VALID_SOURCES.contains(&source) {
        Ok(())
    } else {
        Err(DbError::InvalidEnrichmentSource(source.to_string()))
    }
}

/// One proposed field change: the candidate's value alongside the part's
/// current value and that field's current recorded provenance, so a caller
/// (a UI diff screen) can decide what to approve without a further query.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FieldDiff {
    pub key: String,
    pub current: Option<String>,
    pub proposed: String,
    /// `EnrichSource::as_db_str()` of the candidate that proposed this value.
    pub source: String,
    /// The field's current `field_provenance.source`, if a row exists yet.
    pub current_source: Option<String>,
    pub requires_review: bool,
}

/// The full preview for one part: one `FieldDiff` per candidate whose
/// proposed value differs from the current value (a candidate that already
/// matches is omitted — nothing to review), plus the chain's notes and
/// which providers ran, for a status line.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct EnrichmentDiff {
    pub part_id: PartId,
    pub diffs: Vec<FieldDiff>,
    pub notes: Vec<String>,
    pub provider_summary: Vec<String>,
}

/// One field the caller has approved for `apply_enrichment` — carries the
/// value+source straight from the `FieldDiff` the caller already reviewed
/// (`proposed`/`source`), so apply never has to re-run the provider chain or
/// re-derive a value the user already saw and approved.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AppliedField {
    pub key: String,
    pub value: String,
    pub source: String,
}

impl Database {
    /// Build an `EnrichInput` from `part_id`'s current state, run the
    /// enrichment provider chain (DigiKey — if configured, else it silently
    /// contributes nothing — then the always-available offline description
    /// parser), and diff the resulting candidates against current values.
    /// Writes nothing.
    ///
    /// `cache_dir` is `DataLayout.cache` (mirrors `store_import`'s
    /// `attachments_dir` parameter) — the DigiKey client's on-disk response
    /// cache lives under `<cache_dir>/digikey/`.
    pub fn enrich_part_preview(
        &mut self,
        part_id: &PartId,
        cache_dir: &Path,
    ) -> Result<EnrichmentDiff, DbError> {
        let part = self.get_part(part_id)?.ok_or(DbError::PartNotFound)?;
        let preferred = self.list_variants(part_id)?.into_iter().next();
        let supplier_sku = match &preferred {
            Some(v) => self
                .list_supplier_listings(&v.id)?
                .into_iter()
                .next()
                .map(|l| l.supplier_sku),
            None => None,
        };
        let category_name = self.category_name(&part.category_id)?;

        let input = EnrichInput {
            manufacturer: preferred
                .as_ref()
                .map(|v| v.manufacturer.clone())
                .filter(|s| !s.trim().is_empty()),
            mpn: preferred
                .as_ref()
                .map(|v| v.mpn.clone())
                .filter(|s| !s.trim().is_empty()),
            supplier_sku,
            description: Some(part.description.clone()).filter(|s| !s.trim().is_empty()),
            category_hint: category_name.clone(),
        };

        let environment = self
            .get_setting(DIGIKEY_ENVIRONMENT_SETTING)?
            .unwrap_or_default();
        let digikey = DigiKeyClient::new(DigiKeyConfig {
            environment: DigiKeyEnv::from_setting_str(&environment),
            cache_dir: cache_dir.to_path_buf(),
        });
        let description = DescriptionParser;
        // DigiKey outranks the description parser: an authoritative supplier
        // catalog value should win over a shorthand-parsed guess for the
        // same key (see `inventory_enrich::run_chain`'s priority-order doc).
        let providers: [&dyn EnrichmentProvider; 2] = [&digikey, &description];
        let chain = run_chain(&providers, &input);

        let diffs = self.build_diff(
            part_id,
            &part,
            preferred.as_ref(),
            &category_name,
            &chain.candidates,
        )?;

        Ok(EnrichmentDiff {
            part_id: part_id.clone(),
            diffs,
            notes: chain.notes,
            provider_summary: chain.providers_run,
        })
    }

    /// Compare `candidates` (already-run provider output) against the
    /// part's current state, producing one `FieldDiff` per candidate whose
    /// proposed value differs from the current value.
    ///
    /// `pub(crate)` seam: this module's own tests call it directly with
    /// hand-built candidate lists, so the diff/`requires_review` logic is
    /// exercised without ever running a real provider chain (no network, no
    /// credential store touched) — see the plan's "expose a `build_diff` and
    /// test it directly" guidance.
    pub(crate) fn build_diff(
        &self,
        part_id: &PartId,
        part: &PartRecord,
        preferred: Option<&VariantRecord>,
        category_name: &Option<String>,
        candidates: &[FieldCandidate],
    ) -> Result<Vec<FieldDiff>, DbError> {
        let attrs = self.get_attributes(part_id)?;
        let mut diffs = Vec::new();
        for c in candidates {
            let current = current_value_for_key(&c.key, part, preferred, category_name, &attrs);
            if current.as_deref() == Some(c.value.as_str()) {
                continue; // proposed already matches current — nothing to review
            }
            let current_source = self.field_provenance_source(part_id, &c.key)?;
            let requires_review = current_source.as_deref() == Some("manual")
                || (c.source == EnrichSource::Inferred && current.is_some());
            diffs.push(FieldDiff {
                key: c.key.clone(),
                current,
                proposed: c.value.clone(),
                source: c.source.as_db_str().to_string(),
                current_source,
                requires_review,
            });
        }
        Ok(diffs)
    }

    fn category_name(&self, category_id: &CategoryId) -> Result<Option<String>, DbError> {
        self.raw_conn()
            .query_row(
                "SELECT name FROM categories WHERE id = ?1",
                [category_id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    fn field_provenance_source(
        &self,
        part_id: &PartId,
        field_key: &str,
    ) -> Result<Option<String>, DbError> {
        self.raw_conn()
            .query_row(
                "SELECT source FROM field_provenance WHERE part_id = ?1 AND field_key = ?2",
                rusqlite::params![part_id.as_str(), field_key],
                |r| r.get(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    /// Apply the caller-approved `applied` fields in ONE transaction: each
    /// field is written to the right place, and `field_provenance` is
    /// upserted for it. A category candidate whose value doesn't resolve to
    /// a real category name, or a `variant.*` candidate when the part has no
    /// variant to write into, is skipped (logged via `tracing::warn!`, not
    /// erred) rather than aborting the whole apply — everything else is a
    /// hard `DbError` that aborts (and rolls back) the whole transaction:
    /// `rusqlite` rolls back an uncommitted `Transaction` when it is dropped
    /// without `commit()`, which is exactly what happens here when `?`
    /// returns out of this function before `tx.commit()` is reached.
    pub fn apply_enrichment(
        &mut self,
        part_id: &PartId,
        applied: &[AppliedField],
    ) -> Result<(), DbError> {
        let tx = self.conn_mut().transaction()?;
        apply_enrichment_in_tx(&tx, part_id, applied)?;
        tx.commit()?;
        self.refresh_search_text(part_id)?;
        Ok(())
    }
}

/// Current value for `key`, dispatching on the same field-key scheme
/// `inventory_enrich` candidates use. Returns `None` both for "field never
/// set" and for an unrecognized key — forward-compatible: an unknown key
/// just always looks like a diff (never panics, never errors).
fn current_value_for_key(
    key: &str,
    part: &PartRecord,
    preferred: Option<&VariantRecord>,
    category_name: &Option<String>,
    attrs: &[(String, String, Option<f64>)],
) -> Option<String> {
    match key {
        "variant.datasheet_url" => preferred.and_then(|v| v.datasheet_url.clone()),
        "variant.product_url" => preferred.and_then(|v| v.product_url.clone()),
        "variant.lifecycle" => preferred.and_then(|v| v.lifecycle.clone()),
        "variant.package" => preferred.and_then(|v| v.package.clone()),
        "description" => Some(part.description.clone()).filter(|s| !s.is_empty()),
        "category" => category_name.clone(),
        other => other
            .strip_prefix("attr.")
            .and_then(|attr_key| attrs.iter().find(|(k, _, _)| k == attr_key))
            .map(|(_, original_text, _)| original_text.clone()),
    }
}

fn apply_enrichment_in_tx(
    tx: &Transaction<'_>,
    part_id: &PartId,
    applied: &[AppliedField],
) -> Result<(), DbError> {
    // Validate every source up front, before any write — a bad source
    // string becomes a typed `DbError` here rather than a raw SQLite CHECK
    // violation surfacing from `upsert_provenance` mid-transaction after
    // some fields have already been written (the transaction would still
    // roll back either way, but failing fast means an invalid `AppliedField`
    // never causes even a single field write to run first).
    for field in applied {
        validate_source(&field.source)?;
    }

    let exists: i64 = tx.query_row(
        "SELECT COUNT(*) FROM parts WHERE id = ?1",
        [part_id.as_str()],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Err(DbError::PartNotFound);
    }
    let preferred_variant_id: Option<String> = tx
        .query_row(
            "SELECT id FROM manufacturer_variants WHERE part_id = ?1
             ORDER BY is_preferred DESC, mpn LIMIT 1",
            [part_id.as_str()],
            |r| r.get(0),
        )
        .optional()?;

    for field in applied {
        let written = write_field(tx, part_id, preferred_variant_id.as_deref(), field)?;
        if !written {
            continue; // skipped (bad category name / no variant to write into) — see doc comment
        }
        upsert_provenance(tx, part_id, &field.key, &field.source)?;
    }

    update_metadata_complete(tx, part_id, preferred_variant_id.as_deref())?;
    Ok(())
}

/// Write one approved field to its target row. Returns `false` for the two
/// documented skip cases (bad category name; no preferred variant to write
/// a `variant.*` field into) — those are logged, not errors, so one
/// unresolvable candidate can't abort the whole apply. Every other failure
/// (an invalid attribute value, an unknown attribute key) is a real
/// `DbError` that aborts the whole transaction.
fn write_field(
    tx: &Transaction<'_>,
    part_id: &PartId,
    preferred_variant_id: Option<&str>,
    field: &AppliedField,
) -> Result<bool, DbError> {
    match field.key.as_str() {
        "variant.datasheet_url"
        | "variant.product_url"
        | "variant.lifecycle"
        | "variant.package" => {
            let Some(vid) = preferred_variant_id else {
                tracing::warn!(
                    part_id = %part_id.as_str(),
                    key = %field.key,
                    "apply_enrichment: no preferred variant to write to; skipping"
                );
                return Ok(false);
            };
            let sql = match field.key.as_str() {
                "variant.datasheet_url" => {
                    "UPDATE manufacturer_variants SET datasheet_url = ?1 WHERE id = ?2"
                }
                "variant.product_url" => {
                    "UPDATE manufacturer_variants SET product_url = ?1 WHERE id = ?2"
                }
                "variant.lifecycle" => {
                    "UPDATE manufacturer_variants SET lifecycle = ?1 WHERE id = ?2"
                }
                _ => "UPDATE manufacturer_variants SET package = ?1 WHERE id = ?2",
            };
            tx.execute(sql, rusqlite::params![field.value, vid])?;
            Ok(true)
        }
        "description" => {
            tx.execute(
                "UPDATE parts SET description = ?1, modified_at = datetime('now') WHERE id = ?2",
                rusqlite::params![field.value, part_id.as_str()],
            )?;
            Ok(true)
        }
        "category" => {
            let resolved: Option<String> = tx
                .query_row(
                    "SELECT id FROM categories WHERE name = ?1",
                    [field.value.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(category_id) = resolved else {
                tracing::warn!(
                    part_id = %part_id.as_str(),
                    category = %field.value,
                    "apply_enrichment: category name does not resolve to a known category; skipping"
                );
                return Ok(false);
            };
            tx.execute(
                "UPDATE parts SET category_id = ?1, modified_at = datetime('now') WHERE id = ?2",
                rusqlite::params![category_id, part_id.as_str()],
            )?;
            Ok(true)
        }
        key if key.starts_with("attr.") => {
            let attr_key = &key["attr.".len()..];
            set_attribute_in_tx(tx, part_id, attr_key, &field.value)?;
            Ok(true)
        }
        other => {
            tracing::warn!(
                part_id = %part_id.as_str(),
                key = %other,
                "apply_enrichment: unrecognized field key; skipping"
            );
            Ok(false)
        }
    }
}

/// UPSERT `field_provenance(part_id, field_key)`, matching migration 0009's
/// `UNIQUE(part_id, field_key)` — a second and later apply of the same key
/// updates the existing row rather than creating a duplicate.
///
/// `confidence` is always written `NULL`: `AppliedField` deliberately
/// carries no confidence value (the caller hands back only what it
/// approved, not the provider's numeric confidence score) — a user-approved
/// field's trustworthiness is captured by `source` alone from this point
/// on, not a stale provider confidence number.
fn upsert_provenance(
    tx: &Transaction<'_>,
    part_id: &PartId,
    field_key: &str,
    source: &str,
) -> Result<(), DbError> {
    let id = inventory_core::id::new_id();
    tx.execute(
        "INSERT INTO field_provenance (id, part_id, field_key, source, confidence, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, datetime('now'))
         ON CONFLICT(part_id, field_key) DO UPDATE SET
           source = excluded.source,
           confidence = excluded.confidence,
           updated_at = excluded.updated_at",
        rusqlite::params![id, part_id.as_str(), field_key, source],
    )?;
    Ok(())
}

/// Set `parts.metadata_complete = 1` once the part's preferred variant has a
/// non-blank manufacturer, mpn, AND datasheet_url — a simple, documented
/// "has real identity plus a datasheet" completeness bar. Deliberately
/// monotonic: this only ever sets the flag to `1`, never back to `0` — an
/// apply that doesn't (yet) meet the bar leaves a previously-set flag alone
/// rather than un-completing a part on an unrelated field update.
fn update_metadata_complete(
    tx: &Transaction<'_>,
    part_id: &PartId,
    preferred_variant_id: Option<&str>,
) -> Result<(), DbError> {
    let Some(vid) = preferred_variant_id else {
        return Ok(());
    };
    let (manufacturer, mpn, datasheet_url): (String, String, Option<String>) = tx.query_row(
        "SELECT manufacturer, mpn, datasheet_url FROM manufacturer_variants WHERE id = ?1",
        [vid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let complete = !manufacturer.trim().is_empty()
        && !mpn.trim().is_empty()
        && datasheet_url.is_some_and(|s| !s.trim().is_empty());
    if complete {
        tx.execute(
            "UPDATE parts SET metadata_complete = 1 WHERE id = ?1",
            [part_id.as_str()],
        )?;
    }
    Ok(())
}

// `build_diff` is `pub(crate)`, so — like `parts::create_part_in_tx` and
// friends — an external integration test crate cannot see it. These
// in-source tests exercise it directly with hand-built `FieldCandidate`
// lists, proving the diff/`requires_review` logic without ever running a
// real provider chain (no network, no credential store touched, no
// dependency on whatever DigiKey credentials may or may not be stored on
// the machine running the tests).
#[cfg(test)]
mod tests {
    use inventory_core::ids::CategoryId;
    use inventory_core::quantity::QuantityUnit;

    use crate::parts::{PartDraft, VariantDraft};

    use super::*;

    fn open() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("b");
        std::fs::create_dir_all(&backups).unwrap();
        let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
        (dir, db)
    }

    fn category_id(db: &Database, name: &str) -> CategoryId {
        let raw: String = db
            .raw_conn()
            .query_row("SELECT id FROM categories WHERE name = ?1", [name], |r| {
                r.get(0)
            })
            .unwrap();
        CategoryId::from_string(raw).unwrap()
    }

    fn candidate(key: &str, value: &str, source: EnrichSource) -> FieldCandidate {
        FieldCandidate {
            key: key.to_string(),
            value: value.to_string(),
            source,
            confidence: 0.9,
        }
    }

    /// Seeds a resistor part with:
    /// - a preferred variant with a BLANK `datasheet_url`;
    /// - a manually-set `resistance` attribute (`10k`), with a
    ///   `field_provenance` row directly inserted recording `manual` — this
    ///   simulates a human having typed it in and confirmed it (not itself
    ///   `apply_enrichment`'s job to have produced, since Task 5 is the
    ///   first thing that writes provenance at all);
    /// - a manually-set `tolerance` attribute (`5%`) with NO provenance row
    ///   at all — an "existing but never explicitly confirmed" value, which
    ///   must be protected from a silent inferred overwrite exactly like the
    ///   `manual` one is (see the module doc's `requires_review` rule).
    fn seed_part(db: &mut Database) -> PartId {
        let draft = PartDraft {
            display_name: "10k resistor".into(),
            category_id: category_id(db, "Resistor"),
            description: String::new(),
            bin_label: None,
            usage_behavior: "usually_consumed".into(),
            quantity_unit: QuantityUnit::Each,
            low_stock_threshold: None,
            public_notes: String::new(),
            private_notes: String::new(),
        };
        let part = db.create_part(&draft).unwrap();
        let variant = db
            .add_variant(
                &part.id,
                &VariantDraft {
                    manufacturer: "Yageo".into(),
                    mpn: "CFR-25JB-52-10K".into(),
                    description: String::new(),
                    package: None,
                    datasheet_url: None,
                    product_url: None,
                    lifecycle: None,
                    notes: String::new(),
                },
            )
            .unwrap();
        db.set_preferred_variant(&part.id, &variant.id).unwrap();

        db.set_attribute(&part.id, "resistance", "10k").unwrap();
        db.raw_conn()
            .execute(
                "INSERT INTO field_provenance (id, part_id, field_key, source, confidence)
                 VALUES (?1, ?2, 'attr.resistance', 'manual', NULL)",
                rusqlite::params![inventory_core::id::new_id(), part.id.as_str()],
            )
            .unwrap();

        db.set_attribute(&part.id, "tolerance", "5%").unwrap();

        part.id
    }

    #[test]
    fn build_diff_flags_manual_and_inferred_over_existing_but_not_a_fresh_datasheet() {
        let (_g, mut db) = open();
        let part_id = seed_part(&mut db);
        let part = db.get_part(&part_id).unwrap().unwrap();
        let variants = db.list_variants(&part_id).unwrap();
        let preferred = variants.first();
        let category_name = Some("Resistor".to_string());

        let candidates = vec![
            // New field, nothing there yet: a normal, non-review apply.
            candidate(
                "variant.datasheet_url",
                "https://example.com/ds.pdf",
                EnrichSource::Digikey,
            ),
            // Overwrites a `manual`-sourced attribute: requires review even
            // though the candidate's own source (digikey) is trustworthy.
            candidate("attr.resistance", "10.2k", EnrichSource::Digikey),
            // Overwrites an existing value with an `inferred` (low
            // confidence) guess: requires review even with no recorded
            // provenance row to consult.
            candidate("attr.tolerance", "1%", EnrichSource::Inferred),
        ];

        let diffs = db
            .build_diff(&part_id, &part, preferred, &category_name, &candidates)
            .unwrap();
        assert_eq!(diffs.len(), 3);

        let datasheet = diffs
            .iter()
            .find(|d| d.key == "variant.datasheet_url")
            .unwrap();
        assert_eq!(datasheet.current, None);
        assert_eq!(datasheet.proposed, "https://example.com/ds.pdf");
        assert_eq!(datasheet.current_source, None);
        assert!(!datasheet.requires_review);

        let resistance = diffs.iter().find(|d| d.key == "attr.resistance").unwrap();
        assert_eq!(resistance.current.as_deref(), Some("10k"));
        assert_eq!(resistance.current_source.as_deref(), Some("manual"));
        assert!(resistance.requires_review);

        let tolerance = diffs.iter().find(|d| d.key == "attr.tolerance").unwrap();
        assert_eq!(tolerance.current.as_deref(), Some("5%"));
        assert_eq!(tolerance.current_source, None);
        assert!(
            tolerance.requires_review,
            "an inferred guess must never silently replace an existing value, \
             even with no recorded provenance row"
        );
    }

    #[test]
    fn build_diff_omits_a_candidate_that_already_matches_the_current_value() {
        let (_g, mut db) = open();
        let part_id = seed_part(&mut db);
        let part = db.get_part(&part_id).unwrap().unwrap();
        let variants = db.list_variants(&part_id).unwrap();
        let preferred = variants.first();
        let category_name = Some("Resistor".to_string());

        let candidates = vec![candidate("attr.resistance", "10k", EnrichSource::Digikey)];
        let diffs = db
            .build_diff(&part_id, &part, preferred, &category_name, &candidates)
            .unwrap();
        assert!(
            diffs.is_empty(),
            "a candidate equal to the current value is not a diff: {diffs:?}"
        );
    }

    #[test]
    fn build_diff_treats_an_unknown_key_as_always_different_without_panicking() {
        let (_g, mut db) = open();
        let part_id = seed_part(&mut db);
        let part = db.get_part(&part_id).unwrap().unwrap();
        let category_name = Some("Resistor".to_string());

        let candidates = vec![candidate(
            "variant.manufacturer",
            "Yageo",
            EnrichSource::Digikey,
        )];
        let diffs = db
            .build_diff(&part_id, &part, None, &category_name, &candidates)
            .unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].current, None);
    }
}
