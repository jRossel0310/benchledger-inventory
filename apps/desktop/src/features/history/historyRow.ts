/** Shared per-row display helper for the History screen (Phase 3 Task 9),
 * used by both flat rows (`History.tsx`) and group members (`GroupRow.tsx`)
 * — the same state-transition rendering `PartDetailTransactions.tsx` uses
 * for `TransactionRecord`, restated here for `HistoryRow`'s identical
 * `from_state`/`to_state` shape. */

import type { HistoryRow } from '../../bindings.gen';

export function stateTransition(row: Pick<HistoryRow, 'from_state' | 'to_state'>): string {
  if (row.from_state && row.to_state) return `${row.from_state} → ${row.to_state}`;
  if (row.to_state) return `→ ${row.to_state}`;
  if (row.from_state) return `${row.from_state} →`;
  return '—';
}
