//! Structured physical dimensions: a raw value + unit as entered by the
//! user, normalized to millimeters (length) or grams (mass) for filtering
//! and comparison, while preserving the original display unit.

use inventory_core::ids::PartId;
use inventory_core::units::{parse_with_kind, UnitKind};

use crate::{Database, DbError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionGroup {
    Overall,
    Body,
    Mounting,
    Custom,
}

impl DimensionGroup {
    pub fn as_sql(&self) -> &'static str {
        match self {
            DimensionGroup::Overall => "overall",
            DimensionGroup::Body => "body",
            DimensionGroup::Mounting => "mounting",
            DimensionGroup::Custom => "custom",
        }
    }

    pub fn from_sql(s: &str) -> Option<Self> {
        Some(match s {
            "overall" => DimensionGroup::Overall,
            "body" => DimensionGroup::Body,
            "mounting" => DimensionGroup::Mounting,
            "custom" => DimensionGroup::Custom,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionSource {
    Manufacturer,
    Datasheet,
    Supplier,
    Measured,
    Estimated,
}

impl DimensionSource {
    pub fn as_sql(&self) -> &'static str {
        match self {
            DimensionSource::Manufacturer => "manufacturer",
            DimensionSource::Datasheet => "datasheet",
            DimensionSource::Supplier => "supplier",
            DimensionSource::Measured => "measured",
            DimensionSource::Estimated => "estimated",
        }
    }

    pub fn from_sql(s: &str) -> Option<Self> {
        Some(match s {
            "manufacturer" => DimensionSource::Manufacturer,
            "datasheet" => DimensionSource::Datasheet,
            "supplier" => DimensionSource::Supplier,
            "measured" => DimensionSource::Measured,
            "estimated" => DimensionSource::Estimated,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct DimensionDraft {
    pub group: DimensionGroup,
    pub name: String,      // "Length", "Pin pitch", "Knob outer diameter", ...
    pub raw_value: String, // "5 mm", "0.1 in", "1.5 g"
    pub source: DimensionSource,
    pub notes: String,
    pub measured_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DimensionRecord {
    pub id: String,
    pub part_id: PartId,
    pub group: DimensionGroup,
    pub name: String,
    pub value_num: f64,
    pub display_unit: String,
    pub normalized_value: f64,
    pub source: DimensionSource,
    pub notes: String,
    pub measured_date: Option<String>,
}

/// Split a trimmed raw value into its leading numeric portion and trailing
/// unit token: the last whitespace-separated token ("5 mm" -> "5", "mm"),
/// or — when there's no whitespace — the trailing alphabetic run
/// ("2.54mm" -> "2.54", "mm").
fn split_number_and_unit(raw_trim: &str) -> (&str, &str) {
    if let Some((head, tail)) = raw_trim.rsplit_once(char::is_whitespace) {
        if !tail.is_empty() {
            return (head.trim(), tail);
        }
    }
    let mut start = raw_trim.len();
    for (i, c) in raw_trim.char_indices().rev() {
        if c.is_alphabetic() {
            start = i;
        } else {
            break;
        }
    }
    (raw_trim[..start].trim(), &raw_trim[start..])
}

/// Parse a simple "<numerator>/<denominator>" fraction (e.g. "1/2") as an
/// f64. Returns `None` if either part fails to parse or the denominator is
/// zero, so the caller can fall back to a single `InvalidDimension` error.
fn parse_fraction(s: &str) -> Option<f64> {
    let (num_str, den_str) = s.split_once('/')?;
    let num: f64 = num_str.trim().parse().ok()?;
    let den: f64 = den_str.trim().parse().ok()?;
    if den == 0.0 {
        return None;
    }
    Some(num / den)
}

impl Database {
    pub fn add_dimension(
        &mut self,
        part_id: &PartId,
        draft: &DimensionDraft,
    ) -> Result<DimensionRecord, DbError> {
        if self.get_part(part_id)?.is_none() {
            return Err(DbError::PartNotFound);
        }
        let raw_trim = draft.raw_value.trim();
        let (number_str, unit_str) = split_number_and_unit(raw_trim);
        if unit_str.is_empty() {
            return Err(DbError::InvalidDimension(
                "dimension values need a unit, e.g. '5 mm'".into(),
            ));
        }

        let (kind, parsed) = match parse_with_kind(raw_trim, UnitKind::Length) {
            Ok(p) => (UnitKind::Length, p),
            Err(len_err) => match parse_with_kind(raw_trim, UnitKind::Mass) {
                Ok(p) => (UnitKind::Mass, p),
                Err(mass_err) => {
                    return Err(DbError::InvalidDimension(format!(
                        "'{raw_trim}' is not a valid length or mass (length: {len_err}; mass: {mass_err})"
                    )));
                }
            },
        };

        let value_num: f64 = match number_str.parse::<f64>() {
            Ok(v) => v,
            Err(_) => parse_fraction(number_str).ok_or_else(|| {
                DbError::InvalidDimension(format!(
                    "could not read the numeric portion of '{raw_trim}'"
                ))
            })?,
        };
        let display_unit = unit_str.to_string();

        let normalized_value = match kind {
            UnitKind::Length => parsed.to_f64() * 1000.0, // meters -> millimeters
            UnitKind::Mass => parsed.to_f64(),            // already grams
            _ => unreachable!("only Length and Mass are attempted above"),
        };

        let id = inventory_core::id::new_id();
        self.raw_conn().execute(
            "INSERT INTO dimensions (id, part_id, dim_group, name, value_num, display_unit,
                                     normalized_value, source, notes, measured_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                part_id.as_str(),
                draft.group.as_sql(),
                draft.name,
                value_num,
                display_unit,
                normalized_value,
                draft.source.as_sql(),
                draft.notes,
                draft.measured_date,
            ],
        )?;

        Ok(DimensionRecord {
            id,
            part_id: part_id.clone(),
            group: draft.group,
            name: draft.name.clone(),
            value_num,
            display_unit,
            normalized_value,
            source: draft.source,
            notes: draft.notes.clone(),
            measured_date: draft.measured_date.clone(),
        })
    }

    pub fn list_dimensions(&self, part_id: &PartId) -> Result<Vec<DimensionRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT id, part_id, dim_group, name, value_num, display_unit, normalized_value,
                    source, notes, measured_date
             FROM dimensions WHERE part_id = ?1 ORDER BY created_at, id",
        )?;
        let mut rows = stmt.query([part_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_dimension(row)?);
        }
        Ok(out)
    }

    pub fn remove_dimension(&mut self, id: &str) -> Result<(), DbError> {
        let n = self
            .raw_conn()
            .execute("DELETE FROM dimensions WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(DbError::DimensionNotFound);
        }
        Ok(())
    }
}

fn row_to_dimension(row: &rusqlite::Row<'_>) -> Result<DimensionRecord, DbError> {
    let group_raw: String = row.get(2)?;
    let source_raw: String = row.get(7)?;
    Ok(DimensionRecord {
        id: row.get(0)?,
        part_id: PartId::from_string(row.get(1)?)
            .map_err(|_| DbError::Corrupt("bad part id".into()))?,
        group: DimensionGroup::from_sql(&group_raw)
            .ok_or_else(|| DbError::Corrupt(format!("unknown dim_group '{group_raw}'")))?,
        name: row.get(3)?,
        value_num: row.get(4)?,
        display_unit: row.get(5)?,
        normalized_value: row.get(6)?,
        source: DimensionSource::from_sql(&source_raw)
            .ok_or_else(|| DbError::Corrupt(format!("unknown source '{source_raw}'")))?,
        notes: row.get(8)?,
        measured_date: row.get(9)?,
    })
}
