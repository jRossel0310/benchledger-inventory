//! Parts repository: parts, manufacturer variants, and supplier listings.

use inventory_core::ids::{CategoryId, ListingId, PartId, VariantId};
use inventory_core::quantity::{Quantity, QuantityUnit};

use crate::{Database, DbError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PartDraft {
    pub display_name: String,
    pub category_id: CategoryId,
    pub description: String,
    pub bin_label: Option<String>,
    pub usage_behavior: String, // 'usually_consumed' | 'usually_checked_out' | 'ask' (typed enum arrives in 2b with form layer)
    pub quantity_unit: QuantityUnit,
    pub low_stock_threshold: Option<Quantity>,
    pub public_notes: String,
    pub private_notes: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PartRecord {
    pub id: PartId,
    pub display_name: String,
    pub category_id: CategoryId,
    pub description: String,
    pub bin_label: Option<String>,
    pub usage_behavior: String,
    pub quantity_unit: QuantityUnit,
    pub low_stock_threshold: Option<Quantity>,
    pub public_notes: String,
    pub private_notes: String,
    pub metadata_complete: bool,
    pub archived: bool,
    pub created_at: String,
    pub modified_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct VariantDraft {
    pub manufacturer: String,
    pub mpn: String,
    pub description: String,
    pub package: Option<String>,
    pub datasheet_url: Option<String>,
    pub product_url: Option<String>,
    pub lifecycle: Option<String>,
    pub notes: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct VariantRecord {
    pub id: VariantId,
    pub part_id: PartId,
    pub manufacturer: String,
    pub mpn: String,
    pub description: String,
    pub package: Option<String>,
    pub datasheet_url: Option<String>,
    pub product_url: Option<String>,
    pub lifecycle: Option<String>,
    pub is_preferred: bool,
    pub notes: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ListingDraft {
    pub supplier: String,
    pub supplier_sku: String,
    pub product_url: Option<String>,
    pub packaging: Option<String>,
    pub typical_order: Option<Quantity>,
    pub last_unit_price_micros: Option<i64>,
    pub currency: Option<String>,
    pub last_purchase_date: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ListingRecord {
    pub id: ListingId,
    pub variant_id: VariantId,
    pub supplier: String,
    pub supplier_sku: String,
    pub product_url: Option<String>,
    pub packaging: Option<String>,
    pub typical_order: Option<Quantity>,
    pub last_unit_price_micros: Option<i64>,
    pub currency: Option<String>,
    pub last_purchase_date: Option<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PartStockRow {
    pub available: Quantity,
    pub reserved: Quantity,
    pub checked_out: Quantity,
    pub lifetime_received: Quantity,
    pub lifetime_consumed: Quantity,
}

impl PartStockRow {
    pub fn current_stock(&self) -> Quantity {
        self.available
            .checked_add(self.reserved)
            .and_then(|s| s.checked_add(self.checked_out))
            .expect("stock sums cannot overflow: each column is bounded by i64 CHECKs")
    }
}

fn opt_milli(q: &Option<Quantity>) -> Option<i64> {
    q.map(|v| v.as_milli())
}

impl Database {
    pub fn create_part(&mut self, draft: &PartDraft) -> Result<PartRecord, DbError> {
        let id = PartId::new();
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "INSERT INTO parts (id, display_name, category_id, description, bin_label,
                                usage_behavior, quantity_unit, low_stock_threshold_milli,
                                public_notes, private_notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id.as_str(),
                draft.display_name,
                draft.category_id.as_str(),
                draft.description,
                draft.bin_label,
                draft.usage_behavior,
                draft.quantity_unit.as_sql(),
                opt_milli(&draft.low_stock_threshold),
                draft.public_notes,
                draft.private_notes,
            ],
        )?;
        tx.execute(
            "INSERT INTO part_stock (part_id) VALUES (?1)",
            [id.as_str()],
        )?;
        tx.commit()?;
        self.refresh_search_text(&id)?;
        self.get_part(&id)?.ok_or(DbError::PartNotFound)
    }

    pub fn get_part(&self, id: &PartId) -> Result<Option<PartRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT id, display_name, category_id, description, bin_label, usage_behavior,
                    quantity_unit, low_stock_threshold_milli, public_notes, private_notes,
                    metadata_complete, archived, created_at, modified_at
             FROM parts WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id.as_str()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_part(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_parts(&self, include_archived: bool) -> Result<Vec<PartRecord>, DbError> {
        let sql = if include_archived {
            "SELECT id, display_name, category_id, description, bin_label, usage_behavior,
                    quantity_unit, low_stock_threshold_milli, public_notes, private_notes,
                    metadata_complete, archived, created_at, modified_at
             FROM parts ORDER BY display_name"
        } else {
            "SELECT id, display_name, category_id, description, bin_label, usage_behavior,
                    quantity_unit, low_stock_threshold_milli, public_notes, private_notes,
                    metadata_complete, archived, created_at, modified_at
             FROM parts WHERE archived = 0 ORDER BY display_name"
        };
        let mut stmt = self.raw_conn().prepare(sql)?;
        let mut out = Vec::new();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            out.push(row_to_part(row)?);
        }
        Ok(out)
    }

    pub fn update_part(&mut self, record: &PartRecord) -> Result<(), DbError> {
        let stored = self.get_part(&record.id)?.ok_or(DbError::PartNotFound)?;
        if stored.quantity_unit != record.quantity_unit {
            let txn_count: i64 = self.raw_conn().query_row(
                "SELECT COUNT(*) FROM transactions WHERE part_id = ?1",
                [record.id.as_str()],
                |r| r.get(0),
            )?;
            if txn_count > 0 {
                return Err(DbError::UnitChangeBlocked);
            }
        }
        let n = self.raw_conn().execute(
            "UPDATE parts SET display_name = ?2, category_id = ?3, description = ?4,
                              bin_label = ?5, usage_behavior = ?6, quantity_unit = ?7,
                              low_stock_threshold_milli = ?8, public_notes = ?9,
                              private_notes = ?10, metadata_complete = ?11,
                              modified_at = datetime('now')
             WHERE id = ?1",
            rusqlite::params![
                record.id.as_str(),
                record.display_name,
                record.category_id.as_str(),
                record.description,
                record.bin_label,
                record.usage_behavior,
                record.quantity_unit.as_sql(),
                opt_milli(&record.low_stock_threshold),
                record.public_notes,
                record.private_notes,
                record.metadata_complete,
            ],
        )?;
        if n == 0 {
            return Err(DbError::PartNotFound);
        }
        self.refresh_search_text(&record.id)?;
        Ok(())
    }

    pub fn set_part_archived(&mut self, id: &PartId, archived: bool) -> Result<(), DbError> {
        let n = self.raw_conn().execute(
            "UPDATE parts SET archived = ?2, modified_at = datetime('now') WHERE id = ?1",
            rusqlite::params![id.as_str(), archived],
        )?;
        if n == 0 {
            return Err(DbError::PartNotFound);
        }
        self.refresh_search_text(id)?;
        Ok(())
    }

    pub fn add_variant(
        &mut self,
        part_id: &PartId,
        draft: &VariantDraft,
    ) -> Result<VariantRecord, DbError> {
        if self.get_part(part_id)?.is_none() {
            return Err(DbError::PartNotFound);
        }
        let id = VariantId::new();
        self.raw_conn().execute(
            "INSERT INTO manufacturer_variants (id, part_id, manufacturer, mpn, description,
                                                package, datasheet_url, product_url, lifecycle, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id.as_str(),
                part_id.as_str(),
                draft.manufacturer,
                draft.mpn,
                draft.description,
                draft.package,
                draft.datasheet_url,
                draft.product_url,
                draft.lifecycle,
                draft.notes,
            ],
        )?;
        self.refresh_search_text(part_id)?;
        Ok(VariantRecord {
            id,
            part_id: part_id.clone(),
            manufacturer: draft.manufacturer.clone(),
            mpn: draft.mpn.clone(),
            description: draft.description.clone(),
            package: draft.package.clone(),
            datasheet_url: draft.datasheet_url.clone(),
            product_url: draft.product_url.clone(),
            lifecycle: draft.lifecycle.clone(),
            is_preferred: false,
            notes: draft.notes.clone(),
        })
    }

    pub fn set_preferred_variant(
        &mut self,
        part_id: &PartId,
        variant_id: &VariantId,
    ) -> Result<(), DbError> {
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "UPDATE manufacturer_variants SET is_preferred = 0 WHERE part_id = ?1 AND is_preferred = 1",
            [part_id.as_str()],
        )?;
        let n = tx.execute(
            "UPDATE manufacturer_variants SET is_preferred = 1 WHERE id = ?1 AND part_id = ?2",
            rusqlite::params![variant_id.as_str(), part_id.as_str()],
        )?;
        if n == 0 {
            return Err(DbError::VariantNotFound);
        }
        tx.commit()?;
        Ok(())
    }

    pub fn add_supplier_listing(
        &mut self,
        variant_id: &VariantId,
        draft: &ListingDraft,
    ) -> Result<ListingRecord, DbError> {
        let id = ListingId::new();
        self.raw_conn().execute(
            "INSERT INTO supplier_listings (id, variant_id, supplier, supplier_sku, product_url,
                                            packaging, typical_order_milli, last_unit_price_micros,
                                            currency, last_purchase_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id.as_str(),
                variant_id.as_str(),
                draft.supplier,
                draft.supplier_sku,
                draft.product_url,
                draft.packaging,
                opt_milli(&draft.typical_order),
                draft.last_unit_price_micros,
                draft.currency,
                draft.last_purchase_date,
            ],
        )?;
        let part_id: String = self.raw_conn().query_row(
            "SELECT part_id FROM manufacturer_variants WHERE id = ?1",
            [variant_id.as_str()],
            |r| r.get(0),
        )?;
        let part_id =
            PartId::from_string(part_id).map_err(|_| DbError::Corrupt("bad part id".into()))?;
        self.refresh_search_text(&part_id)?;
        Ok(ListingRecord {
            id,
            variant_id: variant_id.clone(),
            supplier: draft.supplier.clone(),
            supplier_sku: draft.supplier_sku.clone(),
            product_url: draft.product_url.clone(),
            packaging: draft.packaging.clone(),
            typical_order: draft.typical_order,
            last_unit_price_micros: draft.last_unit_price_micros,
            currency: draft.currency.clone(),
            last_purchase_date: draft.last_purchase_date.clone(),
        })
    }

    /// A part's manufacturer variants, preferred first (`is_preferred DESC`),
    /// then by `mpn` — so the part-detail view can render the preferred
    /// sourcing option at the top without a separate query.
    pub fn list_variants(&self, part_id: &PartId) -> Result<Vec<VariantRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT id, part_id, manufacturer, mpn, description, package, datasheet_url,
                    product_url, lifecycle, is_preferred, notes
             FROM manufacturer_variants
             WHERE part_id = ?1
             ORDER BY is_preferred DESC, mpn",
        )?;
        let mut out = Vec::new();
        let mut rows = stmt.query([part_id.as_str()])?;
        while let Some(row) = rows.next()? {
            out.push(row_to_variant(row)?);
        }
        Ok(out)
    }

    /// A variant's supplier listings, ordered by supplier then SKU.
    /// `typical_order` is reconstructed against the owning part's
    /// `quantity_unit` (joined through the variant), the same unit-aware
    /// path `get_stock` uses, rather than the unit-blind `TryFrom<i64>`.
    pub fn list_supplier_listings(
        &self,
        variant_id: &VariantId,
    ) -> Result<Vec<ListingRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT sl.id, sl.variant_id, sl.supplier, sl.supplier_sku, sl.product_url,
                    sl.packaging, sl.typical_order_milli, sl.last_unit_price_micros,
                    sl.currency, sl.last_purchase_date, p.quantity_unit
             FROM supplier_listings sl
             JOIN manufacturer_variants mv ON mv.id = sl.variant_id
             JOIN parts p ON p.id = mv.part_id
             WHERE sl.variant_id = ?1
             ORDER BY sl.supplier, sl.supplier_sku",
        )?;
        let mut out = Vec::new();
        let mut rows = stmt.query([variant_id.as_str()])?;
        while let Some(row) = rows.next()? {
            out.push(row_to_listing(row)?);
        }
        Ok(out)
    }

    /// A part's tags, alphabetically ordered.
    pub fn get_tags(&self, part_id: &PartId) -> Result<Vec<String>, DbError> {
        let mut stmt = self
            .raw_conn()
            .prepare("SELECT tag FROM part_tags WHERE part_id = ?1 ORDER BY tag")?;
        let mut out = Vec::new();
        let mut rows = stmt.query([part_id.as_str()])?;
        while let Some(row) = rows.next()? {
            out.push(row.get(0)?);
        }
        Ok(out)
    }

    pub fn get_stock(&self, id: &PartId) -> Result<PartStockRow, DbError> {
        let part = self.get_part(id)?.ok_or(DbError::PartNotFound)?;
        let unit = part.quantity_unit;
        self.raw_conn()
            .query_row(
                "SELECT available_milli, reserved_milli, checked_out_milli,
                        lifetime_received_milli, lifetime_consumed_milli
                 FROM part_stock WHERE part_id = ?1",
                [id.as_str()],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .map_err(DbError::from)
            .and_then(|(a, rsv, c, lr, lc)| {
                Ok(PartStockRow {
                    available: Quantity::from_milli(a, unit)?,
                    reserved: Quantity::from_milli(rsv, unit)?,
                    checked_out: Quantity::from_milli(c, unit)?,
                    lifetime_received: Quantity::from_milli(lr, unit)?,
                    lifetime_consumed: Quantity::from_milli(lc, unit)?,
                })
            })
    }
}

