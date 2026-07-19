/**
 * TS twin of `crates/inventory-core/src/units.rs`'s electronics-aware unit
 * parser, restricted to what the search bar needs: turning a typed value
 * like `10k`, `4k7`, `0.1uF`, `3V3`, `1/4W`, or `5m` into a plain number in
 * the kind's canonical unit. Locked to the Rust implementation by
 * `packages/shared/fixtures/unit-cases.json`, which `cargo test -p
 * inventory-core` (see `units.rs`'s `cfg(test)` module) also parses — every
 * `cases`/`rejects` entry there must parse identically on both sides.
 *
 * ## Deliberate divergence from the Rust representation
 *
 * `units.rs` keeps values as an exact `(mantissa: i64, exp10: i16)` pair
 * specifically so identity comparisons (hashing, DB storage) never suffer
 * float rounding; `to_f64()` is called out in its module doc as existing
 * ONLY for range filtering. The query-side use case here — rendering a
 * typed search value as a JS `number` for comparison against a snapshot's
 * already-`f64` normalized attribute values — has no such requirement, so
 * this module skips the mantissa/exp10 machinery entirely and computes the
 * canonical value directly as a float (`value * 10**exponent`, or an exact
 * rational division for the `a/b` fraction form). This does not change
 * *which* inputs parse or reject, or their canonical unit — only how the
 * numeric result is represented — so fixture parity (value equality, not
 * bit-for-bit internal representation) holds.
 *
 * ## Two entry points, matching Rust's two public functions
 *
 * - `parseWithKind(text, kind)` mirrors `parse_with_kind`: the kind is
 *   supplied by the caller (in the app, the attribute/dimension key the
 *   query filter's `key:` resolves to against the schema) so `10k`
 *   unambiguously means 10 kΩ once `kind: 'resistance'` is known. This is
 *   the function tested against EVERY fixture case (both `cases` and
 *   `rejects` carry an explicit `kind`), including the kind-disambiguated
 *   pairs added for Phase 6 (`5m` is 5 milliohms as `resistance` but 5
 *   meters as `length` — same input, different kind, different result).
 * - `parseUnitValue(text)` mirrors `detect_and_parse`: kind-blind, for a
 *   bare typed value with no attribute context. Requires an unambiguous
 *   unit symbol in the text (matches exactly one kind); a bare SI-prefixed
 *   number with no unit letter (`10k`, `5m`) is inherently ambiguous across
 *   every kind's milli/kilo/etc. prefix and returns `null`, exactly as
 *   `detect_and_parse` does (see its Rust test:
 *   `detect_and_parse_needs_an_unambiguous_symbol`, "bare prefix is
 *   ambiguous").
 *
 * ## Field mapping
 *
 * `UnitKind`'s string values are identical to Rust's serde output
 * (`#[serde(rename_all = "snake_case")]` on the Rust enum, matching
 * `UnitKind::as_sql()`/`from_sql()`) — `"resistance"`, `"capacitance"`,
 * etc. — so fixture `kind` strings plug directly into this module's
 * `UnitKind` type with no translation.
 */

export type UnitKind =
  | 'resistance'
  | 'capacitance'
  | 'inductance'
  | 'voltage'
  | 'current'
  | 'power'
  | 'frequency'
  | 'length'
  | 'mass'
  | 'time'
  | 'percent'
  | 'charge';

export interface ParsedUnitValue {
  value: number;
  canonicalUnit: string;
}

const CANONICAL_UNIT: Record<UnitKind, string> = {
  resistance: 'Ω',
  capacitance: 'F',
  inductance: 'H',
  voltage: 'V',
  current: 'A',
  power: 'W',
  frequency: 'Hz',
  length: 'm',
  mass: 'g',
  time: 's',
  percent: '%',
  charge: 'C',
};

const ALL_KINDS: readonly UnitKind[] = Object.keys(CANONICAL_UNIT) as UnitKind[];

