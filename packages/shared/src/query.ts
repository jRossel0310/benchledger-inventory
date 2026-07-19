/**
 * TS twin of `crates/inventory-core/src/search.rs`'s `parse_query`: the
 * syntactic, kind-blind search-bar grammar (free text, `key:value`
 * filters, boolean flags). Locked to the Rust implementation by
 * `packages/shared/fixtures/query-cases.json`, generated FROM the Rust
 * parser by a drift-lock test (`search.rs`'s
 * `query_cases_fixture_matches_generator`, mirroring how
 * `apps/desktop/src-tauri/src/commands.rs`'s `export_bindings` locks
 * `bindings.gen.ts` to the Tauri command surface) -- every case in that
 * fixture is `(raw input, the exact ParsedQuery parse_query(input)
 * produced)`, so this module is tested by feeding each input through
 * `parseQuery` and comparing.
 *
 * ## Field mapping (snake_case Rust serde output -> camelCase TS)
 *
 * `RawFilter`, `FilterOp`, and `ParsedQuery.text`/`.filters` carry no
 * multi-word snake_case field names, so they pass through unchanged.
 * `QueryFlags.low_stock` (Rust/fixture) is the ONE renamed field, becoming
 * `lowStock` here; `archived` is unchanged (already single-word, `Option<bool>`
 * -> `boolean | null`). `FilterOp`'s serde values (`eq`, `lt`, `le`, `gt`,
 * `ge`, `range`) are already single lowercase words and are reproduced
 * VERBATIM as this module's `FilterOp` string values -- no case
 * conversion -- so a fixture case's `parsed.filters[].op` string compares
 * directly against this module's output with no translation needed. The
 * fixture-comparison test implements one `fromFixture` normalizer (fixture
 * shape -> this module's shape, i.e. only the `low_stock` -> `lowStock`
 * rename) rather than duplicating that mapping in production code.
 *
 * ## Documented grammar quirks (must reproduce exactly, per `search.rs`)
 *
 * - An operator prefix wins over range detection even when the remaining
 *   value still contains `..`: `resistance:>1..5` classifies as `Gt` with
 *   value `"1..5"` (the `..` is NOT stripped or specially handled -- the
 *   DB layer's range splitter never sees it because `extract_op` checks
 *   `>`/`<`/`>=`/`<=`/`=` before it ever checks `contains("..")`).
 * - An unterminated `"` never closes: `tokenize`'s `in_quotes` flag stays
 *   true for the remainder of the input, so everything after the opening
 *   quote -- including embedded spaces and the lone `"` itself -- becomes
 *   one token/filter-value. `extract_op` only strips quotes from a value
 *   that both starts AND ends with `"`, so the absorbed value keeps its
 *   literal leading quote character.
 * - A bare quoted free-text term (no `key:` prefix) is never unwrapped:
 *   quote-stripping happens only inside `extract_op` for filter values, so
 *   `"hello world"` as a whole query is a single free-text token that
 *   keeps its literal surrounding quotes in `.text`.
 */

export type FilterOp = 'eq' | 'lt' | 'le' | 'gt' | 'ge' | 'range';

export interface RawFilter {
  key: string;
  op: FilterOp;
  value: string;
}

export interface QueryFlags {
  lowStock: boolean;
  /** `null` = don't filter on archived state; `true` = archived only;
   * `false` = active only. Mirrors Rust's `Option<bool>`. */
  archived: boolean | null;
}

export interface ParsedQuery {
  /** Space-joined free-text terms, order preserved. Empty when the query
   * is only filters/flags. */
  text: string;
  filters: RawFilter[];
  flags: QueryFlags;
}