fn row_to_variant(row: &rusqlite::Row<'_>) -> Result<VariantRecord, DbError> {
    Ok(VariantRecord {
        id: VariantId::from_string(row.get(0)?)
            .map_err(|_| DbError::Corrupt("bad variant id".into()))?,
        part_id: PartId::from_string(row.get(1)?)
            .map_err(|_| DbError::Corrupt("bad part id".into()))?,
        manufacturer: row.get(2)?,
        mpn: row.get(3)?,
        description: row.get(4)?,
        package: row.get(5)?,
        datasheet_url: row.get(6)?,
        product_url: row.get(7)?,
        lifecycle: row.get(8)?,
        is_preferred: row.get(9)?,
        notes: row.get(10)?,
    })
}

fn row_to_listing(row: &rusqlite::Row<'_>) -> Result<ListingRecord, DbError> {
    let quantity_unit_raw: String = row.get(10)?;
    let quantity_unit = QuantityUnit::from_sql(&quantity_unit_raw)
        .ok_or_else(|| DbError::Corrupt(format!("unknown quantity_unit '{quantity_unit_raw}'")))?;
    let typical_order_milli: Option<i64> = row.get(6)?;
    let typical_order = typical_order_milli
        .map(|m| Quantity::from_milli(m, quantity_unit))
        .transpose()?;
    Ok(ListingRecord {
        id: ListingId::from_string(row.get(0)?)
            .map_err(|_| DbError::Corrupt("bad listing id".into()))?,
        variant_id: VariantId::from_string(row.get(1)?)
            .map_err(|_| DbError::Corrupt("bad variant id".into()))?,
        supplier: row.get(2)?,
        supplier_sku: row.get(3)?,
        product_url: row.get(4)?,
        packaging: row.get(5)?,
        typical_order,
        last_unit_price_micros: row.get(7)?,
        currency: row.get(8)?,
        last_purchase_date: row.get(9)?,
    })
}