// U+00B1 PLUS-MINUS SIGN.
const PLUS_MINUS = '±';
// U+00B5 MICRO SIGN and U+03BC GREEK SMALL LETTER MU -- units.rs's
// `prefix_exp`/detection code treats both as the micro prefix.
const MICRO_SIGN = 'µ';
const GREEK_MU = 'μ';
// U+03A9 GREEK CAPITAL LETTER OMEGA -- the ohm symbol used throughout
// units.rs (canonical_unit(), the embedded-marker scan, detect_and_parse's
// has_symbol check).
const OMEGA = 'Ω';

function canonicalUnit(kind: UnitKind): string {
  return CANONICAL_UNIT[kind];
}

/** Mirrors `prefix_exp`: SI prefix letter -> power of ten. */
function prefixExp(c: string): number | null {
  switch (c) {
    case 'p':
      return -12;
    case 'n':
      return -9;
    case 'u':
    case MICRO_SIGN:
    case GREEK_MU:
      return -6;
    case 'm':
      return -3;
    case 'k':
    case 'K':
      return 3;
    case 'M':
      return 6;
    case 'G':
      return 9;
    default:
      return null;
  }
}

/**
 * Mirrors `unit_symbol`: a unit word/symbol -> its `UnitKind`, case
 * insensitive. `String.prototype.toLowerCase()` maps U+2126 OHM SIGN to
 * U+03C9 GREEK SMALL LETTER OMEGA (verified against Node's ICU-backed
 * implementation), exactly like Rust's `char::to_lowercase()` -- so a
 * caller passing the OHM SIGN variant of "Ω" reaches the `ω` arm below with
 * no special-casing needed here.
 */
function unitSymbol(s: string): UnitKind | null {
  switch (s.toLowerCase()) {
    case 'ω': // ω (also reached by U+2126 OHM SIGN via toLowerCase)
    case 'ohm':
    case 'ohms':
    case 'r':
      return 'resistance';
    case 'f':
    case 'farad':
    case 'farads':
      return 'capacitance';
    case 'h':
    case 'henry':
    case 'henries':
      return 'inductance';
    case 'v':
    case 'volt':
    case 'volts':
      return 'voltage';
    case 'a':
    case 'amp':
    case 'amps':
    case 'ampere':
    case 'amperes':
      return 'current';
    case 'w':
    case 'watt':
    case 'watts':
      return 'power';
    case 'hz':
    case 'hertz':
      return 'frequency';
    case 'g':
    case 'gram':
    case 'grams':
      return 'mass';
    case 's':
    case 'sec':
    case 'second':
    case 'seconds':
      return 'time';
    case '%':
      return 'percent';
    case 'c':
    case 'coulomb':
    case 'coulombs':
      return 'charge';
    case 'meter':
    case 'meters':
    case 'metre':
    case 'metres':
      return 'length';
    default:
      return null;
  }
}

/**
 * Mirrors `resolve_suffix`. Returns the power-of-ten adjustment, the
 * `'inches'` sentinel (mirroring Rust's `"__inches__"` error sentinel -- 1
 * in = 25.4 mm is not a power of ten, so the caller folds the ×254 into the
 * mantissa itself), or `null` if `rest` doesn't resolve to a unit
 * compatible with `kind`.
 */
function resolveSuffix(rest: string, kind: UnitKind): number | 'inches' | null {
  if (kind === 'length') {
    const lower = rest.toLowerCase();
    if (lower === 'm') return 0;
    if (lower === 'mm') return -3;
    if (lower === 'cm') return -2;
    if (lower === 'um' || lower === `${MICRO_SIGN}m` || lower === `${GREEK_MU}m`) return -6;
    if (lower === 'in' || rest === '"' || lower === 'inch' || lower === 'inches') return 'inches';
  }
  const whole = unitSymbol(rest);
  if (whole !== null) {
    return whole === kind ? 0 : null;
  }
  const chars = Array.from(rest);
  const p = prefixExp(chars[0] ?? '');
  if (p !== null) {
    const unitPart = chars.slice(1).join('');
    if (unitPart === '') return p;
    const found = unitSymbol(unitPart);
    if (found !== null) {
      return found === kind ? p : null;
    }
  }
  return null;
}

/** Mirrors `split_number`: leading `[+-]?\d*\.?\d*` prefix (must contain at
 * least one digit) split from the rest of the string. */
