//! Exact-form identity signatures for duplicate matching (Phase 2c).
//!
//! An identity signature is the set of a part's identity-flagged attribute
//! values, each re-parsed into its exact form so that equivalent notations
//! ("10k" vs "10000 ohm", "0603" vs "1608 metric") compare equal. Comparison
//! NEVER goes through `f64` — see `inventory_core::units::ParsedValue`'s
//! doc comment for why lossy float comparison is unsound here.
//!
//! `identity_defs` and `value_for_def` are the shared low-level primitives:
//! "which attributes define identity for a category" and "parse one raw
//! value under one identity def". Both `identity_signature` (stored-part
//! path) and `signature_from_values` (candidate/raw-value path used by
//! `matching`) build on top of them so there is exactly one place that
//! knows how to parse an identity value. `matching.rs` additionally builds
//! *partial* (some-fields-missing) maps from the same two primitives for
//! its probable/similar comparisons — see that module's doc comment.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;

use inventory_core::ids::{CategoryId, PartId};
use inventory_core::packages::normalize_package;
use inventory_core::units::{parse_with_kind, ParsedValue, UnitKind};

use crate::attributes::split_range;
use crate::{Database, DbError};

/// One identity-attribute value, held in its exact comparable form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdentityValue {
    /// `number_unit` attributes: the re-parsed exact value.
    Exact(ParsedValue),
    /// `range` attributes: re-parsed (low, high) bounds.
    Range(ParsedValue, ParsedValue),
    /// The `package` attribute: `normalize_package(...).canonical`.
    Package(String),
    /// `choice`/`text`/`number` attributes: the trimmed original text.
    Text(String),
}

pub(crate) struct IdentityDef {
    pub(crate) key: String,
    data_type: String,
    unit_kind: Option<UnitKind>,
}

/// Identity attribute definitions linked (and not hidden) to `category_id`,
/// ordered by key. Shared by every signature builder in this module and by
/// `matching.rs`'s partial-field comparisons so all of them agree on which
/// attributes define identity for a category.
pub(crate) fn identity_defs(
    db: &Database,
    category_id: &CategoryId,
) -> Result<Vec<IdentityDef>, DbError> {
    let mut stmt = db.raw_conn().prepare(
        "SELECT a.key, a.data_type, a.unit_kind
         FROM category_attributes ca
         JOIN attribute_defs a ON a.id = ca.attribute_id
         WHERE ca.category_id = ?1 AND a.identity = 1 AND ca.hidden = 0
         ORDER BY a.key",
    )?;
    let defs = stmt
        .query_map([category_id.as_str()], |r| {
            let unit_kind: Option<String> = r.get(2)?;
            Ok(IdentityDef {
                key: r.get(0)?,
                data_type: r.get(1)?,
                unit_kind: unit_kind.as_deref().and_then(UnitKind::from_sql),
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(defs)
}

/// Parse `original` under `def`'s data-type/unit-kind rules into its exact
/// comparable form. Returns `None` when the value can't be parsed under the
/// def (unparsable number/range, or an unrecognized package code) — callers
/// treat that the same as "missing": a signature is only as trustworthy as
/// its weakest field.
pub(crate) fn value_for_def(def: &IdentityDef, original: &str) -> Option<IdentityValue> {
    let original = original.trim();
    if def.key == "package" {
        return normalize_package(original).map(|np| IdentityValue::Package(np.canonical));
    }
    match def.data_type.as_str() {
        "number_unit" => {
            let kind = def.unit_kind?;
            parse_with_kind(original, kind)
                .ok()
                .map(IdentityValue::Exact)
        }
        "range" => {
            let kind = def.unit_kind?;
            let (lo, hi) = split_range(original)?;
            let lo = parse_with_kind(lo.trim(), kind).ok()?;
            let hi = parse_with_kind(hi.trim(), kind).ok()?;
            Some(IdentityValue::Range(lo, hi))
        }
        _ => Some(IdentityValue::Text(original.to_string())),
    }
}

/// Build a part's identity signature: one `IdentityValue` per identity
/// attribute linked (and not hidden) on the part's category, keyed by
/// attribute key.
///
/// Returns `Ok(None)` when:
/// - the part's category has no identity attributes (nothing defines identity), or
/// - any identity attribute is missing on the part, or
/// - any identity attribute's stored value fails to re-parse under its def
///   (shouldn't happen for values that passed `set_attribute`, but a
///   signature is only as trustworthy as its weakest field, so any
///   unparsable value makes identity indeterminate).
///
/// Non-identity attributes are ignored entirely.
pub fn identity_signature(
    db: &Database,
    part_id: &PartId,
) -> Result<Option<BTreeMap<String, IdentityValue>>, DbError> {
    let part = db.get_part(part_id)?.ok_or(DbError::PartNotFound)?;
    let defs = identity_defs(db, &part.category_id)?;
    if defs.is_empty() {
        return Ok(None);
    }

    let mut out = BTreeMap::new();
    for def in &defs {
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
        let Some(original) = original else {
            return Ok(None); // missing identity attribute -> incomplete identity
        };
        let Some(value) = value_for_def(def, &original) else {
            return Ok(None); // unparsable -> incomplete identity
        };
        out.insert(def.key.clone(), value);
    }
    Ok(Some(out))
}

/// Build a complete identity signature from raw `(key, value)` pairs — e.g.
/// a duplicate-matching candidate's attributes — instead of a stored part's
/// `part_attribute_values`. Reuses `value_for_def`, the same per-def parsing
/// `identity_signature` uses, so both sides of a candidate/part comparison
/// are held to identical exact-form rules. Same all-or-nothing contract:
/// `Ok(None)` when `category_id` has no identity attributes, any identity
/// attribute is absent from `attrs`, or any present value fails to parse
/// under its definition.
pub fn signature_from_values(
    db: &Database,
    category_id: &CategoryId,
    attrs: &[(String, String)],
) -> Result<Option<BTreeMap<String, IdentityValue>>, DbError> {
    let defs = identity_defs(db, category_id)?;
    if defs.is_empty() {
        return Ok(None);
    }

    let mut out = BTreeMap::new();
    for def in &defs {
        let Some((_, raw)) = attrs.iter().find(|(k, _)| k == &def.key) else {
            return Ok(None);
        };
        let Some(value) = value_for_def(def, raw) else {
            return Ok(None);
        };
        out.insert(def.key.clone(), value);
    }
    Ok(Some(out))
}

/// Whether two identity signatures denote the same part identity. Plain
/// `==`; exists so call sites read as domain logic rather than a stray
/// equality check.
pub fn signatures_equal(
    a: &BTreeMap<String, IdentityValue>,
    b: &BTreeMap<String, IdentityValue>,
) -> bool {
    a == b
}
