//! Content-addressed attachment store (Phase 3 Task 10, spec §9).
//!
//! Blobs are stored on disk under the data dir's `attachments/` folder, named
//! by the SHA-256 hex digest of their bytes. Because the name is derived
//! purely from content, storing identical bytes twice yields exactly one file
//! and — via the `attachments` table's `content_hash` PRIMARY KEY — exactly
//! one metadata row (deduplication). SQLite holds only metadata; the bytes
//! themselves never cross into the database.
//!
//! The store is the one place that touches the `attachments/` directory, so
//! every method here takes the `attachments_dir` path explicitly (the Tauri
//! command supplies it from `AppState.layout.attachments`) rather than the
//! `Database` owning the filesystem layout. Links to parts
//! (`part_attachments`) and to a dimension's measurement photo
//! (`dimensions.attachment_id`) are pure metadata and need no directory.

use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

use inventory_core::ids::PartId;

use crate::{Database, DbError};

/// What an attachment is, mirroring the `attachments.kind` CHECK list. Kept as
/// a typed enum (rather than a free string) so the command surface and the
/// generated bindings carry a closed union, the same pattern `DimensionGroup`/
/// `DimensionSource` follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Invoice,
    Datasheet,
    Photo,
    MeasurementPhoto,
    Drawing,
    Cad,
    ProjectDoc,
    Other,
}

impl AttachmentKind {
    pub fn as_sql(&self) -> &'static str {
        match self {
            AttachmentKind::Invoice => "invoice",
            AttachmentKind::Datasheet => "datasheet",
            AttachmentKind::Photo => "photo",
            AttachmentKind::MeasurementPhoto => "measurement_photo",
            AttachmentKind::Drawing => "drawing",
            AttachmentKind::Cad => "cad",
            AttachmentKind::ProjectDoc => "project_doc",
            AttachmentKind::Other => "other",
        }
    }

    pub fn from_sql(s: &str) -> Option<Self> {
        Some(match s {
            "invoice" => AttachmentKind::Invoice,
            "datasheet" => AttachmentKind::Datasheet,
            "photo" => AttachmentKind::Photo,
            "measurement_photo" => AttachmentKind::MeasurementPhoto,
            "drawing" => AttachmentKind::Drawing,
            "cad" => AttachmentKind::Cad,
            "project_doc" => AttachmentKind::ProjectDoc,
            "other" => AttachmentKind::Other,
            _ => return None,
        })
    }
}

/// One row of attachment metadata (never the bytes themselves — those are read
/// on demand via [`Database::read_attachment`]). Returned by the store/link
/// commands so the UI can render a name/size/kind and fetch the blob by hash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AttachmentRef {
    pub content_hash: String,
    pub ext: Option<String>,
    pub size_bytes: i64,
    pub kind: AttachmentKind,
    pub original_name: Option<String>,
    pub source: String,
    pub created_at: String,
}

/// SHA-256 of `bytes` as a lowercase hex string (64 chars).
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        // Two lowercase hex digits per byte; `write!` to a String is infallible.
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// On-disk file name for a stored blob: `<hash>.<ext>` when an extension is
/// known, otherwise the bare `<hash>`. A blob's canonical extension is fixed by
/// its first writer (see [`Database::store_attachment`]) so the same content
/// always maps to the same file name — the property deduplication relies on.
fn attachment_filename(hash: &str, ext: Option<&str>) -> String {
    match ext {
        Some(e) if !e.is_empty() => format!("{hash}.{e}"),
        _ => hash.to_string(),
    }
}

/// Longest extension `sanitize_ext` accepts. Comfortably covers every
/// real-world extension this app deals with (png, jpeg, step, xlsx, ...)
/// while keeping the whitelist tight.
const MAX_EXT_LEN: usize = 16;

/// Sanitize and validate a caller-supplied extension before it becomes part
/// of an on-disk file name (`<hash>.<ext>`, joined onto the attachments
/// dir). Trims whitespace, strips a single leading dot, and lowercases; the
/// result must then be either empty (meaning "no extension") or match
/// `[a-z0-9]{1,16}`.
///
/// This is the only check standing between the `add_attachment` Tauri
/// command and the filesystem path `ext` feeds into, so it must be strict
/// enough to make traversal impossible regardless of caller: a whitelist of
/// `[a-z0-9]` cannot encode `/`, `\`, `..`, or any other path metacharacter,
/// so there is no sanitized value that can ever escape the attachments
/// directory.
fn sanitize_ext(ext: &str) -> Result<Option<String>, DbError> {
    let trimmed = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > MAX_EXT_LEN || !trimmed.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(DbError::InvalidAttachment(format!(
            "extension must be 1-{MAX_EXT_LEN} alphanumeric characters, got '{trimmed}'"
        )));
    }
    Ok(Some(trimmed))
}

fn row_to_attachment(row: &rusqlite::Row<'_>) -> Result<AttachmentRef, DbError> {
    let kind_raw: String = row.get(3)?;
    Ok(AttachmentRef {
        content_hash: row.get(0)?,
        ext: row.get(1)?,
        size_bytes: row.get(2)?,
        kind: AttachmentKind::from_sql(&kind_raw)
            .ok_or_else(|| DbError::Corrupt(format!("unknown attachment kind '{kind_raw}'")))?,
        original_name: row.get(4)?,
        source: row.get(5)?,
        created_at: row.get(6)?,
    })
}

