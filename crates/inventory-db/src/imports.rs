//! Import repository — Phase 5a Task 4 (spec §10 Upload -> Extract).
//!
//! Persists a parsed supplier invoice (`inventory_import::ParsedInvoice`) as
//! three rows/row-sets: one `imports` row (order metadata), one
//! `import_files` row pointing at the original bytes preserved verbatim in
//! the existing content-addressed attachments store (Phase 3,
//! `attachment_store.rs`), and one `import_lines` row per parsed line item,
//! each keeping its full `raw_json` so review/debugging can always see
//! exactly what the parser saw. **Nothing here touches `parts`,
//! `part_stock`, or `transactions`** — matching, enrichment, review, and the
//! atomic commit into inventory are 5b/5c; this module only records what was
//! parsed.
//!
//! ## Reusing the attachment store, not a parallel one
//!
//! `store_attachment` (Phase 3 Task 10) is deliberately directory-agnostic:
//! every method there takes `attachments_dir` explicitly rather than
//! `Database` owning the filesystem layout (see that module's doc comment).
//! `store_import` follows the same contract and takes `attachments_dir` too,
//! for the same reason — there is no separate hash/dedup path here.
//!
//! ## Why the file write happens outside the SQL transaction
//!
//! `store_attachment` is itself a small atomic, idempotent unit (write the
//! file if absent, `INSERT OR IGNORE` the metadata row) that every other
//! caller in this codebase (`attach_to_part`, `set_dimension_attachment`,
//! the `add_attachment` Tauri command) also invokes standalone, never nested
//! inside a bigger transaction. `store_import` follows that same precedent:
//! the original bytes are stored first (an interrupted `store_import` after
//! this step leaves, at worst, a harmless orphaned-but-deduplicated blob —
//! never a partial `imports`/`import_lines` row), then the `imports` +
//! `import_files` + `import_lines` rows are inserted together in ONE SQL
//! transaction so that row-set is all-or-nothing.
//!
//! ## Duplicate detection without blocking
//!
//! `find_duplicate_imports` is a pure read — it never blocks `store_import`.
//! A caller (5b/5d) computes the would-be content hash of a file with
//! [`hash_bytes`] (which delegates to the attachment store's own SHA-256
//! function, so the two hashes can never drift) and can check for duplicates
//! *before* calling `store_import`, or pass the hash `store_import` returned
//! afterward — either order works since storing is idempotent.

use std::path::Path;

use inventory_core::ids::{ImportFileId, ImportId, ImportLineId};
use inventory_import::{ParsedInvoice, SourceFormat};

use crate::attachment_store::AttachmentKind;
use crate::{Database, DbError};

/// A persisted import's order-level metadata plus a line count. Mirrors the
/// `imports` table; money stays exact integer micros (never a float).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ImportRecord {
    pub id: ImportId,
    pub supplier: String,
    pub order_number: Option<String>,
    pub invoice_number: Option<String>,
    pub shipment_number: Option<String>,
    pub order_date: Option<String>,
    pub currency: String,
    pub source_format: String,
    pub status: String,
    pub subtotal_micros: Option<i64>,
    pub shipping_micros: Option<i64>,
    pub tax_micros: Option<i64>,
    pub tariff_micros: Option<i64>,
    pub total_micros: Option<i64>,
    pub web_order_id: Option<String>,
    pub line_count: usize,
    pub created_at: String,
}

/// A persisted import line. Mirrors `import_lines`; `raw_json` is the
/// serialized form of the parser's original `ParsedLine.raw` value, kept
/// verbatim for review/debugging regardless of how well the typed fields
/// above were filled in.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ImportLineRecord {
    pub id: ImportLineId,
    pub import_id: ImportId,
    pub line_number: Option<i64>,
    pub supplier_sku: Option<String>,
    pub mpn: Option<String>,
    pub manufacturer: Option<String>,
    pub description: Option<String>,
    pub packaging: Option<String>,
    pub customer_reference: Option<String>,
    pub ordered_milli: Option<i64>,
    pub shipped_milli: Option<i64>,
    pub backordered_milli: Option<i64>,
    pub unit_price_micros: Option<i64>,
    pub extended_price_micros: Option<i64>,
    pub raw_json: String,
    pub line_kind: String,
    pub parse_confidence: f64,
    pub created_at: String,
}

/// SHA-256 hex digest of `bytes`, identical to what `store_attachment`
/// (and therefore `store_import`) will compute for those same bytes.
/// Exposed so a caller can compute a file's would-be content hash BEFORE
/// storing it — e.g. to call `find_duplicate_imports` ahead of persisting —
/// without duplicating the hashing logic (drift-proof by construction).
pub fn hash_bytes(bytes: &[u8]) -> String {
    crate::attachment_store::sha256_hex(bytes)
}

