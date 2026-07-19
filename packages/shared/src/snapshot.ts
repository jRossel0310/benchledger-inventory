/**
 * TS twin of the public snapshot's full schema, mirroring
 * `crates/inventory-sync/src/snapshot.rs`'s `Snapshot` struct tree
 * (Phase 6 Task 1, committed at c4d8b64) field-for-field. `parseSnapshot`
 * structurally validates an `unknown` JSON body (required fields present
 * with the right JS type; every array's elements individually validated)
 * and returns `null` on any mismatch rather than throwing -- a malformed
 * or stale-shape snapshot file must degrade to the web app's empty state,
 * never crash it. Unknown extra top-level or nested keys are tolerated
 * (forward-compat with a future snapshot format that adds fields this
 * client doesn't know about yet) -- only the fields listed below are
 * checked; nothing here rejects on excess keys.
 *
 * ## Field mapping (Rust struct field / serde JSON key -> TS property)
 *
 * Every field is a straight snake_case -> camelCase rename with no other
 * transformation (`display_name` -> `displayName`,
 * `low_stock_threshold_milli` -> `lowStockThresholdMilli`, etc.) EXCEPT:
 * - `published_at`: on the wire this comes from `snapshot.rs`'s
 *   `PublishForm` wrapper (`#[serde(flatten)]`), not the `Snapshot` struct
 *   itself, and is always present in a file that was actually published
 *   (`to_canonical_json(_, Some(timestamp))` -- the digest form with it
 *   omitted is an internal hashing detail, never what's written to the
 *   published path). `parseSnapshot` therefore requires it.
 * - `schema_version` -> `schemaVersion` stays a `string` (Rust's
 *   `SNAPSHOT_FORMAT_VERSION` is a version string, e.g. `"1"`, not a
 *   number) -- see `parseSnapshotHeader`'s doc for how the legacy
 *   `formatVersion: number` field is derived from it.
 * - `Option<T>` fields (`bin`, `package`, `lifecycle`, `datasheet_url`,
 *   `product_url`, `packaging`, `low_stock_threshold_milli`,
 *   `normalized_value` on attributes, `canonical_unit`) map to
 *   `T | null` (never `undefined` -- `serde_json` always emits `null` for
 *   `None`, it never omits the key).
 */

// ---------------------------------------------------------------------------
// Types (mirror crates/inventory-sync/src/snapshot.rs)
// ---------------------------------------------------------------------------

export interface SnapshotStock {
  availableMilli: number;
  reservedMilli: number;
  checkedOutMilli: number;
}

export interface SnapshotAttribute {
  key: string;
  label: string;
  originalText: string;
  normalizedValue: number | null;
  canonicalUnit: string | null;
}

export interface SnapshotDimension {
  group: string;
  name: string;
  valueNum: number;
  displayUnit: string;
  normalizedValue: number;
}

/** Deliberately excludes price/currency/purchase-date/typical-order fields
 * -- see `SnapshotListing`'s Rust doc comment: a supplier SKU is public,
 * what was paid for it is not. */
export interface SnapshotListing {
  supplier: string;
  supplierSku: string;
  productUrl: string | null;
  packaging: string | null;
}

export interface SnapshotVariant {
  id: string;
  manufacturer: string;
  mpn: string;
  package: string | null;
  lifecycle: string | null;
  datasheetUrl: string | null;
  productUrl: string | null;
  isPreferred: boolean;
  listings: SnapshotListing[];
}

export interface SnapshotPart {
  id: string;
  displayName: string;
  category: string;
  description: string;
  tags: string[];
  bin: string | null;
  publicNotes: string;
  usageBehavior: string;
  quantityUnit: string;
  lowStockThresholdMilli: number | null;
  lowStock: boolean;
  stock: SnapshotStock;
  attributes: SnapshotAttribute[];
  dimensions: SnapshotDimension[];
  variants: SnapshotVariant[];
}

export interface SnapshotBin {
  label: string | null;
  partCount: number;
}

/** Deliberately excludes `notes` and `repo_link` -- see `SnapshotProject`'s
 * Rust doc comment. */
export interface SnapshotProject {
  id: string;
  name: string;
  status: string;
  description: string;
  buildQuantity: number;
  partIds: string[];
}

/** The full published snapshot -- `PublishForm { published_at, ..Snapshot }`
 * flattened, exactly as it appears in the published JSON file. */