function splitNumber(s: string): [string, string] | null {
  let end = 0;
  let seenDigit = false;
  let seenDot = false;
  for (let i = 0; i < s.length; i++) {
    const c = s[i] as string; // i < s.length, always defined
    if (c >= '0' && c <= '9') {
      seenDigit = true;
      end = i + 1;
    } else if (c === '.' && !seenDot) {
      seenDot = true;
      end = i + 1;
    } else if ((c === '-' || c === '+') && i === 0) {
      end = i + 1;
    } else {
      break;
    }
  }
  if (!seenDigit) return null;
  return [s.slice(0, end), s.slice(end)];
}

/** Rust's `char::is_alphabetic()` scan target for embedded-marker
 * detection, extended (matching the Rust source) with µ/μ/Ω explicitly --
 * already covered by `\p{L}` but listed for parity with `try_embedded`. */
function isAlphaLike(c: string): boolean {
  return /\p{L}/u.test(c) || c === MICRO_SIGN || c === GREEK_MU || c === OMEGA;
}

type StrategyResult = { matched: true; value: number } | { matched: false } | null; // null = pattern doesn't apply here; caller tries the next strategy.

/**
 * Mirrors `try_embedded`: `<digits><marker><digits?>` notation (`4k7`,
 * `3V3`, `0R`, `2n2`, and the length-specific `5m`/`5m5` where a bare `m`
 * marker means meters, not milli). A marker that resolves to a *different*
 * kind's unit (e.g. embedded-form `V` seen while parsing as `resistance`)
 * is a hard failure (`{ matched: false }`) -- mirrors `Err(WrongKind)`
 * propagating past the `?` in Rust rather than falling through to the
 * fraction/general strategies.
 */
function tryEmbedded(s: string, kind: UnitKind): StrategyResult {
  const chars = Array.from(s);
  let i = -1;
  for (let idx = 0; idx < chars.length; idx++) {
    if (isAlphaLike(chars[idx] as string)) {
      i = idx;
      break;
    }
  }
  if (i <= 0) return null;

  const intPart = chars.slice(0, i).join('');
  if (!/^\d+$/.test(intPart)) return null;

  const marker = chars[i] as string; // 0 < i < chars.length, always defined
  const fracPart = chars.slice(i + 1).join('');
  if (fracPart !== '' && !/^\d+$/.test(fracPart)) return null;

  let markerExp: number | null;
  if (kind === 'length' && marker === 'm') {
    markerExp = 0;
  } else {
    const p = prefixExp(marker);
    if (p !== null) {
      markerExp = p;
    } else {
      const found = unitSymbol(marker);
      if (found === null) {
        markerExp = null;
      } else if (found === kind) {
        markerExp = 0;
      } else {
        return { matched: false }; // WrongKind
      }
    }
  }
  if (markerExp === null) return null;

  const value =
    fracPart === ''
      ? Number(intPart) * Math.pow(10, markerExp)
      : Number(`${intPart}.${fracPart}`) * Math.pow(10, markerExp);
  return { matched: true, value };
}

function gcd(a: number, b: number): number {
  a = Math.abs(a);
  b = Math.abs(b);
  while (b !== 0) {
    [a, b] = [b, a % b];
  }
  return a || 1;
}

/** A rational `num/den` terminates in base 10 iff, once reduced to lowest
 * terms, the denominator's only prime factors are 2 and 5. Mirrors the
 * effect of `try_fraction`'s iterative mantissa-scaling loop (which
 * rejects non-terminating expansions like 1/3) without reproducing its
 * exact-rational bookkeeping -- see the module doc's float-representation
 * note. */
function isTerminatingDecimal(num: number, den: number): boolean {
  const g = gcd(num, den);
  let d = Math.abs(den) / g;
  while (d % 2 === 0) d /= 2;
  while (d % 5 === 0) d /= 5;
  return d === 1;
}

/** Mirrors `try_fraction`: `<num>/<den> [unit]`, including the exact
 * 25.4 mm/inch fold and the non-terminating-fraction rejection. */
