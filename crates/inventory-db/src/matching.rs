//! Duplicate matching engine and decision memory (Phase 2c, spec §7/§10).
//!
//! `find_matches` scores an inbound `MatchCandidate` (e.g. an import row)
//! against parts already in the database; `suggest_duplicates` does the
//! same thing between two parts already in the database. Both walk the same
//! seven-level hierarchy (best rank wins per compared part):
//!
//! 1. `ExactSku`   — case-insensitive `supplier_listings.supplier_sku` hit.
//! 2. `ExactMpn`   — case-insensitive `manufacturer_variants.mpn` hit.
//! 3. `KnownAlias` — a remembered `part_aliases` row, or (suggestions only)
//!    a previously *approved* `equivalence_decisions` pair.
//! 4. `ExactIdentity` — every identity attribute matches exactly (Task 2's
//!    `identity_signature`/`signatures_equal`). PASSIVE categories only —
//!    see `is_passive_category`. ACTIVE/IC categories cap here at
//!    `ProbableEquivalent`; actives are never silently auto-combined.
//! 5. `ProbableEquivalent` — identity fields agree everywhere they're both
//!    present, and at most one field is missing on either side.
//! 6. `Similar` — at least two identity fields agree and at least one
//!    conflicts.
//! 7. `None` — none of the above; never surfaced in a result list.
//!
//! Levels 4-6 reuse `identity::identity_signature`/`signature_from_values`/
//! `signatures_equal` directly for the "everything matches" case (level 4).
//! When that's not clean (either side incomplete, or a genuine conflict),
//! matching falls back to a *partial* per-field map built from the same
//! `identity::identity_defs`/`identity::value_for_def` primitives Task 2
//! uses — never a re-parse or a separate notion of equality.

use std::collections::{BTreeMap, HashMap};

use rusqlite::OptionalExtension;

use inventory_core::ids::{CategoryId, ListingId, PartId, VariantId};

use crate::identity::{self, IdentityDef, IdentityValue};
use crate::{Database, DbError};

/// Seeded categories (matched by name — ids are per-install) whose parts can
/// be auto-combined on an exact identity match. Everything else, including
/// every custom (non-seed) category, is treated as ACTIVE: conservative, no
/// auto-merge, per spec §7 ("never silently merge actives").
const PASSIVE_CATEGORIES: &[&str] = &[
    "Resistor",
    "Capacitor",
    "Inductor",
    "Resistor network",
    "Ferrite bead",
    "Crystal",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchVerdict {
    ExactSku { listing: ListingId },
    ExactMpn { variant: VariantId },
    KnownAlias,
    ExactIdentity,
    ProbableEquivalent,
    Similar,
    None,
}

impl MatchVerdict {
    /// Hierarchy position, 1 (best) through 7 (no match) — sort key.
    fn rank(&self) -> u8 {
        match self {
            MatchVerdict::ExactSku { .. } => 1,
            MatchVerdict::ExactMpn { .. } => 2,
            MatchVerdict::KnownAlias => 3,
            MatchVerdict::ExactIdentity => 4,
            MatchVerdict::ProbableEquivalent => 5,
            MatchVerdict::Similar => 6,
            MatchVerdict::None => 7,
        }
    }

    fn kind_str(&self) -> &'static str {
        match self {
            MatchVerdict::ExactSku { .. } => "exact_sku",
            MatchVerdict::ExactMpn { .. } => "exact_mpn",
            MatchVerdict::KnownAlias => "known_alias",
            MatchVerdict::ExactIdentity => "exact_identity",
            MatchVerdict::ProbableEquivalent => "probable_equivalent",
            MatchVerdict::Similar => "similar",
            MatchVerdict::None => "none",
        }
    }
}

/// One scored match, ready to render: `verdict_kind` is the snake_case name
/// of the `MatchVerdict` that produced it, `rank` its hierarchy position
/// (1-7, ascending = better), and `explanation` a specific, human-readable
/// reason (never generic — see the per-level explanation builders below).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct MatchResult {
    pub part_id: PartId,
    pub display_name: String,
    pub verdict_kind: String,
    pub explanation: String,
    pub rank: u8,
}

/// An inbound row (e.g. an import line) to score against parts already on
/// file. `attributes` are raw `(key, value)` pairs in the same shape
/// `set_attribute` accepts — parsed under the candidate category's identity
/// defs, never the caller's responsibility to pre-normalize.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct MatchCandidate {
    pub supplier: Option<String>,
    pub supplier_sku: Option<String>,
    pub manufacturer: Option<String>,
    pub mpn: Option<String>,
    pub category_id: Option<CategoryId>,
    pub attributes: Vec<(String, String)>,
    pub package: Option<String>,
}