export interface Snapshot {
  publishedAt: string;
  schemaVersion: string;
  parts: SnapshotPart[];
  bins: SnapshotBin[];
  projects: SnapshotProject[];
}

/** Header fields of the published snapshot, kept for callers that only
 * need a cheap summary. Superseded by `parseSnapshot`/`Snapshot` (the full
 * schema, added Phase 6 Task 2) -- see `parseSnapshotHeader`'s doc. */
export interface SnapshotHeader {
  formatVersion: number;
  publishedAt: string; // ISO-8601 UTC
  partCount: number;
}

// ---------------------------------------------------------------------------
// Structural validation
// ---------------------------------------------------------------------------

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function isStr(v: unknown): v is string {
  return typeof v === 'string';
}

function isNum(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v);
}

function isBool(v: unknown): v is boolean {
  return typeof v === 'boolean';
}

function isStrOrNull(v: unknown): v is string | null {
  return v === null || typeof v === 'string';
}

function isNumOrNull(v: unknown): v is number | null {
  return v === null || isNum(v);
}

/** Validates `v` is an array and every element parses via `parseItem`;
 * returns the parsed array or `null` on any failure (mirrors the "every
 * field that exists is safe/required" posture -- one malformed element
 * invalidates the whole snapshot rather than silently dropping it). */
function parseArray<T>(v: unknown, parseItem: (item: unknown) => T | null): T[] | null {
  if (!Array.isArray(v)) return null;
  const out: T[] = [];
  for (const item of v) {
    const parsed = parseItem(item);
    if (parsed === null) return null;
    out.push(parsed);
  }
  return out;
}

function parseStringArray(v: unknown): string[] | null {
  return parseArray(v, (item) => (isStr(item) ? item : null));
}

function parseStock(v: unknown): SnapshotStock | null {
  if (!isRecord(v)) return null;
  if (!isNum(v.available_milli) || !isNum(v.reserved_milli) || !isNum(v.checked_out_milli)) {
    return null;
  }
  return {
    availableMilli: v.available_milli,
    reservedMilli: v.reserved_milli,
    checkedOutMilli: v.checked_out_milli,
  };
}

function parseAttribute(v: unknown): SnapshotAttribute | null {
  if (!isRecord(v)) return null;
  if (!isStr(v.key) || !isStr(v.label) || !isStr(v.original_text)) return null;
  if (!isNumOrNull(v.normalized_value) || !isStrOrNull(v.canonical_unit)) return null;
  return {
    key: v.key,
    label: v.label,
    originalText: v.original_text,
    normalizedValue: v.normalized_value,
    canonicalUnit: v.canonical_unit,
  };
}

function parseDimension(v: unknown): SnapshotDimension | null {
  if (!isRecord(v)) return null;
  if (
    !isStr(v.group) ||
    !isStr(v.name) ||
    !isNum(v.value_num) ||
    !isStr(v.display_unit) ||
    !isNum(v.normalized_value)
  ) {
    return null;
  }
  return {
    group: v.group,
    name: v.name,
    valueNum: v.value_num,
    displayUnit: v.display_unit,
    normalizedValue: v.normalized_value,
  };
}

function parseListing(v: unknown): SnapshotListing | null {
  if (!isRecord(v)) return null;
  if (!isStr(v.supplier) || !isStr(v.supplier_sku)) return null;
  if (!isStrOrNull(v.product_url) || !isStrOrNull(v.packaging)) return null;
  return {
    supplier: v.supplier,
    supplierSku: v.supplier_sku,
    productUrl: v.product_url,
    packaging: v.packaging,
  };
}

function parseVariant(v: unknown): SnapshotVariant | null {
  if (!isRecord(v)) return null;
  if (!isStr(v.id) || !isStr(v.manufacturer) || !isStr(v.mpn)) return null;
  if (
    !isStrOrNull(v.package) ||
    !isStrOrNull(v.lifecycle) ||
    !isStrOrNull(v.datasheet_url) ||
    !isStrOrNull(v.product_url)
  ) {
    return null;
  }
  if (!isBool(v.is_preferred)) return null;
  const listings = parseArray(v.listings, parseListing);
  if (listings === null) return null;
  return {
    id: v.id,
    manufacturer: v.manufacturer,
    mpn: v.mpn,
    package: v.package,
    lifecycle: v.lifecycle,
    datasheetUrl: v.datasheet_url,
    productUrl: v.product_url,
    isPreferred: v.is_preferred,
    listings,
  };
}

