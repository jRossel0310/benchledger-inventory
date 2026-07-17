/**
 * Pure "flow to op" mapping for the Ctrl+K quick-action dialog
 * (`QuickAction.tsx`): which `LedgerOp` variant each quick action builds,
 * whether it needs a project, and the toast copy naming its effect (design
 * voice: "Received 10", never "Submitted successfully" — see
 * docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md). Kept
 * free of React so the entire flow-to-op mapping is unit-testable without
 * rendering anything.
 *
 * Deliberately covers only the six actions the design direction's Ctrl+K
 * spine and this task's quick-action flows name (Add stock, Consume,
 * Reserve, Release reservation, Check out, Return) — `ConsumeReserved`/
 * `ConsumeCheckedOut`/`AdjustUp`/`AdjustDown`/`TransferReservation` are
 * deeper ledger operations for the part-detail/history screens, not the
 * seconds-fast Ctrl+K path.
 */

import type { LedgerOp, PartId, ProjectId } from '../../bindings.gen';
import { formatQuantity } from '../../lib/format';

export type QuickActionKind =
  'receive' | 'consume_available' | 'reserve' | 'release_reservation' | 'check_out' | 'return';

/** `none`: the op has no project field at all (Receive). `optional`: the op
 * accepts a nullable project (ConsumeAvailable). `required`: the ledger
 * rejects the op without one (Reserve/ReleaseReservation/CheckOut/Return —
 * see `LedgerOp` in `bindings.gen.ts`, whose `project_id` is non-optional
 * for exactly these four variants). */
export type ProjectRequirement = 'none' | 'optional' | 'required';

export interface QuickActionConfig {
  kind: QuickActionKind;
  /** Palette entry / dialog title — names the effect, not "Submit". */
  label: string;
  /** Dialog confirm-button label. */
  submitLabel: string;
  project: ProjectRequirement;
  /** Whether this op takes a free-text reason note. */
  hasNote: boolean;
}

export const QUICK_ACTIONS: QuickActionConfig[] = [
  {
    kind: 'receive',
    label: 'Add stock',
    submitLabel: 'Add stock',
    project: 'none',
    hasNote: true,
  },
  {
    kind: 'consume_available',
    label: 'Consume',
    submitLabel: 'Consume',
    project: 'optional',
    hasNote: true,
  },
  {
    kind: 'reserve',
    label: 'Reserve for project',
    submitLabel: 'Reserve',
    project: 'required',
    hasNote: false,
  },
  {
    kind: 'release_reservation',
    label: 'Release reservation',
    submitLabel: 'Release',
    project: 'required',
    hasNote: false,
  },
  {
    kind: 'check_out',
    label: 'Check out',
    submitLabel: 'Check out',
    project: 'required',
    hasNote: false,
  },
  {
    kind: 'return',
    label: 'Return',
    submitLabel: 'Return',
    project: 'required',
    hasNote: false,
  },
];

const BY_KIND: Record<QuickActionKind, QuickActionConfig> = Object.fromEntries(
  QUICK_ACTIONS.map((a) => [a.kind, a]),
) as Record<QuickActionKind, QuickActionConfig>;

export function quickActionConfig(kind: QuickActionKind): QuickActionConfig {
  const found = BY_KIND[kind];
  if (!found) throw new Error(`unknown quick action kind: ${String(kind)}`);
  return found;
}

export interface BuildLedgerOpInput {
  kind: QuickActionKind;
  partId: PartId;
  /** Exact milli-unit integer — never a floating display value. */
  quantityMilli: number;
  /** Ignored for project-only ops (Reserve/ReleaseReservation/CheckOut/
   * Return), which carry no note field on the wire. */
  note: string;
  projectId: ProjectId | null;
}

/** Builds the exact `LedgerOp` tagged union the ledger backend expects.
 * Throws for a project-required op given no project rather than silently
 * sending a request the backend would reject anyway — callers (the dialog's
 * submit handler) are expected to have disabled submission in that case, so
 * reaching this throw means a caller bug, not a normal user path. */
export function buildLedgerOp({
  kind,
  partId,
  quantityMilli,
  note,
  projectId,
}: BuildLedgerOpInput): LedgerOp {
  switch (kind) {
    case 'receive':
      return { type: 'receive', part_id: partId, quantity: quantityMilli, note };
    case 'consume_available':
      return {
        type: 'consume_available',
        part_id: partId,
        quantity: quantityMilli,
        project_id: projectId,
        note,
      };
    case 'reserve':
      if (!projectId) throw new Error('reserve requires a project');
      return { type: 'reserve', part_id: partId, quantity: quantityMilli, project_id: projectId };
    case 'release_reservation':
      if (!projectId) throw new Error('release_reservation requires a project');
      return {
        type: 'release_reservation',
        part_id: partId,
        quantity: quantityMilli,
        project_id: projectId,
      };
    case 'check_out':
      if (!projectId) throw new Error('check_out requires a project');
      return { type: 'check_out', part_id: partId, quantity: quantityMilli, project_id: projectId };
    case 'return':
      if (!projectId) throw new Error('return requires a project');
      return { type: 'return', part_id: partId, quantity: quantityMilli, project_id: projectId };
  }
}

/** The toast title naming the op's effect ("Received 10", "Reserved 5 for
 * Blinky Board") — the design direction's copy voice: actions name their
 * effect, never "Submitted successfully". `projectName` is appended for
 * project-scoped ops when the project is known; omitted (not "for
 * undefined") when it isn't. */
export function quickActionToastTitle(
  kind: QuickActionKind,
  quantityMilli: number,
  unit: string,
  projectName?: string | null,
): string {
  const qty = formatQuantity(quantityMilli, unit);
  switch (kind) {
    case 'receive':
      return `Received ${qty}`;
    case 'consume_available':
      return `Consumed ${qty}`;
    case 'reserve':
      return projectName ? `Reserved ${qty} for ${projectName}` : `Reserved ${qty}`;
    case 'release_reservation':
      return projectName ? `Released ${qty} from ${projectName}` : `Released ${qty}`;
    case 'check_out':
      return projectName ? `Checked out ${qty} for ${projectName}` : `Checked out ${qty}`;
    case 'return':
      return projectName ? `Returned ${qty} from ${projectName}` : `Returned ${qty}`;
  }
}

export interface ReceiveDetails {
  note: string;
  supplier: string;
  order: string;
  date: string;
  cost: string;
}

/** Folds the Add-stock dialog's optional "Add details" fields (supplier,
 * order, date, cost) and the free-text note into the one `note` string
 * `LedgerOp::Receive` actually persists. The ledger has no dedicated
 * supplier/order/date/cost columns on a receive transaction — that
 * structured purchasing data is Phase 5's Orders domain — so rather than
 * collecting it and silently discarding it, or inventing unpersisted
 * fields, every filled field is folded into the one text channel the wire
 * format supports, in a stable `Key: value · ...` order ending with the
 * free-text note. Blank/whitespace-only fields are omitted entirely rather
 * than appearing as `Supplier: `. */
export function composeReceiveNote({ note, supplier, order, date, cost }: ReceiveDetails): string {
  const parts: string[] = [];
  if (supplier.trim()) parts.push(`Supplier: ${supplier.trim()}`);
  if (order.trim()) parts.push(`Order: ${order.trim()}`);
  if (date.trim()) parts.push(`Date: ${date.trim()}`);
  if (cost.trim()) parts.push(`Cost: ${cost.trim()}`);
  if (note.trim()) parts.push(note.trim());
  return parts.join(' · ');
}