function tryFraction(s: string, kind: UnitKind): StrategyResult {
  const slash = s.indexOf('/');
  if (slash === -1) return null;

  const numPart = s.slice(0, slash).trim();
  if (!/^[+-]?\d+$/.test(numPart)) return { matched: false };
  const num = Number(numPart);

  const split = splitNumber(s.slice(slash + 1).trim());
  if (!split) return { matched: false };
  const [denStr, unitRestRaw] = split;
  const den = Number(denStr);
  if (den === 0) return { matched: false };

  let numAdj = num;
  let unitExp = 0;
  const unitRest = unitRestRaw.trim();
  if (unitRest !== '') {
    const resolved = resolveSuffix(unitRest, kind);
    if (resolved === null) return { matched: false };
    if (resolved === 'inches') {
      numAdj = num * 254;
      unitExp = -4;
    } else {
      unitExp = resolved;
    }
  }

  if (!isTerminatingDecimal(numAdj, den)) return { matched: false };
  return { matched: true, value: (numAdj / den) * Math.pow(10, unitExp) };
}

/** Mirrors the general `<decimal> [prefix][unit]` path -- the last-resort
 * strategy; unlike the embedded/fraction strategies it never "doesn't
 * apply," it either succeeds or the whole parse fails. */
function generalParse(s: string, kind: UnitKind): number | null {
  const split = splitNumber(s);
  if (!split) return null;
  const [numStr, restRaw] = split;
  const base = Number(numStr);
  if (!Number.isFinite(base)) return null;

  const rest = restRaw.trim();
  if (rest === '') return base;

  const resolved = resolveSuffix(rest, kind);
  if (resolved === null) return null;
  if (resolved === 'inches') return base * 254 * Math.pow(10, -4);
  return base * Math.pow(10, resolved);
}

/**
 * Mirrors `parse_with_kind`: parse `text` as a value of the given
 * `UnitKind`. Accepts plain decimals, fractions, SI-prefixed/unit-suffixed
 * forms, and embedded-prefix notation -- see the module doc. Returns
 * `null` for anything `parse_with_kind` would return `Err` for (empty,
 * malformed, wrong-kind unit, non-terminating fraction).
 */
export function parseWithKind(text: string, kind: UnitKind): ParsedUnitValue | null {
  let s = text.trim();
  s = s.replace(new RegExp(`^${PLUS_MINUS}+`, 'u'), '').trim();
  if (s === '') return null;

  const embedded = tryEmbedded(s, kind);
  if (embedded !== null) {
    return embedded.matched ? { value: embedded.value, canonicalUnit: canonicalUnit(kind) } : null;
  }

  const fraction = tryFraction(s, kind);
  if (fraction !== null) {
    return fraction.matched ? { value: fraction.value, canonicalUnit: canonicalUnit(kind) } : null;
  }

  const value = generalParse(s, kind);
  return value === null ? null : { value, canonicalUnit: canonicalUnit(kind) };
}

/**
 * Mirrors `detect_and_parse`: kind-blind parse for a bare typed value with
 * no attribute context (e.g. a free-text search term, not a `key:value`
 * filter). Requires an unambiguous unit symbol -- succeeds only when
 * exactly one `UnitKind` parses `text` successfully; a bare SI prefix with
 * no unit letter matches every kind's prefix rule and is therefore
 * ambiguous (`null`), same as Rust's `100n`/`10k` test cases.
 */
export function parseUnitValue(text: string): ParsedUnitValue | null {
  const s = text.trim();
  const hasSymbol = Array.from(s).some(
    (c) => isAlphaLike(c) || c === OMEGA || c === '%' || c === MICRO_SIGN || c === GREEK_MU,
  );
  if (!hasSymbol) return null;

  let hitKind: UnitKind | null = null;
  let hitValue = 0;
  for (const kind of ALL_KINDS) {
    const parsed = parseWithKind(s, kind);
    if (parsed !== null) {
      if (hitKind === null) {
        hitKind = kind;
        hitValue = parsed.value;
      } else if (hitKind !== kind) {
        return null; // ambiguous across kinds
      }
    }
  }
  return hitKind === null ? null : { value: hitValue, canonicalUnit: canonicalUnit(hitKind) };
}