impl Database {
    /// Score `candidate` against every part on file, walking the hierarchy
    /// above. At most one `MatchResult` per part: when a part is reachable
    /// by more than one level (e.g. it matches both the SKU and, separately,
    /// the identity attributes), the better (lower-numbered) rank wins —
    /// SKU beats identity. Results are sorted by rank ascending, then part
    /// id for a stable tiebreak.
    pub fn find_matches(
        &mut self,
        candidate: &MatchCandidate,
    ) -> Result<Vec<MatchResult>, DbError> {
        let mut best: HashMap<String, (MatchVerdict, String)> = HashMap::new();

        if let Some(sku) = non_empty(candidate.supplier_sku.as_deref()) {
            let mut stmt = self.raw_conn().prepare(
                "SELECT mv.part_id, sl.id, sl.supplier_sku
                 FROM supplier_listings sl
                 JOIN manufacturer_variants mv ON mv.id = sl.variant_id
                 WHERE LOWER(sl.supplier_sku) = LOWER(?1)
                   AND (?2 IS NULL OR LOWER(sl.supplier) = LOWER(?2))",
            )?;
            let supplier = non_empty(candidate.supplier.as_deref());
            let mut rows = stmt.query(rusqlite::params![sku, supplier])?;
            while let Some(row) = rows.next()? {
                let part_id: String = row.get(0)?;
                let listing_id: String = row.get(1)?;
                let matched_sku: String = row.get(2)?;
                let listing = ListingId::from_string(listing_id)
                    .map_err(|_| DbError::Corrupt("bad listing id".into()))?;
                record_best(
                    &mut best,
                    part_id,
                    MatchVerdict::ExactSku { listing },
                    format!("Exact supplier SKU match: {matched_sku}"),
                );
            }
        }

        if let Some(mpn) = non_empty(candidate.mpn.as_deref()) {
            let mut stmt = self.raw_conn().prepare(
                "SELECT part_id, id, mpn FROM manufacturer_variants
                 WHERE LOWER(mpn) = LOWER(?1) AND (?2 IS NULL OR LOWER(manufacturer) = LOWER(?2))",
            )?;
            let manufacturer = non_empty(candidate.manufacturer.as_deref());
            let mut rows = stmt.query(rusqlite::params![mpn, manufacturer])?;
            while let Some(row) = rows.next()? {
                let part_id: String = row.get(0)?;
                let variant_id: String = row.get(1)?;
                let matched_mpn: String = row.get(2)?;
                let variant = VariantId::from_string(variant_id)
                    .map_err(|_| DbError::Corrupt("bad variant id".into()))?;
                record_best(
                    &mut best,
                    part_id,
                    MatchVerdict::ExactMpn { variant },
                    format!("Exact manufacturer part-number match: {matched_mpn}"),
                );
            }
        }

        if let Some(sku) = non_empty(candidate.supplier_sku.as_deref()) {
            let mut stmt = self.raw_conn().prepare(
                "SELECT part_id, alias_value FROM part_aliases
                 WHERE alias_kind = 'supplier_sku' AND LOWER(alias_value) = LOWER(?1)",
            )?;
            let mut rows = stmt.query([sku])?;
            while let Some(row) = rows.next()? {
                let part_id: String = row.get(0)?;
                let value: String = row.get(1)?;
                record_best(
                    &mut best,
                    part_id,
                    MatchVerdict::KnownAlias,
                    format!("Known supplier SKU alias: {value}"),
                );
            }
        }
        if let Some(mpn) = non_empty(candidate.mpn.as_deref()) {
            let mut stmt = self.raw_conn().prepare(
                "SELECT part_id, alias_value FROM part_aliases
                 WHERE alias_kind = 'mpn' AND LOWER(alias_value) = LOWER(?1)",
            )?;
            let mut rows = stmt.query([mpn])?;
            while let Some(row) = rows.next()? {
                let part_id: String = row.get(0)?;
                let value: String = row.get(1)?;
                record_best(
                    &mut best,
                    part_id,
                    MatchVerdict::KnownAlias,
                    format!("Known manufacturer part-number alias: {value}"),
                );
            }
        }

        if let Some(category_id) = &candidate.category_id {
            let defs = identity::identity_defs(self, category_id)?;
            if !defs.is_empty() {
                let is_passive = is_passive_category(self, category_id)?;
                let candidate_attributes = with_package_folded_in(candidate);
                let candidate_full =
                    identity::signature_from_values(self, category_id, &candidate_attributes)?;
                let candidate_partial = build_partial_from_values(&defs, &candidate_attributes);

                let mut stmt = self
                    .raw_conn()
                    .prepare("SELECT id FROM parts WHERE category_id = ?1 AND archived = 0")?;
                let mut rows = stmt.query([category_id.as_str()])?;
                let mut part_ids = Vec::new();
                while let Some(row) = rows.next()? {
                    part_ids.push(row.get::<_, String>(0)?);
                }
                drop(rows);
                drop(stmt);

                for part_id_str in part_ids {
                    let part_id = PartId::from_string(part_id_str)
                        .map_err(|_| DbError::Corrupt("bad part id".into()))?;
                    let verdict = classify_against_part(
                        self,
                        &defs,
                        is_passive,
                        &candidate_full,
                        &candidate_partial,
                        &part_id,
                    )?;
                    if let Some((verdict, explanation)) = verdict {
                        record_best(
                            &mut best,
                            part_id.as_str().to_string(),
                            verdict,
                            explanation,
                        );
                    }
                }
            }
        }

        let mut results = Vec::with_capacity(best.len());
        for (part_id_str, (verdict, explanation)) in best {
            let part_id = PartId::from_string(part_id_str)
                .map_err(|_| DbError::Corrupt("bad part id".into()))?;
            let display_name = self
                .get_part(&part_id)?
                .map(|p| p.display_name)
                .unwrap_or_default();
            results.push(MatchResult {
                part_id,
                display_name,
                verdict_kind: verdict.kind_str().to_string(),
                explanation,
                rank: verdict.rank(),
            });
        }
        results.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then_with(|| a.part_id.as_str().cmp(b.part_id.as_str()))
        });
        Ok(results)
    }

    /// Score every other non-archived part in `part_id`'s category against
    /// it: an `equivalence_decisions` 'rejected' pair is skipped entirely
    /// (never re-suggested); an 'approved' pair is surfaced at rank 3
    /// ("Previously approved as equivalent") without recomputing identity;
    /// everything else falls through the same identity hierarchy
    /// `find_matches` uses (levels 4-6).
    pub fn suggest_duplicates(&mut self, part_id: &PartId) -> Result<Vec<MatchResult>, DbError> {
        let part = self.get_part(part_id)?.ok_or(DbError::PartNotFound)?;
        let defs = identity::identity_defs(self, &part.category_id)?;
        let is_passive = is_passive_category(self, &part.category_id)?;
        let mine_full = identity::identity_signature(self, part_id)?;
        let mine_partial = if defs.is_empty() {
            BTreeMap::new()
        } else {
            build_partial_from_part(self, part_id, &defs)?
        };

        let mut stmt = self.raw_conn().prepare(
            "SELECT id, display_name FROM parts WHERE category_id = ?1 AND archived = 0 AND id != ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![
            part.category_id.as_str(),
            part_id.as_str()
        ])?;
        let mut others = Vec::new();
        while let Some(row) = rows.next()? {
            others.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
        }
        drop(rows);
        drop(stmt);

        let mut results = Vec::new();
        for (other_id_str, display_name) in others {
            let other_id = PartId::from_string(other_id_str)
                .map_err(|_| DbError::Corrupt("bad part id".into()))?;

            if let Some(decision) = self.equivalence_between(part_id, &other_id)? {
                match decision.as_str() {
                    "rejected" => continue,
                    "approved" => {
                        results.push(MatchResult {
                            part_id: other_id,
                            display_name,
                            verdict_kind: MatchVerdict::KnownAlias.kind_str().to_string(),
                            explanation: "Previously approved as equivalent".to_string(),
                            rank: MatchVerdict::KnownAlias.rank(),
                        });
                        continue;
                    }
                    _ => {}
                }
            }

            if defs.is_empty() {
                continue;
            }
            let other_full = identity::identity_signature(self, &other_id)?;
            let verdict = match (&mine_full, &other_full) {
                (Some(a), Some(b)) if identity::signatures_equal(a, b) => {
                    Some(exact_verdict(is_passive, a))
                }
                _ => {
                    let other_partial = build_partial_from_part(self, &other_id, &defs)?;
                    let cmp = compare_identity(&mine_partial, &other_partial);
                    classify_partial(&cmp)
                }
            };
            if let Some((verdict, explanation)) = verdict {
                results.push(MatchResult {
                    part_id: other_id,
                    display_name,
                    verdict_kind: verdict.kind_str().to_string(),
                    explanation,
                    rank: verdict.rank(),
                });
            }
        }
        results.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then_with(|| a.part_id.as_str().cmp(b.part_id.as_str()))
        });
        Ok(results)
    }

    /// Remember a user judgment about a part pair. Canonicalizes `(a, b)` to
    /// `part_a < part_b` before writing (matches the schema's `CHECK` and
    /// `UNIQUE` ordering) and upserts, so recording a pair again (e.g.
    /// flipping a decision) updates the existing row rather than erroring.
    pub fn record_equivalence(
        &mut self,
        a: &PartId,
        b: &PartId,
        decision: &str,
        note: &str,
    ) -> Result<(), DbError> {
        if decision != "approved" && decision != "rejected" {
            return Err(DbError::InvalidAttributeValue {
                key: "decision".to_string(),
                reason: format!(
                    "'{decision}' is not a valid equivalence decision (must be 'approved' or 'rejected')"
                ),
            });
        }
        if a == b {
            return Err(DbError::InvalidAttributeValue {
                key: "pair".to_string(),
                reason: "a part cannot be recorded as equivalent to itself".to_string(),
            });
        }
        let (lo, hi) = canonical_order(a, b);
        let id = inventory_core::id::new_id();
        self.raw_conn().execute(
            "INSERT INTO equivalence_decisions (id, part_a, part_b, decision, note)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(part_a, part_b) DO UPDATE SET
                 decision = excluded.decision,
                 note = excluded.note",
            rusqlite::params![id, lo, hi, decision, note],
        )?;
        Ok(())
    }

    /// The recorded decision ('approved' | 'rejected') for a part pair, if
    /// any judgment has ever been made about it (canonical-pair lookup).
    pub fn equivalence_between(&self, a: &PartId, b: &PartId) -> Result<Option<String>, DbError> {
        let (lo, hi) = canonical_order(a, b);
        let decision = self
            .raw_conn()
            .query_row(
                "SELECT decision FROM equivalence_decisions WHERE part_a = ?1 AND part_b = ?2",
                rusqlite::params![lo, hi],
                |r| r.get(0),
            )
            .optional()?;
        Ok(decision)
    }

    /// Remember a supplier SKU or MPN seen for `part_id` so a repeat import
    /// resolves straight to `KnownAlias` next time. `(kind, value)` must be
    /// globally unique (schema `UNIQUE`); a collision — the same SKU/MPN
    /// already pointing at a different part — surfaces as `AliasTaken`.
    pub fn add_alias(
        &mut self,
        kind: &str,
        value: &str,
        part_id: &PartId,
        source: &str,
    ) -> Result<(), DbError> {
        if kind != "supplier_sku" && kind != "mpn" {
            return Err(DbError::InvalidAttributeValue {
                key: "kind".to_string(),
                reason: format!(
                    "'{kind}' is not a valid alias kind (must be 'supplier_sku' or 'mpn')"
                ),
            });
        }
        if self.get_part(part_id)?.is_none() {
            return Err(DbError::PartNotFound);
        }
        let id = inventory_core::id::new_id();
        self.raw_conn()
            .execute(
                "INSERT INTO part_aliases (id, alias_kind, alias_value, part_id, source)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, kind, value, part_id.as_str(), source],
            )
            .map_err(map_unique_to_alias_taken)?;
        Ok(())
    }
}

