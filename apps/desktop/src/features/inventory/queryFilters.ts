/**
 * Pure translation between the Inventory browser's filter chips and search
 * query-string fragments (Phase 3 Task 4, see
 * docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md §9). Every
 * fragment written/read here is one `inventory_core::search::parse_query`
 * (`crates/inventory-core/src/search.rs`) genuinely understands — no filter
 * is invented that the backend grammar doesn't already support:
 * `category:X` (quoted when `X` has whitespace, matching the grammar's
 * `bin:"Drawer 3"`-style quoting), the bare two-token `low stock` flag,
 * `is:archived`, `has:datasheet`, `has:dimensions`.
 *
 * Filters compose with free text and with each other by literally appending/
 * removing their fragment in the single `q` string the whole screen shares
 * (search box, filter chips, and saved views all read/write the same
 * string) — there is no separate structured filter state to keep in sync.
 */

export interface ActiveFilters {
  /** The unquoted category name from a `category:` fragment, or `null` if
   * none is present. */
  category: string | null;
  lowStock: boolean;
  archived: boolean;
  hasDatasheet: boolean;
  hasDimensions: boolean;
}

const LOW_STOCK_TOKENS = ['low', 'stock'];
const ARCHIVED_TOKEN = 'is:archived';
const HAS_DATASHEET_TOKEN = 'has:datasheet';
const HAS_DIMENSIONS_TOKEN = 'has:dimensions';
const CATEGORY_PREFIX = 'category:';

/** Split a query string into whitespace-separated tokens, treating a
 * double-quoted span as one atomic token (mirrors the backend tokenizer's
 * quoting rule in `inventory_core::search::tokenize`) so a quoted category
 * name with an internal space is never split into two tokens. */
function splitTokens(query: string): string[] {
  const tokens: string[] = [];
  let current = '';
  let inQuotes = false;
  for (const ch of query) {
    if (ch === '"') {
      inQuotes = !inQuotes;
      current += ch;
    } else if (/\s/.test(ch) && !inQuotes) {
      if (current) {
        tokens.push(current);
        current = '';
      }
    } else {
      current += ch;
    }
  }
  if (current) tokens.push(current);
  return tokens;
}

function joinTokens(tokens: string[]): string {
  return tokens.join(' ');
}

/** Remove every occurrence of `fragmentTokens` as a contiguous, in-order run
 * from `tokens` (used for both single-token flags and the two-token `low
 * stock` phrase, with the same logic either way). */
function removeFragmentTokens(tokens: string[], fragmentTokens: string[]): string[] {
  const result: string[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const matches = fragmentTokens.every((t, j) => tokens[i + j] === t);
    if (matches) {
      i += fragmentTokens.length - 1;
      continue;
    }
    result.push(tokens[i] as string);
  }
  return result;
}

function setFlagFragment(query: string, fragmentTokens: string[], enabled: boolean): string {
  const tokens = removeFragmentTokens(splitTokens(query), fragmentTokens);
  if (enabled) tokens.push(...fragmentTokens);
  return joinTokens(tokens);
}

function hasFragmentTokens(tokens: string[], fragmentTokens: string[]): boolean {
  for (let i = 0; i + fragmentTokens.length <= tokens.length; i++) {
    if (fragmentTokens.every((t, j) => tokens[i + j] === t)) return true;
  }
  return false;
}

/** Toggle the bare two-token `low stock` flag. */
export function withLowStock(query: string, enabled: boolean): string {
  return setFlagFragment(query, LOW_STOCK_TOKENS, enabled);
}

/** Toggle the `is:archived` flag. */
export function withArchived(query: string, enabled: boolean): string {
  return setFlagFragment(query, [ARCHIVED_TOKEN], enabled);
}

/** Toggle the `has:datasheet` filter. */
export function withHasDatasheet(query: string, enabled: boolean): string {
  return setFlagFragment(query, [HAS_DATASHEET_TOKEN], enabled);
}

/** Toggle the `has:dimensions` filter. */
export function withHasDimensions(query: string, enabled: boolean): string {
  return setFlagFragment(query, [HAS_DIMENSIONS_TOKEN], enabled);
}

/** Set (or, for `null`, clear) the `category:` filter — replaces any
 * existing `category:` fragment rather than appending a second one, since
 * only one category can be selected at a time. Quotes the value when it
 * contains whitespace, matching the grammar's quoting for multi-word values
 * (e.g. `category:"Voltage regulator"`). */
export function withCategory(query: string, category: string | null): string {
  const tokens = splitTokens(query).filter((t) => !t.startsWith(CATEGORY_PREFIX));
  if (category) {
    const value = /\s/.test(category) ? `"${category}"` : category;
    tokens.push(`${CATEGORY_PREFIX}${value}`);
  }
  return joinTokens(tokens);
}

function unquote(value: string): string {
  if (value.length >= 2 && value.startsWith('"') && value.endsWith('"')) {
    return value.slice(1, -1);
  }
  return value;
}

/** Read the current filter-chip state back out of a query string — the
 * inverse of the `with*` helpers, used to render each chip as active/
 * inactive and the category select's current value. */
export function parseActiveFilters(query: string): ActiveFilters {
  const tokens = splitTokens(query);
  const categoryToken = tokens.find((t) => t.startsWith(CATEGORY_PREFIX));
  return {
    category: categoryToken ? unquote(categoryToken.slice(CATEGORY_PREFIX.length)) : null,
    lowStock: hasFragmentTokens(tokens, LOW_STOCK_TOKENS),
    archived: tokens.includes(ARCHIVED_TOKEN),
    hasDatasheet: tokens.includes(HAS_DATASHEET_TOKEN),
    hasDimensions: tokens.includes(HAS_DIMENSIONS_TOKEN),
  };
}
