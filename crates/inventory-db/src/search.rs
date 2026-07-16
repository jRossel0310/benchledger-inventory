//! Full-text + structured search over parts (Phase 2c).
//!
//! `refresh_search_text` rebuilds a part's denormalized search blob from
//! every piece of content the search bar can match against; the migration
//! 0004 triggers keep `parts_fts` in sync automatically whenever that row
//! changes (INSERT/UPDATE/DELETE all propagate — see `tests/schema.rs`'s
//! `fts_stays_in_sync_with_search_text`). `search` parses a raw query
//! string (`inventory_core::search::parse_query`), narrows to an FTS
//! candidate set (or all parts when there's no free text), applies every
//! structured filter as a set intersection, applies the archived/low-stock
//! flags, and returns deterministically ordered hits.

use std::collections::{HashMap, HashSet};

use rusqlite::OptionalExtension;

use inventory_core::ids::PartId;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_core::search::{parse_query, FilterOp, RawFilter};
use inventory_core::units::{parse_with_kind, UnitKind};

use crate::attributes::split_range;
use crate::{Database, DbError};

/// One search result row: enough to render a result list without a further
/// per-part query. Quantities are in the part's real unit (reuses
/// `get_stock`'s milli -> `Quantity` plumbing).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SearchHit {
    pub part_id: PartId,
    pub display_name: String,
    pub category_name: String,
    pub bin_label: Option<String>,
    pub available: Quantity,
    pub reserved: Quantity,
    pub checked_out: Quantity,
    pub archived: bool,
}

/// Candidate part ids for a free-text query plus, when the query had text,
/// a parallel bm25-rank map for later ordering (`None` when every part is a
/// candidate, i.e. there was no free text to rank against).
type TextCandidates = (Vec<String>, Option<HashMap<String, f64>>);

/// Pre-flag-filtering row loaded for every surviving candidate; `part_id`
/// stays a raw string here so it can double as a `HashMap`/`HashSet` key
/// without repeated ULID validation, and is only converted to a typed
/// `PartId` in `into_hit`.
struct Row {
    part_id: String,
    display_name: String,
    category_name: String,
    bin_label: Option<String>,
    quantity_unit: QuantityUnit,
    available_milli: i64,
    reserved_milli: i64,
    checked_out_milli: i64,
    low_stock_threshold_milli: Option<i64>,
    archived: bool,
}

impl Row {
    fn into_hit(self) -> Result<SearchHit, DbError> {
        Ok(SearchHit {
            part_id: PartId::from_string(self.part_id)
                .map_err(|_| DbError::Corrupt("bad part id".into()))?,
            display_name: self.display_name,
            category_name: self.category_name,
            bin_label: self.bin_label,
            available: Quantity::from_milli(self.available_milli, self.quantity_unit)?,
            reserved: Quantity::from_milli(self.reserved_milli, self.quantity_unit)?,
            checked_out: Quantity::from_milli(self.checked_out_milli, self.quantity_unit)?,
            archived: self.archived,
        })
    }
}