/// Keep the better (lower-ranked) verdict recorded for a given part id; the
/// first-recorded verdict wins any tie, so hierarchy walk order only
/// matters for which explanation prose is kept, never for which rank wins.
fn record_best(
    best: &mut HashMap<String, (MatchVerdict, String)>,
    part_id: String,
    verdict: MatchVerdict,
    explanation: String,
) {
    let rank = verdict.rank();
    best.entry(part_id)
        .and_modify(|entry| {
            if rank < entry.0.rank() {
                *entry = (verdict.clone(), explanation.clone());
            }
        })
        .or_insert((verdict, explanation));
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// `candidate.attributes`, with `candidate.package` folded in as a
/// `("package", _)` entry when the candidate supplied it that way and
/// `attributes` doesn't already carry one. A caller (e.g. an importer) may
/// send package either as its own dedicated field or inline in
/// `attributes` — both must be treated identically by identity matching, so
/// this is the single place that reconciles them before any signature is
/// built. Never double-counts: an existing `("package", _)` entry in
/// `attributes` always wins over the `package` field.
fn with_package_folded_in(candidate: &MatchCandidate) -> Vec<(String, String)> {
    let mut attributes = candidate.attributes.clone();
    if let Some(package) = &candidate.package {
        if !attributes.iter().any(|(k, _)| k == "package") {
            attributes.push(("package".to_string(), package.clone()));
        }
    }
    attributes
}

/// PASSIVE-category membership by seeded name (ids are per-install and
/// can't be relied on). A category whose name isn't in `PASSIVE_CATEGORIES`
/// — including every user-defined custom category — is ACTIVE: exact
/// identity caps at `ProbableEquivalent` rather than auto-combining.
fn is_passive_category(db: &Database, category_id: &CategoryId) -> Result<bool, DbError> {
    let name: String = db
        .raw_conn()
        .query_row(
            "SELECT name FROM categories WHERE id = ?1",
            [category_id.as_str()],
            |r| r.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::CategoryNotFound,
            other => DbError::Sqlite(other),
        })?;
    Ok(PASSIVE_CATEGORIES.contains(&name.as_str()))
}

