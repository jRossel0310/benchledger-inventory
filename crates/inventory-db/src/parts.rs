//! Parts repository: parts, manufacturer variants, and supplier listings.

use inventory_core::ids::{CategoryId, ListingId, PartId, VariantId};
use inventory_core::quantity::{Quantity, QuantityUnit};

use crate::{Database, DbError};

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Copy)]
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
        tx.execute("INSERT INTO part_stock (part_id) VALUES (?1)", [id.as_str()])?;
        tx.commit()?;
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
        Ok(())
    }

    pub fn add_variant(&mut self, part_id: &PartId, draft: &VariantDraft) -> Result<VariantRecord, DbError> {
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

    pub fn set_preferred_variant(&mut self, part_id: &PartId, variant_id: &VariantId) -> Result<(), DbError> {
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
            return Err(DbError::PartNotFound);
        }
        tx.commit()?;
        Ok(())
    }

    pub fn add_supplier_listing(&mut self, variant_id: &VariantId, draft: &ListingDraft) -> Result<ListingRecord, DbError> {
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