impl Database {
    /// Rebuild `search_text.body` for `part_id` from every current piece of
    /// searchable content: display name, category name, description, tags,
    /// bin label, manufacturers, MPNs, supplier SKUs, attribute
    /// original-text + formatted value + key, and dimension names.
    /// Upserts so the migration 0004 triggers propagate to `parts_fts`.
    /// Called from every content-mutation method (create/update part,
    /// archive toggle, variant/listing/attribute/dimension/tag writes) —
    /// see call sites in `parts.rs`, `attributes.rs`, `dimensions.rs`, and
    /// `set_tags` below.
    pub(crate) fn refresh_search_text(&mut self, part_id: &PartId) -> Result<(), DbError> {
        let part = self.get_part(part_id)?.ok_or(DbError::PartNotFound)?;
        let category_name: String = self.raw_conn().query_row(
            "SELECT name FROM categories WHERE id = ?1",
            [part.category_id.as_str()],
            |r| r.get(0),
        )?;

        let mut terms: Vec<String> = vec![part.display_name, category_name, part.description];
        if let Some(bin) = part.bin_label {
            terms.push(bin);
        }

        {
            let mut stmt = self
                .raw_conn()
                .prepare("SELECT tag FROM part_tags WHERE part_id = ?1")?;
            let mut rows = stmt.query([part_id.as_str()])?;
            while let Some(row) = rows.next()? {
                terms.push(row.get(0)?);
            }
        }
        {
            let mut stmt = self.raw_conn().prepare(
                "SELECT manufacturer, mpn FROM manufacturer_variants WHERE part_id = ?1",
            )?;
            let mut rows = stmt.query([part_id.as_str()])?;
            while let Some(row) = rows.next()? {
                terms.push(row.get(0)?);
                terms.push(row.get(1)?);
            }
        }
        {
            let mut stmt = self.raw_conn().prepare(
                "SELECT sl.supplier_sku FROM supplier_listings sl
                 JOIN manufacturer_variants mv ON mv.id = sl.variant_id
                 WHERE mv.part_id = ?1",
            )?;
            let mut rows = stmt.query([part_id.as_str()])?;
            while let Some(row) = rows.next()? {
                terms.push(row.get(0)?);
            }
        }
        {
            let mut stmt = self.raw_conn().prepare(
                "SELECT a.key, a.data_type, a.unit_kind, v.original_text
                 FROM part_attribute_values v JOIN attribute_defs a ON a.id = v.attribute_id
                 WHERE v.part_id = ?1",
            )?;
            let mut rows = stmt.query([part_id.as_str()])?;
            while let Some(row) = rows.next()? {
                let key: String = row.get(0)?;
                let data_type: String = row.get(1)?;
                let unit_kind: Option<String> = row.get(2)?;
                let original: String = row.get(3)?;
                terms.push(key);
                terms.push(original.clone());
                let kind = unit_kind.as_deref().and_then(UnitKind::from_sql);
                match (data_type.as_str(), kind) {
                    ("number_unit", Some(kind)) => {
                        if let Ok(v) = parse_with_kind(&original, kind) {
                            terms.push(v.format());
                        }
                    }
                    ("range", Some(kind)) => {
                        if let Some((lo, hi)) = split_range(&original) {
                            if let (Ok(lo), Ok(hi)) = (
                                parse_with_kind(lo.trim(), kind),
                                parse_with_kind(hi.trim(), kind),
                            ) {
                                terms.push(lo.format());
                                terms.push(hi.format());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        {
            let mut stmt = self
                .raw_conn()
                .prepare("SELECT name FROM dimensions WHERE part_id = ?1")?;
            let mut rows = stmt.query([part_id.as_str()])?;
            while let Some(row) = rows.next()? {
                terms.push(row.get(0)?);
            }
        }

        let body = terms
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        self.raw_conn().execute(
            "INSERT INTO search_text (part_id, body) VALUES (?1, ?2)
             ON CONFLICT(part_id) DO UPDATE SET body = excluded.body",
            rusqlite::params![part_id.as_str(), body],
        )?;
        Ok(())
    }

    /// Replace a part's tag set (part_tags) and refresh its search text.
    /// The search spec needs a tags mutation entry point that doesn't
    /// otherwise exist yet in the parts/attributes/dimensions APIs.
    pub fn set_tags(&mut self, part_id: &PartId, tags: &[String]) -> Result<(), DbError> {
        if self.get_part(part_id)?.is_none() {
            return Err(DbError::PartNotFound);
        }
        {
            let tx = self.conn_mut().transaction()?;
            tx.execute(
                "DELETE FROM part_tags WHERE part_id = ?1",
                [part_id.as_str()],
            )?;
            for tag in tags {
                tx.execute(
                    "INSERT INTO part_tags (part_id, tag) VALUES (?1, ?2)",
                    rusqlite::params![part_id.as_str(), tag],
                )?;
            }
            tx.commit()?;
        }
        self.refresh_search_text(part_id)
    }

    /// Parse `query`, narrow to an FTS candidate set (or all parts when
    /// there's no free text), apply every structured filter (set
    /// intersection) and flag (archived/low-stock post-filter), and return
    /// deterministically ordered hits: bm25 rank when free text was
    /// present, else display name — part id as the final tiebreak either
    /// way.
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, DbError> {
        let parsed = parse_query(query);

        let (candidate_ids, ranks) = self.text_candidates(parsed.text.trim())?;
        let mut set: HashSet<String> = candidate_ids.into_iter().collect();
        for filter in &parsed.filters {
            let matched = self.apply_filter(filter)?;
            set = set.intersection(&matched).cloned().collect();
        }

        let mut rows = Vec::with_capacity(set.len());
        for part_id in &set {
            if let Some(row) = self.load_row(part_id)? {
                rows.push(row);
            }
        }

        // Default (no is:archived flag) excludes archived parts; is:archived
        // returns only archived parts.
        let archived_only = parsed.flags.archived == Some(true);
        rows.retain(|r| r.archived == archived_only);
        if parsed.flags.low_stock {
            rows.retain(|r| match r.low_stock_threshold_milli {
                Some(threshold) => r.available_milli < threshold,
                None => false,
            });
        }

        if let Some(ranks) = &ranks {
            rows.sort_by(|a, b| {
                let ra = ranks.get(&a.part_id).copied().unwrap_or(0.0);
                let rb = ranks.get(&b.part_id).copied().unwrap_or(0.0);
                ra.partial_cmp(&rb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.part_id.cmp(&b.part_id))
            });
        } else {
            rows.sort_by(|a, b| {
                a.display_name
                    .cmp(&b.display_name)
                    .then_with(|| a.part_id.cmp(&b.part_id))
            });
        }

        rows.into_iter().map(Row::into_hit).collect()
    }

    /// Candidate part ids for the query's free text: every part id when
    /// `text` is empty, otherwise the FTS5 `MATCH` result (each term
    /// sanitized into a quoted prefix phrase, joined by implicit AND) with
    /// a parallel bm25-rank map for later ordering.
    fn text_candidates(&self, text: &str) -> Result<TextCandidates, DbError> {
        if text.is_empty() {
            let mut stmt = self.raw_conn().prepare("SELECT id FROM parts")?;
            let mut rows = stmt.query([])?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next()? {
                ids.push(row.get::<_, String>(0)?);
            }
            return Ok((ids, None));
        }
        let match_expr = sanitize_fts_query(text);
        let mut stmt = self
            .raw_conn()
            .prepare("SELECT part_id, bm25(parts_fts) FROM parts_fts WHERE parts_fts MATCH ?1")?;
        let mut rows = stmt.query([match_expr.as_str()])?;
        let mut ids = Vec::new();
        let mut ranks = HashMap::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let rank: f64 = row.get(1)?;
            ranks.insert(id.clone(), rank);
            ids.push(id);
        }
        Ok((ids, Some(ranks)))
    }

    fn load_row(&self, part_id: &str) -> Result<Option<Row>, DbError> {
        let result = self.raw_conn().query_row(
            "SELECT p.id, p.display_name, c.name, p.bin_label, p.quantity_unit,
                    p.low_stock_threshold_milli, p.archived,
                    s.available_milli, s.reserved_milli, s.checked_out_milli
             FROM parts p
             JOIN categories c ON c.id = p.category_id
             JOIN part_stock s ON s.part_id = p.id
             WHERE p.id = ?1",
            [part_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, bool>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                ))
            },
        );
        match result {
            Ok((
                id,
                display_name,
                category_name,
                bin_label,
                unit_raw,
                threshold,
                archived,
                available,
                reserved,
                checked_out,
            )) => {
                let quantity_unit = QuantityUnit::from_sql(&unit_raw).ok_or_else(|| {
                    DbError::Corrupt(format!("unknown quantity_unit '{unit_raw}'"))
                })?;
                Ok(Some(Row {
                    part_id: id,
                    display_name,
                    category_name,
                    bin_label,
                    quantity_unit,
                    available_milli: available,
                    reserved_milli: reserved,
                    checked_out_milli: checked_out,
                    low_stock_threshold_milli: threshold,
                    archived,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    fn apply_filter(&self, filter: &RawFilter) -> Result<HashSet<String>, DbError> {
        match filter.key.as_str() {
            "project" => self.filter_project(&filter.value),
            "bin" => self.filter_exact_ci("bin_label", &filter.value),
            "category" => self.filter_category(&filter.value),
            "has" => self.filter_has(&filter.value),
            "available" => self.filter_stock_column("available_milli", filter),
            "reserved" => self.filter_stock_column("reserved_milli", filter),
            "checked_out" => self.filter_stock_column("checked_out_milli", filter),
            "stock" => self.filter_stock_column(
                "(available_milli + reserved_milli + checked_out_milli)",
                filter,
            ),
            key => self.filter_attribute_or_dimension(key, filter),
        }
    }

    /// `project:` — parts with a reservation/checkout/transfer transaction
    /// (either leg) against a project whose name LIKE %value%.
    fn filter_project(&self, value: &str) -> Result<HashSet<String>, DbError> {
        let pattern = format!("%{value}%");
        let mut stmt = self.raw_conn().prepare(
            "SELECT DISTINCT part_id FROM transactions
             WHERE project_id IN (SELECT id FROM projects WHERE name LIKE ?1 COLLATE NOCASE)
                OR to_project_id IN (SELECT id FROM projects WHERE name LIKE ?1 COLLATE NOCASE)",
        )?;
        let out = collect_ids(stmt.query([pattern.as_str()])?)?;
        Ok(out)
    }

    /// `bin:` — exact, case-insensitive match on `parts.bin_label`.
    fn filter_exact_ci(&self, column: &str, value: &str) -> Result<HashSet<String>, DbError> {
        let sql = format!("SELECT id FROM parts WHERE {column} = ?1 COLLATE NOCASE");
        let mut stmt = self.raw_conn().prepare(&sql)?;
        let out = collect_ids(stmt.query([value])?)?;
        Ok(out)
    }

    /// `category:` — case-insensitive category name match.
    fn filter_category(&self, value: &str) -> Result<HashSet<String>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT id FROM parts WHERE category_id IN
             (SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE)",
        )?;
        let out = collect_ids(stmt.query([value])?)?;
        Ok(out)
    }

    /// `has:` — `datasheet` (a variant with a non-null `datasheet_url`) and
    /// `dimensions` (any dimensions row) are supported; `footprint` is a
    /// real, recognized concept that just isn't modeled yet (CAD links
    /// arrive with Phase 3), so it gets a typed `UnsupportedSearchKey`
    /// rather than silently returning an empty result set. Anything else is
    /// `UnknownSearchKey`.
    fn filter_has(&self, value: &str) -> Result<HashSet<String>, DbError> {
        match value {
            "datasheet" => {
                let mut stmt = self.raw_conn().prepare(
                    "SELECT DISTINCT part_id FROM manufacturer_variants WHERE datasheet_url IS NOT NULL",
                )?;
                let out = collect_ids(stmt.query([])?)?;
                Ok(out)
            }
            "dimensions" => {
                let mut stmt = self
                    .raw_conn()
                    .prepare("SELECT DISTINCT part_id FROM dimensions")?;
                let out = collect_ids(stmt.query([])?)?;
                Ok(out)
            }
            "footprint" => Err(DbError::UnsupportedSearchKey(
                "footprint (arrives with CAD links in Phase 3)".to_string(),
            )),
            other => Err(DbError::UnknownSearchKey(format!("has:{other}"))),
        }
    }

    /// `available:` / `reserved:` / `checked_out:` / `stock:` — numeric ops
    /// against `part_stock` milli columns (or their sum for `stock`); the
    /// filter value is whole units, scaled by `Quantity::SCALE`.
    fn filter_stock_column(
        &self,
        column: &str,
        filter: &RawFilter,
    ) -> Result<HashSet<String>, DbError> {
        let sql = format!("SELECT part_id, {column} FROM part_stock");
        let mut stmt = self.raw_conn().prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut out = HashSet::new();
        while let Some(row) = rows.next()? {
            let part_id: String = row.get(0)?;
            let stored: i64 = row.get(1)?;
            if stock_matches(&filter.op, &filter.value, stored)? {
                out.insert(part_id);
            }
        }
        Ok(out)
    }

    /// Resolve `key` against `attribute_defs` first; if that fails, try it
    /// as a (lowercased) dimension name; if that also turns up nothing
    /// anywhere in the database, the key is genuinely unrecognized.
    fn filter_attribute_or_dimension(
        &self,
        key: &str,
        filter: &RawFilter,
    ) -> Result<HashSet<String>, DbError> {
        let attr: Option<(String, String, Option<String>)> = self
            .raw_conn()
            .query_row(
                "SELECT id, data_type, unit_kind FROM attribute_defs WHERE key = ?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((attribute_id, data_type, unit_kind)) = attr {
            return self.filter_by_attribute(
                &attribute_id,
                &data_type,
                unit_kind.as_deref(),
                filter,
            );
        }

        let lower = key.to_lowercase();
        let mut stmt = self
            .raw_conn()
            .prepare("SELECT part_id, normalized_value FROM dimensions WHERE LOWER(name) = ?1")?;
        let mut rows = stmt.query([lower.as_str()])?;
        let mut out = HashSet::new();
        let mut any = false;
        while let Some(row) = rows.next()? {
            any = true;
            let part_id: String = row.get(0)?;
            let normalized: f64 = row.get(1)?;
            if dimension_matches(&filter.op, &filter.value, normalized)? {
                out.insert(part_id);
            }
        }
        if any {
            return Ok(out);
        }
        Err(DbError::UnknownSearchKey(key.to_string()))
    }

    /// `number_unit`/`range` attributes compare `value_num` (the exact
    /// re-parse for `range`'s upper bound, `value_num_hi`, is intentionally
    /// not consulted — see the module-level note on range-attribute
    /// filtering being a simplified, documented approximation). Every other
    /// data type falls back to a case-insensitive equality match on
    /// `original_text` (undocumented by the brief, but needed so an
    /// unrecognized-op filter on a text/choice attribute degrades safely
    /// rather than panicking).
    fn filter_by_attribute(
        &self,
        attribute_id: &str,
        data_type: &str,
        unit_kind: Option<&str>,
        filter: &RawFilter,
    ) -> Result<HashSet<String>, DbError> {
        let invalid = |reason: String| DbError::InvalidAttributeValue {
            key: filter.key.clone(),
            reason,
        };
        match (data_type, unit_kind.and_then(UnitKind::from_sql)) {
            (dt, Some(kind)) if dt == "number_unit" || dt == "range" => {
                let mut stmt = self.raw_conn().prepare(
                    "SELECT part_id, value_num FROM part_attribute_values WHERE attribute_id = ?1",
                )?;
                let mut rows = stmt.query([attribute_id])?;
                let mut out = HashSet::new();
                while let Some(row) = rows.next()? {
                    let part_id: String = row.get(0)?;
                    let value_num: Option<f64> = row.get(1)?;
                    let Some(stored) = value_num else {
                        continue;
                    };
                    if numeric_filter_matches(&filter.op, &filter.value, kind, stored)
                        .map_err(|e| invalid(e.to_string()))?
                    {
                        out.insert(part_id);
                    }
                }
                Ok(out)
            }
            _ => {
                let mut stmt = self.raw_conn().prepare(
                    "SELECT part_id FROM part_attribute_values
                     WHERE attribute_id = ?1 AND original_text = ?2 COLLATE NOCASE",
                )?;
                let out = collect_ids(stmt.query(rusqlite::params![attribute_id, filter.value])?)?;
                Ok(out)
            }
        }
    }
}

fn collect_ids(mut rows: rusqlite::Rows<'_>) -> Result<HashSet<String>, DbError> {
    let mut out = HashSet::new();
    while let Some(row) = rows.next()? {
        out.insert(row.get(0)?);
    }
    Ok(out)
}

/// Sanitize free-text terms into an FTS5 `MATCH` expression: each
/// whitespace-separated term becomes a double-quoted phrase (embedded
/// quotes doubled, the standard FTS5 string-literal escape) immediately
/// followed by `*` for a prefix match, joined by spaces (FTS5's implicit
/// AND between phrases). Quoting means punctuation inside a term (parens,
/// stray quotes) can never be interpreted as FTS5 query syntax — confirmed
/// empirically against rusqlite's bundled SQLite that terms containing `"`,
/// `(`, `)`, or reserved words like `AND`/`OR`/`NOT` never raise a MATCH
/// syntax error. The trailing `*` is what makes `search("TLV9002")` find a
/// token like `tlv9002iddfr` (tokenchars `-_.` keep hyphenated SKUs and
/// part numbers as single tokens, so exact-phrase-only matching would miss
/// partial part numbers).
fn sanitize_fts_query(text: &str) -> String {
    text.split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn compare(op: &FilterOp, stored: f64, target: f64) -> bool {
    match op {
        FilterOp::Eq => (stored - target).abs() <= target.abs().max(1.0) * 1e-9,
        FilterOp::Lt => stored < target,
        FilterOp::Le => stored <= target,
        FilterOp::Gt => stored > target,
        FilterOp::Ge => stored >= target,
        // Range is always intercepted by its caller before reaching here.
        FilterOp::Range => stored >= target,
    }
}

/// `available:`/`reserved:`/`checked_out:`/`stock:` values are whole units;
/// scale to milli before comparing against the stored column.
fn stock_matches(op: &FilterOp, raw_value: &str, stored: i64) -> Result<bool, DbError> {
    let parse = |s: &str| -> Result<i64, DbError> {
        s.trim()
            .parse::<f64>()
            .map(|f| (f * Quantity::SCALE as f64).round() as i64)
            .map_err(|_| DbError::InvalidAttributeValue {
                key: "stock".into(),
                reason: format!("'{s}' is not a number"),
            })
    };
    match op {
        FilterOp::Range => {
            let (lo, hi) =
                split_range(raw_value).ok_or_else(|| DbError::InvalidAttributeValue {
                    key: "stock".into(),
                    reason: format!("'{raw_value}' is not a valid range"),
                })?;
            let lo = parse(lo)?;
            let hi = parse(hi)?;
            Ok(stored >= lo && stored <= hi)
        }
        _ => {
            let v = parse(raw_value)?;
            Ok(compare(op, stored as f64, v as f64))
        }
    }
}

/// `number_unit`/`range` attribute filters: parse the filter value under
/// the attribute's own unit kind and compare against the stored
/// (already-normalized) `value_num`.
fn numeric_filter_matches(
    op: &FilterOp,
    raw_value: &str,
    kind: UnitKind,
    stored: f64,
) -> Result<bool, inventory_core::units::UnitParseError> {
    match op {
        FilterOp::Range => {
            let (lo, hi) = split_range(raw_value).ok_or(
                inventory_core::units::UnitParseError::Malformed(raw_value.to_string()),
            )?;
            let lo = parse_with_kind(lo.trim(), kind)?.to_f64();
            let hi = parse_with_kind(hi.trim(), kind)?.to_f64();
            Ok(stored >= lo && stored <= hi)
        }
        _ => {
            let v = parse_with_kind(raw_value, kind)?.to_f64();
            Ok(compare(op, stored, v))
        }
    }
}

/// Dimension filters (`height:<10mm`, etc.): parse the value as a length
/// first (converted to millimeters, matching `dimensions.normalized_value`
/// for lengths), falling back to mass (already grams) — mirrors
/// `dimensions::add_dimension`'s own length-then-mass parse order.
fn dimension_matches(op: &FilterOp, raw_value: &str, normalized: f64) -> Result<bool, DbError> {
    let invalid =
        |s: &str| DbError::InvalidDimension(format!("'{s}' is not a valid length or mass"));
    let parse_mm_or_g = |s: &str| -> Result<f64, DbError> {
        if let Ok(v) = parse_with_kind(s, UnitKind::Length) {
            return Ok(v.to_f64() * 1000.0);
        }
        if let Ok(v) = parse_with_kind(s, UnitKind::Mass) {
            return Ok(v.to_f64());
        }
        Err(invalid(s))
    };
    match op {
        FilterOp::Range => {
            let (lo, hi) = split_range(raw_value).ok_or_else(|| invalid(raw_value))?;
            let lo = parse_mm_or_g(lo.trim())?;
            let hi = parse_mm_or_g(hi.trim())?;
            Ok(normalized >= lo && normalized <= hi)
        }
        _ => {
            let v = parse_mm_or_g(raw_value)?;
            Ok(compare(op, normalized, v))
        }
    }
}