/// Levels 4-6 for one (candidate, part) pair: try the clean "everything
/// matches" case first via Task 2's own signature machinery, then fall back
/// to a partial per-field comparison for the probable/similar cases.
fn classify_against_part(
    db: &Database,
    defs: &[IdentityDef],
    is_passive: bool,
    candidate_full: &Option<BTreeMap<String, IdentityValue>>,
    candidate_partial: &BTreeMap<String, Option<IdentityValue>>,
    part_id: &PartId,
) -> Result<Option<(MatchVerdict, String)>, DbError> {
    let part_full = identity::identity_signature(db, part_id)?;
    if let (Some(a), Some(b)) = (candidate_full, &part_full) {
        if identity::signatures_equal(a, b) {
            return Ok(Some(exact_verdict(is_passive, a)));
        }
    }
    let part_partial = build_partial_from_part(db, part_id, defs)?;
    let cmp = compare_identity(candidate_partial, &part_partial);
    Ok(classify_partial(&cmp))
}

/// The rank-4/5 verdict for a pair whose full identity signatures matched
/// exactly: `ExactIdentity` for PASSIVE categories, capped at
/// `ProbableEquivalent` for ACTIVE ones (never silently auto-combined).
fn exact_verdict(
    is_passive: bool,
    sig: &BTreeMap<String, IdentityValue>,
) -> (MatchVerdict, String) {
    let names = friendly_names(sig.keys());
    let joined = capitalize(&join_and(&names));
    if is_passive {
        (MatchVerdict::ExactIdentity, format!("{joined} all match"))
    } else {
        (
            MatchVerdict::ProbableEquivalent,
            format!("{joined} match, but active/IC parts are never combined automatically"),
        )
    }
}

