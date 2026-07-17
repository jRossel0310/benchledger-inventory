/**
 * Shared display formatters. Quantities and prices cross the IPC boundary as
 * exact fixed-point integers (milli-units, currency micros) — these are the
 * only place raw milli/micro values get turned into text; nothing else in
 * the UI should format them ad hoc. See
 * docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md.
 */

import type { CommandError } from '../bindings.gen';

/** Known `QuantityUnit` display abbreviations, plus the raw serde values
 * ("each" | "meter" | "foot") the bindings actually carry — both are
 * accepted so callers can pass either `part.quantity_unit` or a display
 * abbreviation directly. */
const UNIT_SUFFIX: Record<string, string> = {
  each: '',
  meter: 'm',
  foot: 'ft',
};

/**
 * Render an exact milli-unit integer as `<int>` (`each`, no fractional part
 * is ever legal) or `<int>[.<fraction>]` (continuous units) without any
 * floating-point rounding: the milli value is split into integer and
 * fractional parts directly rather than dividing by 1000 first.
 */
function formatMilli(milli: number): string {
  const sign = milli < 0 ? '-' : '';
  const abs = Math.abs(milli);
  const whole = Math.floor(abs / 1000);
  const frac = abs % 1000;
  if (frac === 0) return `${sign}${whole}`;
  const fracStr = String(frac).padStart(3, '0').replace(/0+$/, '');
  return `${sign}${whole}.${fracStr}`;
}

/** Format a milli-unit `Quantity` for display: whole units for `each`
 * (`1000` -> `"1"`), decimals with a unit suffix for continuous units
 * (`1500` meters -> `"1.5 m"`). Accepts either the bindings' `QuantityUnit`
 * values (`"each" | "meter" | "foot"`) or a display abbreviation
 * (`"m" | "ft"`) directly. */
export function formatQuantity(milli: number, unit: string): string {
  const suffix = unit in UNIT_SUFFIX ? UNIT_SUFFIX[unit] : unit;
  const number = formatMilli(milli);
  return suffix ? `${number} ${suffix}` : number;
}

const CURRENCY_SYMBOLS: Record<string, string> = {
  USD: '$',
  EUR: '€',
  GBP: '£',
  JPY: '¥',
};

/** Format a currency-micros integer (1_000_000 micros = 1 whole currency
 * unit) as a two-decimal price with a currency symbol/prefix. `null` micros
 * (no price on file) renders as an em dash. */
export function formatPrice(micros: number | null, currency: string | null): string {
  if (micros === null) return '—';
  const amount = (micros / 1_000_000).toFixed(2);
  if (!currency) return amount;
  const symbol = CURRENCY_SYMBOLS[currency];
  return symbol ? `${symbol}${amount}` : `${currency} ${amount}`;
}

/** Render a SQLite UTC timestamp (`"YYYY-MM-DD HH:MM:SS"`, no zone suffix)
 * or a full ISO-8601 string as a readable string in the viewer's local
 * time zone. Falls back to the raw input when it can't be parsed. */
export function formatTimestamp(iso: string): string {
  const hasZone = /[zZ]|[+-]\d\d:\d\d$/.test(iso);
  const normalized = hasZone ? iso : `${iso.replace(' ', 'T')}Z`;
  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}

/** Pull a human-readable message out of anything a failed command or query
 * can throw: a `CommandError`-shaped object (`{code, message}`), a native
 * `Error`, or anything else (stringified as a last resort). */
export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (
    e !== null &&
    typeof e === 'object' &&
    'message' in e &&
    typeof (e as { message: unknown }).message === 'string'
  ) {
    return (e as { message: string }).message;
  }
  return String(e);
}

/** Recovery hints for `CommandError.code` values the UI knows how to guide
 * the user through. Unknown codes return `null` — the caller shows the
 * error message alone, never a raw code with no hint. Keyed by the
 * snake_case `DbError` variant names from `commands.rs`. */
const ERROR_HINTS: Record<string, string> = {
  insufficient_stock:
    'Not enough stock is available for this action — receive more or lower the quantity.',
  part_not_found: 'This part no longer exists — it may have been deleted elsewhere.',
  part_archived:
    'This part is archived. Only release, return, and reversals are allowed until it is restored.',
  unit_change_blocked: 'The quantity unit can’t change once this part has transactions.',
  already_reversed: 'This transaction or group was already reversed.',
  cannot_reverse_reversal:
    'A reversal can’t itself be reversed — reverse the original transaction instead.',
  transaction_in_group: 'This transaction belongs to a group — reverse the whole group instead.',
  empty_group: 'A transaction group needs at least one operation.',
  group_not_found: 'That transaction group no longer exists.',
  transaction_not_found: 'That transaction no longer exists.',
  project_not_found: 'That project no longer exists.',
  variant_not_found: 'That manufacturer variant no longer exists.',
  category_name_taken: 'That category name is already in use — choose a different name.',
  attribute_key_taken: 'That attribute key is already in use — choose a different key.',
  attribute_not_found: 'That attribute is not defined.',
  category_not_found: 'That category no longer exists.',
  invalid_attribute_value: 'That value isn’t valid for this attribute.',
  invalid_dimension: 'That dimension value could not be read — check the number and unit.',
  dimension_not_found: 'That dimension no longer exists.',
  alias_taken: 'That alias is already registered to another part.',
  invalid_bin_label: 'Enter a bin label, or leave it empty to unassign.',
  newer_schema:
    'This database was created by a newer version of the app — update the app to open it.',
};

/** A short recovery hint for a `CommandError.code`, or `null` for codes the
 * UI has no specific guidance for (the message alone is shown). */
export function errorHint(code: string): string | null {
  return ERROR_HINTS[code] ?? null;
}

/** Human-readable labels for `TransactionRecord`/`RecentTxn`'s `txn_type`
 * (the snake_case `LedgerOp`/reversal variant name) — shared by the
 * Dashboard's recent-activity feed and the part-detail Transactions tab so
 * the same ledger event always reads the same way everywhere it appears. */
const TXN_TYPE_LABELS: Record<string, string> = {
  receive: 'Received',
  reserve: 'Reserved',
  release_reservation: 'Released reservation',
  check_out: 'Checked out',
  return: 'Returned',
  consume_available: 'Consumed',
  consume_reserved: 'Consumed from reserved',
  consume_checked_out: 'Consumed from checked out',
  adjust_up: 'Adjusted up',
  adjust_down: 'Adjusted down',
  transfer_reservation: 'Transferred reservation',
  reverse: 'Reversed',
};

/** A ledger transaction's `txn_type` as a human label, falling back to the
 * raw string for any type the UI doesn't have copy for yet (never blank). */
export function formatTxnType(txnType: string): string {
  return TXN_TYPE_LABELS[txnType] ?? txnType;
}

// Re-exported so `errorMessage`/`errorHint` callers can type a caught value
// as `CommandError` when they already know its shape (e.g. from `unwrap`).
export type { CommandError };