/**
 * Mirrors Rust's `char::is_whitespace()` (the Unicode `White_Space`
 * property). JS `/\s/` matches the same set PLUS U+FEFF (ZERO WIDTH
 * NO-BREAK SPACE / BOM), which `White_Space` does not include -- so U+FEFF
 * is excluded here explicitly and rides inside tokens exactly as it does in
 * Rust (fixture-pinned by the `"cap\uFEFF0603"` query-cases.json case).
 *
 * Known remaining divergence (documented, not fixed): U+0085 NEXT LINE is
 * `White_Space` in Rust but NOT matched by JS `/\s/`, so a query containing
 * a literal NEL splits into two tokens in Rust and stays one token here.
 * NEL is untypeable in a search box and survives almost no clipboard
 * round-trip; see docs/known-limitations.md.
 */
function isWhitespace(c: string): boolean {
  return c !== '\uFEFF' && /\s/.test(c);
}

/** Mirrors `tokenize`: splits on whitespace, treating a double-quoted span
 * (which may itself contain whitespace) as atomic. Quote characters are
 * kept in the token; `extractOp` strips them from filter values
 * afterward. An unterminated quote absorbs the rest of the input into one
 * token (see the module doc's quirks list). */
function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let current = '';
  let inQuotes = false;

  for (const c of input) {
    if (c === '"') {
      inQuotes = !inQuotes;
      current += c;
    } else if (isWhitespace(c) && !inQuotes) {
      if (current !== '') {
        tokens.push(current);
        current = '';
      }
    } else {
      current += c;
    }
  }
  if (current !== '') tokens.push(current);
  return tokens;
}

/** Mirrors `extract_op`: `>=`/`<=`/`>`/`<`/`=` strip to their operator
 * (value keeps the remainder); a value containing `..` (checked only
 * AFTER the operator-prefix checks above) is a `range` and keeps its full
 * raw string; a fully double-quoted value is a literal (`eq`, quotes
 * stripped, no prefix scanning); anything else is `eq` verbatim. */
function extractOp(value: string): [FilterOp, string] {
  if (value.length >= 2 && value.startsWith('"') && value.endsWith('"')) {
    return ['eq', value.slice(1, -1)];
  }
  if (value.startsWith('>=')) return ['ge', value.slice(2)];
  if (value.startsWith('<=')) return ['le', value.slice(2)];
  if (value.startsWith('>')) return ['gt', value.slice(1)];
  if (value.startsWith('<')) return ['lt', value.slice(1)];
  if (value.startsWith('=')) return ['eq', value.slice(1)];
  if (value.includes('..')) return ['range', value];
  return ['eq', value];
}

/**
 * Mirrors `parse_query`: parse a raw search-bar string into free text,
 * filters, and flags. Infallible -- every input, including the empty
 * string, produces a `ParsedQuery`.
 */
export function parseQuery(input: string): ParsedQuery {
  const tokens = tokenize(input);
  const textTerms: string[] = [];
  const filters: RawFilter[] = [];
  const flags: QueryFlags = { lowStock: false, archived: null };

  let i = 0;
  while (i < tokens.length) {
    const tok = tokens[i] as string; // i < tokens.length, always defined

    // Bare "low stock" (two tokens) or "low-stock" (one token).
    if (tok === 'low' && tokens[i + 1] === 'stock') {
      flags.lowStock = true;
      i += 2;
      continue;
    }
    if (tok === 'low-stock') {
      flags.lowStock = true;
      i += 1;
      continue;
    }

    const colon = tok.indexOf(':');
    if (colon !== -1) {
      const key = tok.slice(0, colon);
      const rawValue = tok.slice(colon + 1);
      const [op, value] = extractOp(rawValue);

      if (key === 'is' && op === 'eq') {
        if (value === 'archived') {
          flags.archived = true;
          i += 1;
          continue;
        }
        if (value === 'active') {
          flags.archived = false;
          i += 1;
          continue;
        }
        if (value === 'low') {
          flags.lowStock = true;
          i += 1;
          continue;
        }
      }

      filters.push({ key, op, value });
      i += 1;
      continue;
    }

    textTerms.push(tok);
    i += 1;
  }

  return { text: textTerms.join(' '), filters, flags };
}