struct IdentityComparison {
    total: usize,
    matched: Vec<String>,
    conflicting: Vec<String>,
    missing: Vec<String>,
}

/// Per-field classification of two partial identity maps built from the
/// *same* def list (so their key sets are identical): a field is `matched`
/// when both sides have a value and it's equal, `conflicting` when both
/// have a value and it differs, and `missing` otherwise (absent — or
/// unparsable, which `value_for_def` already folds into absent — on at
/// least one side).
fn compare_identity(
    a: &BTreeMap<String, Option<IdentityValue>>,
    b: &BTreeMap<String, Option<IdentityValue>>,
) -> IdentityComparison {
    let mut matched = Vec::new();
    let mut conflicting = Vec::new();
    let mut missing = Vec::new();
    for (key, av) in a {
        let bv = b.get(key).and_then(|v| v.as_ref());
        match (av.as_ref(), bv) {
            (Some(x), Some(y)) if x == y => matched.push(key.clone()),
            (Some(_), Some(_)) => conflicting.push(key.clone()),
            _ => missing.push(key.clone()),
        }
    }
    IdentityComparison {
        total: a.len(),
        matched,
        conflicting,
        missing,
    }
}

/// Ranks 5 ("probable") and 6 ("similar") only — the rank-4 "everything
/// matches" case is handled upstream via `identity_signature`/
/// `signatures_equal` before this is ever called on a matched pair, so a
/// zero-missing zero-conflicting `cmp` never reaches here in practice; if it
/// did (e.g. a category with no defs), it falls through to `None` below
/// rather than double-reporting an exact match.
fn classify_partial(cmp: &IdentityComparison) -> Option<(MatchVerdict, String)> {
    if cmp.total == 0 {
        return None;
    }
    if cmp.conflicting.is_empty() && cmp.missing.len() == 1 && !cmp.matched.is_empty() {
        let matched_names = friendly_names(cmp.matched.iter());
        let missing_name = friendly_field_name(&cmp.missing[0]);
        let explanation = format!(
            "{} match, but {} is missing",
            capitalize(&join_and(&matched_names)),
            missing_name
        );
        return Some((MatchVerdict::ProbableEquivalent, explanation));
    }
    if cmp.matched.len() >= 2 && !cmp.conflicting.is_empty() {
        let conflict_names = friendly_names(cmp.conflicting.iter());
        let verb = if conflict_names.len() == 1 {
            "differs"
        } else {
            "differ"
        };
        let explanation = format!("Similar device, but {} {verb}", join_and(&conflict_names));
        return Some((MatchVerdict::Similar, explanation));
    }
    None
}