function parsePart(v: unknown): SnapshotPart | null {
  if (!isRecord(v)) return null;
  if (!isStr(v.id) || !isStr(v.display_name) || !isStr(v.category) || !isStr(v.description)) {
    return null;
  }
  const tags = parseStringArray(v.tags);
  if (tags === null) return null;
  if (!isStrOrNull(v.bin)) return null;
  if (!isStr(v.public_notes) || !isStr(v.usage_behavior) || !isStr(v.quantity_unit)) return null;
  if (!isNumOrNull(v.low_stock_threshold_milli)) return null;
  if (!isBool(v.low_stock)) return null;
  const stock = parseStock(v.stock);
  if (stock === null) return null;
  const attributes = parseArray(v.attributes, parseAttribute);
  if (attributes === null) return null;
  const dimensions = parseArray(v.dimensions, parseDimension);
  if (dimensions === null) return null;
  const variants = parseArray(v.variants, parseVariant);
  if (variants === null) return null;
  return {
    id: v.id,
    displayName: v.display_name,
    category: v.category,
    description: v.description,
    tags,
    bin: v.bin,
    publicNotes: v.public_notes,
    usageBehavior: v.usage_behavior,
    quantityUnit: v.quantity_unit,
    lowStockThresholdMilli: v.low_stock_threshold_milli,
    lowStock: v.low_stock,
    stock,
    attributes,
    dimensions,
    variants,
  };
}

function parseBin(v: unknown): SnapshotBin | null {
  if (!isRecord(v)) return null;
  if (!isStrOrNull(v.label) || !isNum(v.part_count)) return null;
  return { label: v.label, partCount: v.part_count };
}

function parseProject(v: unknown): SnapshotProject | null {
  if (!isRecord(v)) return null;
  if (
    !isStr(v.id) ||
    !isStr(v.name) ||
    !isStr(v.status) ||
    !isStr(v.description) ||
    !isNum(v.build_quantity)
  ) {
    return null;
  }
  const partIds = parseStringArray(v.part_ids);
  if (partIds === null) return null;
  return {
    id: v.id,
    name: v.name,
    status: v.status,
    description: v.description,
    buildQuantity: v.build_quantity,
    partIds,
  };
}

/**
 * Structurally validates `body` as a full published `Snapshot`: every
 * required field present with the right JS type, every array's elements
 * individually valid. Returns `null` (never throws) on any mismatch --
 * missing snapshot / malformed JSON / stale shape all collapse to the same
 * "couldn't load" outcome for the caller. Unknown extra keys, at any
 * nesting level, are ignored (forward-compatible with future snapshot
 * fields).
 */
export function parseSnapshot(body: unknown): Snapshot | null {
  if (!isRecord(body)) return null;
  if (!isStr(body.published_at) || !isStr(body.schema_version)) return null;
  const parts = parseArray(body.parts, parsePart);
  if (parts === null) return null;
  const bins = parseArray(body.bins, parseBin);
  if (bins === null) return null;
  const projects = parseArray(body.projects, parseProject);
  if (projects === null) return null;
  return {
    publishedAt: body.published_at,
    schemaVersion: body.schema_version,
    parts,
    bins,
    projects,
  };
}

/**
 * Thin wrapper over `parseSnapshot`, kept for callers that only want the
 * header summary (originally written before Task 1 defined the real
 * `Snapshot` shape, back when this was the only parser). `formatVersion`
 * is `Number(schemaVersion)` (Rust's `SNAPSHOT_FORMAT_VERSION` is the
 * string `"1"`); `partCount` is `parts.length`. Because it now delegates
 * to the full structural parse, a body that satisfies the header shape but
 * not the full `Snapshot` shape (e.g. an old fixture with only
 * `formatVersion`/`publishedAt`/`partCount` and no `parts`/`bins`/
 * `projects` arrays) no longer parses -- there is no longer a real
 * snapshot format that has a header without a body, so this reflects that.
 */
export function parseSnapshotHeader(json: unknown): SnapshotHeader | null {
  const snapshot = parseSnapshot(json);
  if (snapshot === null) return null;
  return {
    formatVersion: Number(snapshot.schemaVersion),
    publishedAt: snapshot.publishedAt,
    partCount: snapshot.parts.length,
  };
}
