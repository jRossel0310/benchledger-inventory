//! Deterministic, privacy-safe public snapshot builder (spec §12, Phase 6
//! Task 1). `build_snapshot` reads the local database through the same
//! typed repository APIs the desktop UI uses (`inventory_db::Database`'s
//! `pub fn`s) and assembles a `Snapshot` — a plain serde struct tree that is
//! deliberately a *narrower* view of the schema than the DB itself: every
//! field that exists on `Snapshot` is a field that is safe to publish to a
//! public GitHub repo and a public website. Nothing here ever reads
//! `private_notes`, any price/micros column, purchase dates, import/invoice
//! tables, attachments, `field_provenance`, or archived parts — the fields
//! simply do not appear on the structs below, so there is no privacy-unsafe
//! data left to accidentally serialize (`tests/snapshot.rs`'s denylist test
//! is the enforcement backstop, not the only safeguard).
//!
//! ## Determinism
//!
//! Two builds from the same DB state must produce byte-identical JSON:
//! - Every collection is explicitly sorted here (by ULID for parts/variants/
//!   projects, alphabetically for tags/bins/dimension keys) rather than
//!   relying on SQL row order, which SQLite does not guarantee is stable
//!   across connections or versions.
//! - `to_canonical_json` serializes through serde's normal (non-`Value`,
//!   non-`HashMap`) struct path, so field order always matches each struct's
//!   declaration order — never alphabetized, never insertion-order-of-a-map.
//! - `published_at` is the only field allowed to vary between two snapshots
//!   of identical inventory content; `content_digest` always hashes the form
//!   with `published_at` omitted (see `to_canonical_json(snapshot, None)`),
//!   so an unchanged inventory always yields the same digest regardless of
//!   when it was computed.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use inventory_db::Database;

use crate::SyncError;