impl Database {
    /// Store `bytes`, returning its content-addressed metadata. Deduplicating:
    /// the file is written only if it isn't already present, and the metadata
    /// row is inserted only if the hash is new, so storing identical bytes any
    /// number of times leaves exactly one file and one row. The first writer's
    /// `ext`/`kind`/`original_name`/`source` are canonical; later stores of the
    /// same bytes return that first row unchanged.
    pub fn store_attachment(
        &self,
        attachments_dir: &Path,
        bytes: &[u8],
        ext: Option<&str>,
        kind: AttachmentKind,
        original_name: Option<&str>,
        source: &str,
    ) -> Result<AttachmentRef, DbError> {
        let hash = sha256_hex(bytes);
        let existing = self.get_attachment_opt(&hash)?;

        // The canonical extension is the first writer's; a later store of the
        // same bytes reuses it so the file name (and therefore the file) can't
        // fork just because a second caller passed a different extension. A
        // brand-new extension is validated (never just trusted) since it
        // becomes part of the on-disk path below.
        let canonical_ext: Option<String> = match &existing {
            Some(a) => a.ext.clone(),
            None => match ext {
                Some(raw) => sanitize_ext(raw)?,
                None => None,
            },
        };

        let path = attachments_dir.join(attachment_filename(&hash, canonical_ext.as_deref()));
        if !path.exists() {
            std::fs::create_dir_all(attachments_dir)?;
            std::fs::write(&path, bytes)?;
        }

        if existing.is_none() {
            self.raw_conn().execute(
                "INSERT OR IGNORE INTO attachments
                     (content_hash, ext, size_bytes, kind, original_name, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    hash,
                    canonical_ext,
                    bytes.len() as i64,
                    kind.as_sql(),
                    original_name,
                    source,
                ],
            )?;
        }

        self.get_attachment(&hash)
    }

    /// Metadata for a stored blob, or `AttachmentNotFound` if the hash is
    /// unknown.
    pub fn get_attachment(&self, content_hash: &str) -> Result<AttachmentRef, DbError> {
        self.get_attachment_opt(content_hash)?
            .ok_or(DbError::AttachmentNotFound)
    }

    fn get_attachment_opt(&self, content_hash: &str) -> Result<Option<AttachmentRef>, DbError> {
        self.raw_conn()
            .query_row(
                "SELECT content_hash, ext, size_bytes, kind, original_name, source, created_at
                 FROM attachments WHERE content_hash = ?1",
                [content_hash],
                |r| Ok(row_to_attachment(r)),
            )
            .optional()?
            .transpose()
    }

    /// The on-disk path of a stored blob (whether or not the file still
    /// exists), resolved from its recorded extension. `AttachmentNotFound` if
    /// the hash is unknown.
    pub fn attachment_path(
        &self,
        attachments_dir: &Path,
        content_hash: &str,
    ) -> Result<PathBuf, DbError> {
        let meta = self.get_attachment(content_hash)?;
        Ok(attachments_dir.join(attachment_filename(content_hash, meta.ext.as_deref())))
    }

    /// Read a stored blob's bytes back. Used by the `read_attachment` command
    /// so the webview can turn them into a blob URL (image thumbnails etc.).
    pub fn read_attachment(
        &self,
        attachments_dir: &Path,
        content_hash: &str,
    ) -> Result<Vec<u8>, DbError> {
        let path = self.attachment_path(attachments_dir, content_hash)?;
        Ok(std::fs::read(&path)?)
    }

    /// Link a stored blob to a part. Idempotent (`INSERT OR IGNORE` on the
    /// `(part_id, content_hash)` primary key), so attaching the same blob to
    /// the same part twice is a no-op rather than an error.
    pub fn attach_to_part(&self, part_id: &PartId, content_hash: &str) -> Result<(), DbError> {
        if self.get_part(part_id)?.is_none() {
            return Err(DbError::PartNotFound);
        }
        if self.get_attachment_opt(content_hash)?.is_none() {
            return Err(DbError::AttachmentNotFound);
        }
        self.raw_conn().execute(
            "INSERT OR IGNORE INTO part_attachments (part_id, content_hash) VALUES (?1, ?2)",
            rusqlite::params![part_id.as_str(), content_hash],
        )?;
        Ok(())
    }

    /// Every attachment linked to a part, oldest link first.
    pub fn list_part_attachments(&self, part_id: &PartId) -> Result<Vec<AttachmentRef>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT a.content_hash, a.ext, a.size_bytes, a.kind, a.original_name, a.source,
                    a.created_at
             FROM part_attachments pa
             JOIN attachments a ON a.content_hash = pa.content_hash
             WHERE pa.part_id = ?1
             ORDER BY pa.created_at, a.content_hash",
        )?;
        let mut rows = stmt.query([part_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_attachment(row)?);
        }
        Ok(out)
    }

    /// Unlink a blob from a part. The shared blob and its `attachments` row are
    /// left intact — other parts or dimensions may still reference the same
    /// content — so this only removes the `part_attachments` link.
    pub fn remove_part_attachment(
        &self,
        part_id: &PartId,
        content_hash: &str,
    ) -> Result<(), DbError> {
        self.raw_conn().execute(
            "DELETE FROM part_attachments WHERE part_id = ?1 AND content_hash = ?2",
            rusqlite::params![part_id.as_str(), content_hash],
        )?;
        Ok(())
    }

    /// Point a dimension's `attachment_id` at a stored blob (its measurement
    /// photo). Enforces at the application layer the relationship migration
    /// 0005 documents but leaves without a DB-level foreign key.
    pub fn set_dimension_attachment(
        &self,
        dimension_id: &str,
        content_hash: &str,
    ) -> Result<(), DbError> {
        if self.get_attachment_opt(content_hash)?.is_none() {
            return Err(DbError::AttachmentNotFound);
        }
        let n = self.raw_conn().execute(
            "UPDATE dimensions SET attachment_id = ?1 WHERE id = ?2",
            rusqlite::params![content_hash, dimension_id],
        )?;
        if n == 0 {
            return Err(DbError::DimensionNotFound);
        }
        Ok(())
    }
}
