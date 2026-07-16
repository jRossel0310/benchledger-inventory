//! Category and custom attribute management: create/list/duplicate
//! categories, define custom attributes, and attach/reorder/hide them per
//! category. Built-in seed rows (Task 6's `seed.rs`) populate the starting
//! set; everything here is the user-facing mutation layer on top of it.

use inventory_core::ids::CategoryId;
use inventory_core::units::UnitKind;

use crate::{Database, DbError};

/// `data_type` values accepted by the `attribute_defs.data_type` CHECK
/// constraint (migration 0003). Kept in sync with that list.
const DATA_TYPES: [&str; 8] = [
    "text",
    "number",
    "number_unit",
    "boolean",
    "choice",
    "multi_choice",
    "range",
    "url",
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CategoryRecord {
    pub id: CategoryId,
    pub name: String,
    pub group_name: String,
    pub built_in: bool,
}

impl Database {
    pub fn list_categories(&self) -> Result<Vec<CategoryRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT id, name, group_name, built_in FROM categories ORDER BY group_name, name",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_category(row)?);
        }
        Ok(out)
    }

    pub fn create_category(
        &mut self,
        name: &str,
        group_name: &str,
    ) -> Result<CategoryRecord, DbError> {
        let id = CategoryId::new();
        self.raw_conn()
            .execute(
                "INSERT INTO categories (id, name, group_name, built_in) VALUES (?1, ?2, ?3, 0)",
                rusqlite::params![id.as_str(), name, group_name],
            )
            .map_err(map_unique_to_category_name_taken)?;
        Ok(CategoryRecord {
            id,
            name: name.to_string(),
            group_name: group_name.to_string(),
            built_in: false,
        })
    }

    /// Copy `source` into a new custom category `new_name`, including its
    /// `category_attributes` links (display order and hidden flag), in a
    /// single transaction so the copy is never left half-populated.
    pub fn duplicate_category(
        &mut self,
        source: &CategoryId,
        new_name: &str,
    ) -> Result<CategoryRecord, DbError> {
        let tx = self.conn_mut().transaction()?;
        let group_name: String = tx
            .query_row(
                "SELECT group_name FROM categories WHERE id = ?1",
                [source.as_str()],
                |r| r.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DbError::CategoryNotFound,
                other => DbError::Sqlite(other),
            })?;
        let new_id = CategoryId::new();
        tx.execute(
            "INSERT INTO categories (id, name, group_name, built_in) VALUES (?1, ?2, ?3, 0)",
            rusqlite::params![new_id.as_str(), new_name, group_name],
        )
        .map_err(map_unique_to_category_name_taken)?;
        tx.execute(
            "INSERT INTO category_attributes (category_id, attribute_id, display_order, hidden)
             SELECT ?1, attribute_id, display_order, hidden
             FROM category_attributes WHERE category_id = ?2",
            rusqlite::params![new_id.as_str(), source.as_str()],
        )?;
        tx.commit()?;
        Ok(CategoryRecord {
            id: new_id,
            name: new_name.to_string(),
            group_name,
            built_in: false,
        })
    }

    /// Define a new custom (non-seed) attribute. Validates `data_type`
    /// against the CHECK-constraint list and `unit_kind` (when present) via
    /// `UnitKind::from_sql`, both before touching the database. Returns the
    /// new attribute's id.
    pub fn create_custom_attribute(
        &mut self,
        key: &str,
        label: &str,
        data_type: &str,
        unit_kind: Option<&str>,
        identity: bool,
    ) -> Result<String, DbError> {
        let invalid = |reason: String| DbError::InvalidAttributeValue {
            key: key.to_string(),
            reason,
        };
        if !DATA_TYPES.contains(&data_type) {
            return Err(invalid(format!("unknown data type '{data_type}'")));
        }
        let requires_unit_kind = matches!(data_type, "number_unit" | "range");
        if requires_unit_kind && unit_kind.is_none() {
            return Err(invalid("data type requires a unit kind".to_string()));
        }
        if !requires_unit_kind && unit_kind.is_some() {
            return Err(invalid("data type does not take a unit kind".to_string()));
        }
        let kind = match unit_kind {
            Some(u) => Some(
                UnitKind::from_sql(u).ok_or_else(|| invalid(format!("unknown unit kind '{u}'")))?,
            ),
            None => None,
        };
        let canonical = kind.map(|k| k.canonical_unit());
        let id = inventory_core::id::new_id();
        self.raw_conn()
            .execute(
                "INSERT INTO attribute_defs (id, key, label, data_type, unit_kind, canonical_unit, identity, built_in)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
                rusqlite::params![id, key, label, data_type, unit_kind, canonical, identity],
            )
            .map_err(map_unique_to_attribute_key_taken)?;
        Ok(id)
    }

    /// Attach an attribute to a category at a given display order (hidden
    /// defaults to visible for a fresh link; an existing link's order is
    /// updated in place).
    pub fn attach_attribute(
        &mut self,
        category: &CategoryId,
        attribute_key: &str,
        display_order: i64,
    ) -> Result<(), DbError> {
        let attribute_id = self.resolve_attribute_id(attribute_key)?;
        self.raw_conn().execute(
            "INSERT INTO category_attributes (category_id, attribute_id, display_order, hidden)
             VALUES (?1, ?2, ?3, 0)
             ON CONFLICT(category_id, attribute_id) DO UPDATE SET display_order = excluded.display_order",
            rusqlite::params![category.as_str(), attribute_id, display_order],
        )?;
        Ok(())
    }

    /// Change display order for an already-attached (or not-yet-attached)
    /// attribute. Same upsert as `attach_attribute`; kept as a separate
    /// method for call-site clarity.
    pub fn reorder_attribute(
        &mut self,
        category: &CategoryId,
        attribute_key: &str,
        display_order: i64,
    ) -> Result<(), DbError> {
        let attribute_id = self.resolve_attribute_id(attribute_key)?;
        self.raw_conn().execute(
            "INSERT INTO category_attributes (category_id, attribute_id, display_order, hidden)
             VALUES (?1, ?2, ?3, 0)
             ON CONFLICT(category_id, attribute_id) DO UPDATE SET display_order = excluded.display_order",
            rusqlite::params![category.as_str(), attribute_id, display_order],
        )?;
        Ok(())
    }

    pub fn set_attribute_hidden(
        &mut self,
        category: &CategoryId,
        attribute_key: &str,
        hidden: bool,
    ) -> Result<(), DbError> {
        let attribute_id = self.resolve_attribute_id(attribute_key)?;
        self.raw_conn().execute(
            "INSERT INTO category_attributes (category_id, attribute_id, display_order, hidden)
             VALUES (?1, ?2, 0, ?3)
             ON CONFLICT(category_id, attribute_id) DO UPDATE SET hidden = excluded.hidden",
            rusqlite::params![category.as_str(), attribute_id, hidden],
        )?;
        Ok(())
    }

    /// (key, label, display_order, hidden) for every attribute linked to
    /// `category`, ordered by display order then key. Hidden links are
    /// included — callers filter for display purposes.
    pub fn category_attributes(
        &self,
        category: &CategoryId,
    ) -> Result<Vec<(String, String, i64, bool)>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT a.key, a.label, ca.display_order, ca.hidden
             FROM category_attributes ca JOIN attribute_defs a ON a.id = ca.attribute_id
             WHERE ca.category_id = ?1
             ORDER BY ca.display_order, a.key",
        )?;
        let mut rows = stmt.query([category.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?));
        }
        Ok(out)
    }

    fn resolve_attribute_id(&self, key: &str) -> Result<String, DbError> {
        self.raw_conn()
            .query_row("SELECT id FROM attribute_defs WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DbError::AttributeNotFound(key.to_string()),
                other => DbError::Sqlite(other),
            })
    }
}