/// Build a part's identity fields as a *partial* map (missing/unparsable
/// fields become `None` instead of aborting the whole map the way
/// `identity_signature` does) — needed here because probable/similar
/// comparisons are specifically about incomplete or conflicting data.
fn build_partial_from_part(
    db: &Database,
    part_id: &PartId,
    defs: &[IdentityDef],
) -> Result<BTreeMap<String, Option<IdentityValue>>, DbError> {
    let mut out = BTreeMap::new();
    for def in defs {
        let original: Option<String> = db
            .raw_conn()
            .query_row(
                "SELECT v.original_text
                 FROM part_attribute_values v
                 JOIN attribute_defs a ON a.id = v.attribute_id
                 WHERE v.part_id = ?1 AND a.key = ?2",
                rusqlite::params![part_id.as_str(), def.key],
                |r| r.get(0),
            )
            .optional()?;
        let value = original.and_then(|o| identity::value_for_def(def, &o));
        out.insert(def.key.clone(), value);
    }
    Ok(out)
}

/// Same as `build_partial_from_part` but sourced from an in-memory
/// `(key, value)` slice (a candidate's raw attributes) instead of stored
/// `part_attribute_values`.
fn build_partial_from_values(
    defs: &[IdentityDef],
    attrs: &[(String, String)],
) -> BTreeMap<String, Option<IdentityValue>> {
    let mut out = BTreeMap::new();
    for def in defs {
        let value = attrs
            .iter()
            .find(|(k, _)| k == &def.key)
            .and_then(|(_, raw)| identity::value_for_def(def, raw));
        out.insert(def.key.clone(), value);
    }
    out
}

fn friendly_names<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<String> {
    keys.map(|k| friendly_field_name(k)).collect()
}

/// A human-friendly rendering of an identity attribute key for prose
/// explanations: drop a trailing `_rating` (`power_rating` -> "power",
/// `voltage_rating` -> "voltage") and turn remaining underscores into
/// spaces (`channel_count` -> "channel count"). Not the attribute's display
/// `label` — the label ("Type / dielectric") doesn't read naturally inside
/// a sentence the way this heuristic does.
fn friendly_field_name(key: &str) -> String {
    key.strip_suffix("_rating").unwrap_or(key).replace('_', " ")
}

/// Oxford-comma join: "a", "a and b", "a, b, and c".
fn join_and(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        _ => {
            let (last, rest) = items.split_last().expect("len >= 3 checked above");
            format!("{}, and {last}", rest.join(", "))
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn canonical_order<'a>(a: &'a PartId, b: &'a PartId) -> (&'a str, &'a str) {
    if a.as_str() < b.as_str() {
        (a.as_str(), b.as_str())
    } else {
        (b.as_str(), a.as_str())
    }
}

/// `part_aliases.(alias_kind, alias_value)` is `UNIQUE` (extended code
/// 2067); translate a violation into the typed error. Mirrors
/// `categories::map_unique_to_category_name_taken`.
fn map_unique_to_alias_taken(e: rusqlite::Error) -> DbError {
    if let rusqlite::Error::SqliteFailure(err, _) = &e {
        if err.extended_code == 2067 {
            return DbError::AliasTaken;
        }
    }
    DbError::Sqlite(e)
}
