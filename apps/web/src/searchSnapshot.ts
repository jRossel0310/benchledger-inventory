/**
 * Pure snapshot search: the shared query grammar (`@ei/shared`'s
 * `parseQuery`, the TS twin of `inventory-core::search`) evaluated directly
 * against a parsed `Snapshot` -- no FTS, no index; plain substring and
 * predicate scans are fine at snapshot scale (hundreds to low thousands of
 * parts).
 *
 * Semantics mirror the desktop/DB side (`crates/inventory-db/src/search.rs`)
 * for everything the snapshot's data supports:
 *
 * - free text: every whitespace term must match (AND) as a case-insensitive
 *   substring of name/category/description/tags/bin/manufacturer/MPN/
 *   supplier-SKU/attribute keys + original text/dimension names -- exactly
 *   the fields the desktop's `refresh_search_text` indexes; supplier NAMES
 *   are deliberately excluded, as they are there too (the DB uses FTS
 *   prefix matching; substring is the client-side equivalent for this
 *   scale).
 * - `bin:` / `category:` -- exact, case-insensitive.
 * - `available:` / `reserved:` / `checked_out:` / `stock:` -- numeric ops,
 *   filter value in whole units scaled to milli.
 * - attribute keys (e.g. `resistance:>=10k`) -- resolved against the
 *   attribute keys actually present in the snapshot; unit-kinded attributes
 *   compare `parseWithKind(value)` against the stored normalized value
 *   (so equivalent spellings match: `0.1uF` == `100nF`), unit-less ones
 *   fall back to case-insensitive equality on the original text (same
 *   degrade as the DB layer).
 * - dimension names (e.g. `length:<100mm`) -- length parsed to millimeters
 *   (matching the snapshot's normalized dimension values), mass to grams.
 * - `has:datasheet` / `has:dimensions`; `low stock` / `is:low` flag.
 *
 * **The honesty rule:** anything the snapshot cannot answer -- `project:`
 * (no transaction history is published), `is:archived` (archived parts are
 * excluded from the snapshot by construction), unknown keys, unknown `has:`
 * values, or a filter value that doesn't parse for its attribute's unit
 * kind -- is NOT silently dropped: the filter is ignored (does not restrict
 * results) and reported in `unsupported` for the UI to show as a chip.
 */

import {
  parseQuery,
  parseWithKind,
  type FilterOp,
  type RawFilter,
  type Snapshot,
  type SnapshotPart,
  type UnitKind,
} from '@ei/shared';

export interface SearchResult {
  parts: SnapshotPart[];
  /** Human-readable renderings (`key:opvalue`) of every filter that was
   * ignored because the snapshot cannot evaluate it. */
  unsupported: string[];
}

/** Canonical unit symbol (as stored in the snapshot's `canonicalUnit`) ->
 * the `UnitKind` to parse filter values under. Inverse of the shared units
 * module's kind -> canonical-unit table. */
const KIND_BY_UNIT: Record<string, UnitKind> = {
  Ω: 'resistance',
  F: 'capacitance',
  H: 'inductance',
  V: 'voltage',
  A: 'current',
  W: 'power',
  Hz: 'frequency',
  m: 'length',
  g: 'mass',
  s: 'time',
  '%': 'percent',
  C: 'charge',
};

const OP_SYMBOL: Record<FilterOp, string> = {
  eq: '',
  lt: '<',
  le: '<=',
  gt: '>',
  ge: '>=',
  range: '',
};

/** Reconstruct a filter as the user typed it (modulo quoting) for the
 * unsupported-filter chip. */
function chipText(filter: RawFilter): string {
  return `${filter.key}:${OP_SYMBOL[filter.op]}${filter.value}`;
}

/** Mirrors the DB layer's `compare`: relative-epsilon equality so float
 * round-trips (`0.1uF` vs a stored `1e-7`) still compare equal. */
function compare(op: FilterOp, stored: number, target: number): boolean {
  switch (op) {
    case 'eq':
      return Math.abs(stored - target) <= Math.max(Math.abs(target), 1.0) * 1e-9;
    case 'lt':
      return stored < target;
    case 'le':
      return stored <= target;
    case 'gt':
      return stored > target;
    case 'ge':
      return stored >= target;
    case 'range':
      // Range is always split by the caller before reaching here.
      return stored >= target;
  }
}

/** Mirrors `inventory-db::attributes::split_range`: `lo..hi`, both halves
 * non-empty after trimming. */
