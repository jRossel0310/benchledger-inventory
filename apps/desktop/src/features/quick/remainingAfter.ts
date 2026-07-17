/**
 * The QuickAction dialog's live "remaining after" preview (design direction
 * §9: "Consume 5 from 40 available → '35 available after'") and its
 * client-side over-draw early warning. Pure milli-unit integer arithmetic
 * against the part's current `PartStockRow` (from `useStock`) — no
 * floating-point rounding, and no network round trip, so the preview
 * updates on every keystroke. This is a UX hint only: the ledger backend
 * remains the authoritative check (`InsufficientStock`), surfaced inline by
 * `QuickAction.tsx` when a submit fails anyway (e.g. a concurrent change
 * between typing and confirming).
 */

import type { PartStockRow } from '../../bindings.gen';
import { formatQuantity } from '../../lib/format';
import type { QuickActionKind } from './quickActionConfig';

export interface RemainingAfter {
  available: number;
  reserved: number;
  checkedOut: number;
}

/** Which stock pool each op subtracts from (`from`) and adds to (`to`),
 * `null` when the op has no such side (Receive has no source; Consume* has
 * no destination — the quantity leaves the system entirely). Single source
 * of truth shared by `previewAfter` and `wouldGoNegative` so they can never
 * disagree about which pool an action touches. */
function poolsFor(kind: QuickActionKind): {
  from: keyof RemainingAfter | null;
  to: keyof RemainingAfter | null;
} {
  switch (kind) {
    case 'receive':
      return { from: null, to: 'available' };
    case 'consume_available':
      return { from: 'available', to: null };
    case 'reserve':
      return { from: 'available', to: 'reserved' };
    case 'release_reservation':
      return { from: 'reserved', to: 'available' };
    case 'check_out':
      return { from: 'available', to: 'checkedOut' };
    case 'return':
      return { from: 'checkedOut', to: 'available' };
  }
}

function currentPools(stock: PartStockRow): RemainingAfter {
  return { available: stock.available, reserved: stock.reserved, checkedOut: stock.checked_out };
}

/** The three stock pools as they would read immediately after applying
 * `quantityMilli` of `kind` to `stock` — exact milli-unit integer math, no
 * rounding. Does not clamp at zero: a negative result is a real signal
 * (see `wouldGoNegative`), not something to hide from the preview. */
export function previewAfter(
  kind: QuickActionKind,
  stock: PartStockRow,
  quantityMilli: number,
): RemainingAfter {
  const next = currentPools(stock);
  const { from, to } = poolsFor(kind);
  if (from) next[from] -= quantityMilli;
  if (to) next[to] += quantityMilli;
  return next;
}

/** Whether this op would draw its source pool below zero — the dialog uses
 * this to show an inline warning and hold off enabling submit before the
 * user ever reaches the backend's `InsufficientStock` error. Always `false`
 * for ops with no source pool (Receive). */
export function wouldGoNegative(
  kind: QuickActionKind,
  stock: PartStockRow,
  quantityMilli: number,
): boolean {
  const { from } = poolsFor(kind);
  if (!from) return false;
  return currentPools(stock)[from] - quantityMilli < 0;
}

const POOL_ORDER: (keyof RemainingAfter)[] = ['available', 'reserved', 'checkedOut'];
const POOL_LABEL: Record<keyof RemainingAfter, string> = {
  available: 'available',
  reserved: 'reserved',
  checkedOut: 'checked out',
};

/** The dialog's live preview text: every pool this op touches (one for
 * Receive/Consume, two for Reserve/Release/CheckOut/Return), always ordered
 * available → reserved → checked-out to match the stock gauge's canonical
 * segment order, each formatted in the part's display unit. */
export function formatRemainingAfter(
  kind: QuickActionKind,
  stock: PartStockRow,
  quantityMilli: number,
  unit: string,
): string {
  const after = previewAfter(kind, stock, quantityMilli);
  const { from, to } = poolsFor(kind);
  const touched = new Set<keyof RemainingAfter>();
  if (from) touched.add(from);
  if (to) touched.add(to);

  const parts = POOL_ORDER.filter((pool) => touched.has(pool)).map(
    (pool) => `${formatQuantity(after[pool], unit)} ${POOL_LABEL[pool]}`,
  );
  return `${parts.join(', ')} after`;
}