fn row_to_part(row: &rusqlite::Row<'_>) -> Result<PartRecord, DbError> {
    let quantity_unit_raw: String = row.get(6)?;
    let quantity_unit = QuantityUnit::from_sql(&quantity_unit_raw)
        .ok_or_else(|| DbError::Corrupt(format!("unknown quantity_unit '{quantity_unit_raw}'")))?;
    let threshold_milli: Option<i64> = row.get(7)?;
    let low_stock_threshold = threshold_milli
        .map(|m| Quantity::from_milli(m, quantity_unit))
        .transpose()?;
    Ok(PartRecord {
        id: PartId::from_string(row.get(0)?).map_err(|_| DbError::Corrupt("bad part id".into()))?,
        display_name: row.get(1)?,
        category_id: CategoryId::from_string(row.get(2)?)
            .map_err(|_| DbError::Corrupt("bad category id".into()))?,
        description: row.get(3)?,
        bin_label: row.get(4)?,
        usage_behavior: row.get(5)?,
        quantity_unit,
        low_stock_threshold,
        public_notes: row.get(8)?,
        private_notes: row.get(9)?,
        metadata_complete: row.get(10)?,
        archived: row.get(11)?,
        created_at: row.get(12)?,
        modified_at: row.get(13)?,
    })
}