/// The extension to preserve the original bytes under: the filename's own
/// extension if it has one, else the parser's source format (`"pdf"`/`"csv"`/
/// `"xlsx"` — conveniently identical to real file extensions).
fn derive_ext(filename: &str, source_format: SourceFormat) -> String {
    match Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
    {
        Some(e) => e.to_ascii_lowercase(),
        None => source_format.as_db_str().to_string(),
    }
}

const IMPORT_COLUMNS: &str = "i.id, i.supplier, i.order_number, i.invoice_number, \
     i.shipment_number, i.order_date, i.currency, i.source_format, i.status, \
     i.subtotal_micros, i.shipping_micros, i.tax_micros, i.tariff_micros, i.total_micros, \
     i.web_order_id, i.created_at, \
     (SELECT COUNT(*) FROM import_lines l WHERE l.import_id = i.id) AS line_count";

fn row_to_import(row: &rusqlite::Row<'_>) -> Result<ImportRecord, DbError> {
    let line_count: i64 = row.get(16)?;
    Ok(ImportRecord {
        id: ImportId::from_string(row.get(0)?)
            .map_err(|_| DbError::Corrupt("bad import id".into()))?,
        supplier: row.get(1)?,
        order_number: row.get(2)?,
        invoice_number: row.get(3)?,
        shipment_number: row.get(4)?,
        order_date: row.get(5)?,
        currency: row.get(6)?,
        source_format: row.get(7)?,
        status: row.get(8)?,
        subtotal_micros: row.get(9)?,
        shipping_micros: row.get(10)?,
        tax_micros: row.get(11)?,
        tariff_micros: row.get(12)?,
        total_micros: row.get(13)?,
        web_order_id: row.get(14)?,
        created_at: row.get(15)?,
        line_count: line_count as usize,
    })
}

fn row_to_import_line(row: &rusqlite::Row<'_>) -> Result<ImportLineRecord, DbError> {
    Ok(ImportLineRecord {
        id: ImportLineId::from_string(row.get(0)?)
            .map_err(|_| DbError::Corrupt("bad import line id".into()))?,
        import_id: ImportId::from_string(row.get(1)?)
            .map_err(|_| DbError::Corrupt("bad import id".into()))?,
        line_number: row.get(2)?,
        supplier_sku: row.get(3)?,
        mpn: row.get(4)?,
        manufacturer: row.get(5)?,
        description: row.get(6)?,
        ordered_milli: row.get(7)?,
        shipped_milli: row.get(8)?,
        backordered_milli: row.get(9)?,
        unit_price_micros: row.get(10)?,
        extended_price_micros: row.get(11)?,
        packaging: row.get(12)?,
        customer_reference: row.get(13)?,
        raw_json: row.get(14)?,
        line_kind: row.get(15)?,
        parse_confidence: row.get(16)?,
        created_at: row.get(17)?,
    })
}

impl Database {
    /// Persist a fully parsed invoice: the original bytes (via the existing
    /// attachment store — see the module doc comment for why that step is
    /// not nested in the SQL transaction below), one `imports` row, one
    /// `import_files` row, and one `import_lines` row per `parsed.lines`.
    /// The `imports`/`import_files`/`import_lines` rows are inserted in ONE
    /// transaction, so that row-set is all-or-nothing. Money fields store
    /// `Money.micros` verbatim (no float ever involved); quantities store
    /// `Quantity::as_milli()` verbatim; a `None` field is a SQL `NULL`, never
    /// a fabricated value. Does not touch `parts`/`part_stock`/`transactions`.
    pub fn store_import(
        &mut self,
        attachments_dir: &Path,
        parsed: &ParsedInvoice,
        original: &[u8],
        filename: &str,
    ) -> Result<ImportRecord, DbError> {
        let ext = derive_ext(filename, parsed.source_format);
        let attachment = self.store_attachment(
            attachments_dir,
            original,
            Some(&ext),
            AttachmentKind::Invoice,
            Some(filename),
            "import",
        )?;

        let import_id = ImportId::new();
        let tx = self.conn_mut().transaction()?;

        tx.execute(
            "INSERT INTO imports (
                id, supplier, order_number, invoice_number, shipment_number, order_date,
                currency, subtotal_micros, shipping_micros, tax_micros, tariff_micros,
                total_micros, source_format, status, web_order_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'parsed', ?14)",
            rusqlite::params![
                import_id.as_str(),
                parsed.supplier,
                parsed.order.order_number,
                parsed.order.invoice_number,
                parsed.order.shipment_number,
                parsed.order.order_date,
                parsed.order.currency,
                parsed.order.subtotal.as_ref().map(|m| m.micros),
                parsed.order.shipping.as_ref().map(|m| m.micros),
                parsed.order.tax.as_ref().map(|m| m.micros),
                parsed.order.tariff.as_ref().map(|m| m.micros),
                parsed.order.total.as_ref().map(|m| m.micros),
                parsed.source_format.as_db_str(),
                parsed.order.web_order_id,
            ],
        )?;