function splitRange(value: string): [string, string] | null {
  const idx = value.indexOf('..');
  if (idx === -1) return null;
  const lo = value.slice(0, idx).trim();
  const hi = value.slice(idx + 2).trim();
  if (lo === '' || hi === '') return null;
  return [lo, hi];
}

/** A numeric comparison of `stored` against the filter's value(s), where
 * `parse` turns one filter-value string into a number (returning `null` on
 * failure). Returns `null` when the filter value itself doesn't parse --
 * the caller surfaces the whole filter as unsupported. */
function numericPredicate(
  filter: RawFilter,
  parse: (s: string) => number | null,
): ((stored: number) => boolean) | null {
  if (filter.op === 'range') {
    const range = splitRange(filter.value);
    if (range === null) return null;
    const lo = parse(range[0]);
    const hi = parse(range[1]);
    if (lo === null || hi === null) return null;
    return (stored) => stored >= lo && stored <= hi;
  }
  const target = parse(filter.value);
  if (target === null) return null;
  return (stored) => compare(filter.op, stored, target);
}

/** Stock filter values are whole units (possibly fractional for continuous
 * units); stored values are milli. */
function parseStockValue(s: string): number | null {
  const trimmed = s.trim();
  if (trimmed === '' || Number.isNaN(Number(trimmed))) return null;
  return Math.round(Number(trimmed) * 1000);
}

/** Dimension filter values parse as length first (scaled to millimeters,
 * matching the snapshot's normalized dimension values), then as mass
 * (already grams) -- mirroring `dimension_matches` in the DB layer. */
function parseDimensionValue(s: string): number | null {
  const asLength = parseWithKind(s, 'length');
  if (asLength !== null) return asLength.value * 1000;
  const asMass = parseWithKind(s, 'mass');
  if (asMass !== null) return asMass.value;
  return null;
}

type PartPredicate = (part: SnapshotPart) => boolean;

/** Everything free text matches against, lowercased once per part. */
function haystack(part: SnapshotPart): string {
  const fields: string[] = [
    part.displayName,
    part.category,
    part.description,
    ...part.tags,
    part.bin ?? '',
  ];
  for (const variant of part.variants) {
    fields.push(variant.manufacturer, variant.mpn);
    for (const listing of variant.listings) {
      // supplierSku only -- the desktop's refresh_search_text never indexes
      // the supplier NAME, and including it here would make a term like
      // 'digikey' match essentially every part on the web only.
      fields.push(listing.supplierSku);
    }
  }
  // The desktop's refresh_search_text also indexes attribute keys +
  // original text and dimension names -- mirror that so a spec living only
  // in an attribute (e.g. a dielectric of "X7R") is findable by free text.
  for (const attr of part.attributes) {
    fields.push(attr.key, attr.originalText);
  }
  for (const dim of part.dimensions) {
    fields.push(dim.name);
  }
  return fields.join('\n').toLowerCase();
}

function stockPredicate(
  filter: RawFilter,
  pick: (part: SnapshotPart) => number,
): PartPredicate | null {
  const predicate = numericPredicate(filter, parseStockValue);
  if (predicate === null) return null;
  return (part) => predicate(pick(part));
}

/** `has:datasheet` / `has:dimensions`; anything else (including the
 * grammar-recognized-but-unmodeled `has:footprint`) is unsupported. */
function hasPredicate(value: string): PartPredicate | null {
  switch (value) {
    case 'datasheet':
      return (part) => part.variants.some((v) => v.datasheetUrl !== null);
    case 'dimensions':
      return (part) => part.dimensions.length > 0;
    default:
      return null;
  }
}

/** Resolve a non-reserved filter key against the attribute keys and
 * dimension names actually present in the snapshot -- the client-side
 * equivalent of the DB's schema lookup. Returns `null` (unsupported) when
 * the key exists nowhere in the snapshot or its value doesn't parse. */