/// Snapshot format version — bumped when `Snapshot`'s shape changes in a way
/// that would break an already-published consumer (the web app, GitHub
/// history). Independent of the DB's `SUPPORTED_SCHEMA_VERSION`.
const SNAPSHOT_FORMAT_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub schema_version: String,
    pub parts: Vec<SnapshotPart>,
    pub bins: Vec<SnapshotBin>,
    pub projects: Vec<SnapshotProject>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotPart {
    pub id: String,
    pub display_name: String,
    pub category: String,
    pub description: String,
    pub tags: Vec<String>,
    pub bin: Option<String>,
    pub public_notes: String,
    pub usage_behavior: String,
    pub quantity_unit: String,
    pub low_stock_threshold_milli: Option<i64>,
    pub low_stock: bool,
    pub stock: SnapshotStock,
    pub attributes: Vec<SnapshotAttribute>,
    pub dimensions: Vec<SnapshotDimension>,
    pub variants: Vec<SnapshotVariant>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotStock {
    pub available_milli: i64,
    pub reserved_milli: i64,
    pub checked_out_milli: i64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotAttribute {
    pub key: String,
    pub label: String,
    pub original_text: String,
    pub normalized_value: Option<f64>,
    pub canonical_unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotDimension {
    pub group: String,
    pub name: String,
    pub value_num: f64,
    pub display_unit: String,
    pub normalized_value: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotVariant {
    pub id: String,
    pub manufacturer: String,
    pub mpn: String,
    pub package: Option<String>,
    pub lifecycle: Option<String>,
    pub datasheet_url: Option<String>,
    pub product_url: Option<String>,
    pub is_preferred: bool,
    pub listings: Vec<SnapshotListing>,
}

/// Deliberately excludes `last_unit_price_micros`, `currency`,
/// `last_purchase_date`, and `typical_order` from `inventory_db::parts::ListingRecord`
/// — a supplier part number is public, what was paid for it is not.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotListing {
    pub supplier: String,
    pub supplier_sku: String,
    pub product_url: Option<String>,
    pub packaging: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotBin {
    pub label: Option<String>,
    pub part_count: i64,
}

/// Deliberately excludes `notes` and `repo_link` from
/// `inventory_db::projects::ProjectRecord` — see the plan's Task 1 note:
/// notes are free-text working scratchpad, and repo_link is left out this
/// phase pending an explicit decision on whether a linked repo is meant to
/// be public (a future phase can add it back once that's confirmed).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotProject {
    pub id: String,
    pub name: String,
    pub status: String,
    pub description: String,
    pub build_quantity: i64,
    pub part_ids: Vec<String>,
}

/// Reads the current database and assembles a `Snapshot`. Only non-archived
/// parts are included (`list_parts(false)`); every collection is sorted
/// explicitly (see the module doc comment) so the result does not depend on
/// SQL row order.
pub fn build_snapshot(db: &Database) -> Result<Snapshot, SyncError> {
    let category_names: HashMap<String, String> = db
        .list_categories()?
        .into_iter()
        .map(|c| (c.id.as_str().to_string(), c.name))
        .collect();

    let mut parts = db.list_parts(false)?;
    parts.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    let mut snapshot_parts = Vec::with_capacity(parts.len());
    for part in &parts {
        let category = category_names
            .get(part.category_id.as_str())
            .cloned()
            .unwrap_or_default();
        let tags = db.get_tags(&part.id)?; // already ORDER BY tag
        let stock = db.get_stock(&part.id)?;
        let low_stock_threshold_milli = part.low_stock_threshold.map(|q| q.as_milli());
        let available_milli = stock.available.as_milli();
        let low_stock = low_stock_threshold_milli.is_some_and(|t| available_milli <= t);

        let mut attributes: Vec<SnapshotAttribute> = db
            .list_attribute_values(&part.id)?
            .into_iter()
            .map(|a| SnapshotAttribute {
                key: a.key,
                label: a.label,
                original_text: a.original_text,
                normalized_value: a.normalized_value,
                canonical_unit: a.canonical_unit,
            })
            .collect();
        attributes.sort_by(|a, b| a.key.cmp(&b.key));

        let mut dimensions: Vec<SnapshotDimension> = db
            .list_dimensions(&part.id)?
            .into_iter()
            .map(|d| SnapshotDimension {
                group: d.group.as_sql().to_string(),
                name: d.name,
                value_num: d.value_num,
                display_unit: d.display_unit,
                normalized_value: d.normalized_value,
            })
            .collect();
        dimensions.sort_by(|a, b| {
            (a.group.as_str(), a.name.as_str()).cmp(&(b.group.as_str(), b.name.as_str()))
        });

        let mut variants = Vec::new();
        for variant in db.list_variants(&part.id)? {
            // Already ordered by (supplier, supplier_sku) — see
            // `Database::list_supplier_listings`.
            let listings: Vec<SnapshotListing> = db
                .list_supplier_listings(&variant.id)?
                .into_iter()
                .map(|l| SnapshotListing {
                    supplier: l.supplier,
                    supplier_sku: l.supplier_sku,
                    product_url: l.product_url,
                    packaging: l.packaging,
                })
                .collect();
            variants.push(SnapshotVariant {
                id: variant.id.as_str().to_string(),
                manufacturer: variant.manufacturer,
                mpn: variant.mpn,
                package: variant.package,
                lifecycle: variant.lifecycle,
                datasheet_url: variant.datasheet_url,
                product_url: variant.product_url,
                is_preferred: variant.is_preferred,
                listings,
            });
        }
        variants.sort_by(|a, b| a.id.cmp(&b.id));

        snapshot_parts.push(SnapshotPart {
            id: part.id.as_str().to_string(),
            display_name: part.display_name.clone(),
            category,
            description: part.description.clone(),
            tags,
            bin: part.bin_label.clone(),
            public_notes: part.public_notes.clone(),
            usage_behavior: part.usage_behavior.clone(),
            quantity_unit: part.quantity_unit.as_sql().to_string(),
            low_stock_threshold_milli,
            low_stock,
            stock: SnapshotStock {
                available_milli,
                reserved_milli: stock.reserved.as_milli(),
                checked_out_milli: stock.checked_out.as_milli(),
            },
            attributes,
            dimensions,
            variants,
        });
    }

    let bins = db
        .list_bins()?
        .into_iter()
        .map(|b| SnapshotBin {
            label: b.bin_label,
            part_count: b.part_count,
        })
        .collect();

    // Every project regardless of status: a project's `status` field already
    // communicates planned/active/completed/archived, and — unlike a part —
    // an archived project isn't stale/superseded data, just a finished one;
    // its BOM associations remain meaningful history.
    let mut project_records = db.list_projects_full(None)?;
    project_records.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    let mut projects = Vec::with_capacity(project_records.len());
    for project in &project_records {
        let mut part_ids: Vec<String> = db
            .list_bom(&project.id)?
            .into_iter()
            .map(|item| item.part_id.as_str().to_string())
            .collect();
        part_ids.sort();
        projects.push(SnapshotProject {
            id: project.id.as_str().to_string(),
            name: project.name.clone(),
            status: project.status.as_sql().to_string(),
            description: project.description.clone(),
            build_quantity: project.build_quantity,
            part_ids,
        });
    }

    Ok(Snapshot {
        schema_version: SNAPSHOT_FORMAT_VERSION.to_string(),
        parts: snapshot_parts,
        bins,
        projects,
    })
}

/// Wrapper that puts an optional `published_at` ahead of `Snapshot`'s own
/// fields via `#[serde(flatten)]` — the only field this whole module ever
/// varies between two structurally-identical snapshots. `skip_serializing_if`
/// omits the key entirely (not `null`) when `published_at` is `None`, which
/// is what makes the digest form and the publish form byte-comparable except
/// for that one key.
#[derive(serde::Serialize)]
struct PublishForm<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    published_at: Option<&'a str>,
    #[serde(flatten)]
    snapshot: &'a Snapshot,
}

/// Renders `snapshot` to its canonical, byte-stable JSON form: 2-space
/// indent, LF line endings, exactly one trailing newline. `published_at:
/// None` produces the digest form (key omitted entirely, not `null`); `Some`
/// produces the publish form with that timestamp included.
pub fn to_canonical_json(snapshot: &Snapshot, published_at: Option<&str>) -> String {
    let form = PublishForm {
        published_at,
        snapshot,
    };
    // `Snapshot` is built exclusively from Strings, bools, i64s, and finite
    // f64s parsed out of already-validated attribute/dimension values
    // (`inventory_core::units::parse_with_kind`/`add_dimension` reject
    // anything that isn't a plain finite number) — there is no map with
    // non-string keys and no way for this to fail in practice, so
    // `to_canonical_json`/`content_digest` (matching the plan's Interfaces
    // block) return a bare `String` rather than a `Result`.
    let mut json = serde_json::to_string_pretty(&form)
        .expect("Snapshot contains only plain serde-safe data; serialization cannot fail");
    json.push('\n');
    json
}

/// SHA-256 hex digest of the digest form (`published_at` omitted) — the
/// value `inventory-sync::publish` (Task 4) compares against
/// `app_state.last_published_digest` to decide whether a publish is a no-op.
pub fn content_digest(snapshot: &Snapshot) -> String {
    let json = to_canonical_json(snapshot, None);
    let digest = Sha256::digest(json.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}