fn row_to_category(row: &rusqlite::Row<'_>) -> Result<CategoryRecord, DbError> {
    Ok(CategoryRecord {
        id: CategoryId::from_string(row.get(0)?)
            .map_err(|_| DbError::Corrupt("bad category id".into()))?,
        name: row.get(1)?,
        group_name: row.get(2)?,
        built_in: row.get(3)?,
    })
}

/// `categories.name` is UNIQUE (extended code 2067); translate a violation
/// on that column into the typed error instead of a raw SQLite one. Mirrors
/// `ledger::map_unique_to_already_reversed`.
fn map_unique_to_category_name_taken(e: rusqlite::Error) -> DbError {
    if let rusqlite::Error::SqliteFailure(err, _) = &e {
        if err.extended_code == 2067 {
            return DbError::CategoryNameTaken;
        }
    }
    DbError::Sqlite(e)
}

/// `attribute_defs.key` is UNIQUE (extended code 2067); translate a
/// violation on that column into the typed error.
fn map_unique_to_attribute_key_taken(e: rusqlite::Error) -> DbError {
    if let rusqlite::Error::SqliteFailure(err, _) = &e {
        if err.extended_code == 2067 {
            return DbError::AttributeKeyTaken;
        }
    }
    DbError::Sqlite(e)
}