function attributeOrDimensionPredicate(
  snapshot: Snapshot,
  filter: RawFilter,
): PartPredicate | null {
  // Deliberately more lenient than the desktop, whose key match is
  // case-sensitive: the web has no schema UI to teach users exact casing.
  const key = filter.key.toLowerCase();

  let attrExists = false;
  let unitKind: UnitKind | null = null;
  for (const part of snapshot.parts) {
    for (const attr of part.attributes) {
      if (attr.key.toLowerCase() !== key) continue;
      attrExists = true;
      if (attr.canonicalUnit !== null && unitKind === null) {
        unitKind = KIND_BY_UNIT[attr.canonicalUnit] ?? null;
      }
    }
  }

  if (attrExists) {
    if (unitKind !== null) {
      const kind = unitKind;
      const predicate = numericPredicate(filter, (s) => parseWithKind(s, kind)?.value ?? null);
      if (predicate === null) return null;
      return (part) =>
        part.attributes.some(
          (attr) =>
            attr.key.toLowerCase() === key &&
            attr.normalizedValue !== null &&
            predicate(attr.normalizedValue),
        );
    }
    // Unit-less attribute: case-insensitive equality on the original text
    // (the DB layer's safe degrade for text/choice attributes).
    const target = filter.value.toLowerCase();
    return (part) =>
      part.attributes.some(
        (attr) => attr.key.toLowerCase() === key && attr.originalText.toLowerCase() === target,
      );
  }

  const dimensionExists = snapshot.parts.some((part) =>
    part.dimensions.some((dim) => dim.name.toLowerCase() === key),
  );
  if (dimensionExists) {
    const predicate = numericPredicate(filter, parseDimensionValue);
    if (predicate === null) return null;
    return (part) =>
      part.dimensions.some(
        (dim) => dim.name.toLowerCase() === key && predicate(dim.normalizedValue),
      );
  }

  return null;
}

/** Turn one parsed filter into a part predicate, or `null` when the
 * snapshot cannot evaluate it (unsupported key, unknown `has:` value, or a
 * value that doesn't parse for the key's unit kind). */
function filterPredicate(snapshot: Snapshot, filter: RawFilter): PartPredicate | null {
  switch (filter.key) {
    case 'bin': {
      const target = filter.value.toLowerCase();
      return (part) => part.bin !== null && part.bin.toLowerCase() === target;
    }
    case 'category': {
      const target = filter.value.toLowerCase();
      return (part) => part.category.toLowerCase() === target;
    }
    case 'has':
      return hasPredicate(filter.value);
    case 'available':
      return stockPredicate(filter, (part) => part.stock.availableMilli);
    case 'reserved':
      return stockPredicate(filter, (part) => part.stock.reservedMilli);
    case 'checked_out':
      return stockPredicate(filter, (part) => part.stock.checkedOutMilli);
    case 'stock':
      return stockPredicate(
        filter,
        (part) => part.stock.availableMilli + part.stock.reservedMilli + part.stock.checkedOutMilli,
      );
    case 'project':
      // The snapshot publishes project part-associations but not the
      // transaction history the desktop's project: filter is defined over --
      // honestly unsupported rather than approximated differently.
      return null;
    default:
      return attributeOrDimensionPredicate(snapshot, filter);
  }
}

/**
 * Evaluate `query` (the shared search grammar) against `snapshot`. Returns
 * the matching parts in snapshot order plus the list of filters that were
 * ignored as unsupported (shown to the user -- never silently dropped).
 * An empty query matches every part.
 */
export function searchSnapshot(snapshot: Snapshot, query: string): SearchResult {
  const parsed = parseQuery(query);
  const unsupported: string[] = [];
  const predicates: PartPredicate[] = [];

  // The shared grammar keeps a bare quoted term's literal quote characters
  // in `parsed.text` (documented quirk); desktop FTS strips quotes at
  // tokenization, so strip them here too. Plain strip, no phrase semantics
  // -- the desktop has none either. Terms empty after stripping are dropped.
  const terms = parsed.text
    .toLowerCase()
    .split(/\s+/)
    .map((t) => t.replaceAll('"', ''))
    .filter((t) => t !== '');
  if (terms.length > 0) {
    predicates.push((part) => {
      const hay = haystack(part);
      return terms.every((term) => hay.includes(term));
    });
  }

  for (const filter of parsed.filters) {
    const predicate = filterPredicate(snapshot, filter);
    if (predicate === null) {
      unsupported.push(chipText(filter));
    } else {
      predicates.push(predicate);
    }
  }

  if (parsed.flags.lowStock) {
    predicates.push((part) => part.lowStock);
  }
  // flags.archived === false (is:active) is a no-op: the snapshot only ever
  // contains active parts. archived === true can never match anything for
  // the same reason -- surfaced as unsupported instead of a silent empty
  // result.
  if (parsed.flags.archived === true) {
    unsupported.push('is:archived');
  }

  const parts = snapshot.parts.filter((part) => predicates.every((p) => p(part)));
  return { parts, unsupported };
}
