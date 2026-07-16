# Search query grammar

The search bar is a single string accepted by `Database::search`. Parsing
happens in two layers:

- `inventory_core::search::parse_query` — a syntactic, DB-blind tokenizer.
  Splits the raw string into free-text terms, `key:value` filters, and
  boolean flags. Infallible: every input, including the empty string,
  produces a `ParsedQuery`. Has no knowledge of the schema — it doesn't know
  `bin` from a made-up key. A future web TS twin re-implements this exact
  grammar so the web app's search bar behaves identically without a round
  trip to the Rust core.
- `inventory_db::search` — resolves each `RawFilter`'s key against the
  schema (attribute defs, dimensions, or a fixed set of reserved words) and
  executes: narrows to an FTS candidate set for any free text, applies every
  structured filter as a set intersection, applies the archived/low-stock
  flags, and returns deterministically ordered `SearchHit`s.

## Free text

Any token without a `key:` prefix and not a recognized flag is free text,
space-joined in original order. Matched against `parts_fts`, the FTS5 index
over `search_text` (see `docs/schema.md`'s migration 0004 section): each
term becomes a double-quoted phrase with a trailing `*` for prefix matching,
terms are joined by FTS5's implicit AND. When the query has no free text,
every part is a candidate and filters/flags apply directly.

```
10k                          -- matches display name, attribute text, etc.
TLV9002                      -- prefix-matches the tokenized "tlv9002iddfr"
296-TLV9002IDDFRCT-ND        -- matches a supplier SKU (tokenchars '-_.'
                                 keep hyphenated SKUs as one token)
```

## Comparison operators

A filter's value prefix selects the operator; whatever follows the prefix is
the comparison value.

| Prefix | Operator | Example |
|---|---|---|
| *(none)* | `Eq` | `bin:A12` |
| `>` | `Gt` | `available:>10` |
| `>=` | `Ge` | `voltage_rating:>=25V` |
| `<` | `Lt` | `available:<10` |
| `<=` | `Le` | `height:<=10mm` |
| `a..b` | `Range` | `capacitance:10nF..1uF` |
| `"..."` | `Eq` (literal) | `bin:"Drawer 3"` |

A fully double-quoted value (`bin:"Drawer 3"`) is a literal equality match —
quotes are stripped and the value may contain spaces, and no operator prefix
is scanned for. Range detection (a bare `..` in the value) is checked *after*
the `>=`/`<=`/`>`/`<`/`=` prefixes, so `>1..5` parses as `Gt` with value
`1..5` rather than as a range — deliberate, if surprising: get the operator
right and skip `..` in a `>`/`<` value.

**Tokenization quirks worth knowing:** a double-quoted span is treated as one
atomic token wherever it appears, including in bare free text — but unlike
inside a filter's value, a bare quoted free-text term does *not* have its
quotes stripped (they stay in the resulting text term literally). An
unbalanced quote (no closing `"`) absorbs the rest of the input into a single
token rather than erroring.

## Reserved keys

- **`project:<substring>`** — parts with any reservation, checkout, or
  transfer transaction (either leg) against a project whose name contains
  `<substring>`, case-insensitive. `project:Blinky` matches a project named
  "Blinky Board".
- **`bin:<label>`** — exact, case-insensitive match on the part's bin label.
  Use the quoted form for bins with spaces: `bin:"Drawer 3"`.
- **`category:<name>`** — exact, case-insensitive category name match.
- **`has:datasheet`** — parts with at least one manufacturer variant whose
  `datasheet_url` is set.
- **`has:dimensions`** — parts with at least one recorded dimension row.
- **`has:footprint`** — **typed-unsupported.** Footprint/CAD-link data is a
  real, recognized concept — it just isn't modeled yet; CAD links arrive with
  Phase 3. Returns `DbError::UnsupportedSearchKey` rather than silently
  matching nothing, so the UI can distinguish "not yet" from "not a thing."
- **`has:<anything else>`** — `DbError::UnknownSearchKey`.
- **`is:archived`** — restrict to archived parts. The default (no flag)
  *excludes* archived parts.
- **`is:active`** — restrict to non-archived parts (same effect as the
  default, but explicit).
- **`is:low`** — same as the bare `low stock` flag below.
- **`low stock`** (two separate tokens) or **`low-stock`** (one hyphenated
  token) — restrict to parts that both have a low-stock threshold set *and*
  have `available < threshold`. A part with no threshold never matches this
  flag, regardless of how low its stock is.

## Stock-column filters

`available:`, `reserved:`, `checked_out:`, and `stock:` (the sum of all
three) compare against `part_stock`'s milli-unit columns. The filter value is
whole units; it's scaled internally by `Quantity::SCALE` before comparison.
All five comparison operators and range apply:

```
available:<10
reserved:>0
checked_out:>=1
stock:>500
available:5..20
```

## Attribute filters

Any key that isn't one of the reserved words above is looked up against
`attribute_defs.key` first. **The key must match the attribute's key
exactly** — for example `voltage_rating:>=25V`, not the more casual
`voltage:>=25V`. There is no label or alias resolution yet (so a category's
display label, or a shorthand a user might guess, won't resolve); that
friendlier matching is deferred to Phase 3.

- `number_unit` and `range` attributes parse the filter value under the
  attribute's own unit kind (exact-decimal, via `inventory-core::units`) and
  compare against the stored `value_num`. For `range` attributes, only the
  lower bound's `value_num` is consulted — `value_num_hi` is not — a
  documented simplification, not a full interval comparison.
- Every other data type (`text`, `choice`, `multi_choice`, `number` without a
  unit kind, `boolean`) falls back to a case-insensitive equality match on
  `original_text`, regardless of which operator was requested. A `>`/`<`/
  range prefix against a non-numeric attribute silently degrades to plain
  equality rather than erroring.

```
resistance:10k
voltage_rating:>=25V
capacitance:10nF..1uF
```

## Dimension filters

If a key matches neither a reserved word nor an attribute, it's tried as a
(lowercased) dimension name (`dimensions.name` — e.g. `height:`, `width:`,
`weight:`). The value parses as a length first (converted to millimeters,
matching how `dimensions.normalized_value` stores lengths), falling back to
mass (already in grams) if length parsing fails — the same order
`add_dimension` itself uses. If the key matches no attribute and no
dimension anywhere in the database, it's `DbError::UnknownSearchKey`.

```
height:<10mm
height:1mm..8mm
```

## Archived parts and the index

Archived parts are **not** removed from `search_text`/`parts_fts` — they
stay in the FTS index. The archived/active split is applied as a
post-filter over query results instead (default: exclude archived;
`is:archived`: only archived). This keeps `refresh_search_text` simple (no
special-case delete-on-archive) at the cost of the index carrying rows most
queries filter back out. A full FTS rebuild-all utility is deferred to
Phase 7's recovery tooling.

## Determinism

Results are always deterministically ordered: bm25 rank (ascending — a more
negative/lower bm25 score is a better match) when the query had free text,
otherwise display name. Part id is the final tiebreak either way, so a query
with no free text and no distinguishing filter still returns a stable order
across repeated calls.