        let file_id = ImportFileId::new();
        tx.execute(
            "INSERT INTO import_files (id, import_id, attachment_hash, original_filename, byte_size)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                file_id.as_str(),
                import_id.as_str(),
                attachment.content_hash,
                filename,
                attachment.size_bytes,
            ],
        )?;

        for line in &parsed.lines {
            let line_id = ImportLineId::new();
            let raw_json = serde_json::to_string(&line.raw)?;
            tx.execute(
                "INSERT INTO import_lines (
                    id, import_id, line_number, supplier_sku, mpn, manufacturer, description,
                    ordered_milli, shipped_milli, backordered_milli, unit_price_micros,
                    extended_price_micros, packaging, customer_reference, raw_json, line_kind,
                    parse_confidence
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                rusqlite::params![
                    line_id.as_str(),
                    import_id.as_str(),
                    line.line_number.map(|n| n as i64),
                    line.supplier_sku,
                    line.mpn,
                    line.manufacturer,
                    line.description,
                    line.ordered.map(|q| q.as_milli()),
                    line.shipped.map(|q| q.as_milli()),
                    line.backordered.map(|q| q.as_milli()),
                    line.unit_price.as_ref().map(|m| m.micros),
                    line.extended_price.as_ref().map(|m| m.micros),
                    line.packaging,
                    line.customer_reference,
                    raw_json,
                    line.kind.as_db_str(),
                    line.confidence as f64,
                ],
            )?;
        }

        tx.commit()?;

        self.get_import(&import_id)?.ok_or(DbError::ImportNotFound)
    }

    /// A single import by id, or `None` if unknown.
    pub fn get_import(&self, id: &ImportId) -> Result<Option<ImportRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(&format!(
            "SELECT {IMPORT_COLUMNS} FROM imports i WHERE i.id = ?1"
        ))?;
        let mut rows = stmt.query([id.as_str()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_import(row)?)),
            None => Ok(None),
        }
    }

    /// Every import, newest first (`created_at` descending, `rowid`
    /// descending as a tiebreak for imports created within the same second).
    pub fn list_imports(&self) -> Result<Vec<ImportRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(&format!(
            "SELECT {IMPORT_COLUMNS} FROM imports i ORDER BY i.created_at DESC, i.rowid DESC"
        ))?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_import(row)?);
        }
        Ok(out)
    }

    /// Every line of one import, ordered by `line_number` (nulls last), then
    /// insertion order (`rowid`) as a tiebreak.
    pub fn list_import_lines(
        &self,
        import_id: &ImportId,
    ) -> Result<Vec<ImportLineRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT id, import_id, line_number, supplier_sku, mpn, manufacturer, description,
                    ordered_milli, shipped_milli, backordered_milli, unit_price_micros,
                    extended_price_micros, packaging, customer_reference, raw_json, line_kind,
                    parse_confidence, created_at
             FROM import_lines
             WHERE import_id = ?1
             ORDER BY line_number IS NULL, line_number, rowid",
        )?;
        let mut rows = stmt.query([import_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_import_line(row)?);
        }
        Ok(out)
    }

    /// The §10 duplicate-import signal: every existing import whose stored
    /// original has the SAME content hash as `file_hash`, OR whose supplier
    /// matches `parsed.supplier` AND whose `order_number`/`invoice_number`/
    /// `shipment_number` matches the corresponding (non-null) field on
    /// `parsed.order`. A field that is `None` on `parsed.order` never
    /// matches anything (SQL `NULL = NULL` and `NULL = x` are both not-true),
    /// so only genuinely shared identifiers count. Newest first, like
    /// `list_imports`. This never blocks `store_import` — it's purely
    /// informational for the caller (5b/5d) to warn on; an explicit reimport
    /// is always allowed.
    pub fn find_duplicate_imports(
        &self,
        parsed: &ParsedInvoice,
        file_hash: &str,
    ) -> Result<Vec<ImportRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(&format!(
            "SELECT {IMPORT_COLUMNS}
             FROM imports i
             WHERE EXISTS (
                       SELECT 1 FROM import_files f
                       WHERE f.import_id = i.id AND f.attachment_hash = ?1
                   )
                OR (i.supplier = ?2 AND i.order_number = ?3)
                OR (i.supplier = ?2 AND i.invoice_number = ?4)
                OR (i.supplier = ?2 AND i.shipment_number = ?5)
             ORDER BY i.created_at DESC, i.rowid DESC"
        ))?;
        let mut rows = stmt.query(rusqlite::params![
            file_hash,
            parsed.supplier,
            parsed.order.order_number,
            parsed.order.invoice_number,
            parsed.order.shipment_number,
        ])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_import(row)?);
        }
        Ok(out)
    }
}
