/**
 * The import review's per-line table (Phase 5d Task 3, extended in Task 4
 * with a Bin column and the create-from-line dialog trigger): line #, item
 * identity, ordered/shipped/backordered quantities (shipped highlighted as
 * the actual receive quantity — spec §10's "shipped, never ordered"), unit
 * price, top match verdict, current decision, target/draft bin, and a
 * "Change…" trigger (`LineActionEditor`) for `part`-kind lines only. Non-
 * `part` kinds (fee/tariff/no_charge/unknown) render greyed with their kind
 * badge and no action editor — they never create inventory (`ProposedAction
 * ::NonInventory`/`Ignore`), so there is nothing to decide.
 *
 * A plain (non-virtualized) `<table>`, not `DataTable.tsx`: `DataTable` is
 * built for the 10k-row Inventory browser and needs a fixed per-row pixel
 * height, which doesn't fit this screen's variable-height rows (a warning
 * line, a two-line item cell) at the handful-to-dozens of lines one order
 * actually has — the same "no gauge/DataTable forcing" reasoning
 * `BomTable.tsx`'s module doc comment gives for its own dense-table choice.
 *
 * Bin column (Task 4, spec §10): `add_stock`/`add_as_variant` decisions
 * target an EXISTING part, whose current bin isn't on `MatchResult` (only
 * `part_id`/`display_name`/`verdict_kind`/`explanation`/`rank`) — so this
 * resolves it with a per-row `usePart(partId)`, the same "fine at invoice
 * scale" fallback `DecisionSummary`'s `ResolvedPartDecision` already uses for
 * the target's display name below. `create_new` shows the draft's own
 * `bin_label` (editable only via `CreateFromLineDialog` — this cell is
 * read-only display; the Decision cell's "Complete draft"/"Edit draft"
 * trigger is the one path to change it, so bin editing has exactly one
 * entry point rather than two that could drift). `skip` and non-`part` lines
 * show an em dash — neither creates or touches a part.
 *
 * `disabled` (Task 4) freezes every control in the table — the trigger,
 * the "Complete draft"/"Edit draft" button, and closes any open dialog —
 * once `ImportReview.tsx` reports the import is no longer `parsed` (already
 * committed or reversed): a decision made against a committed import can
 * never actually apply, so editing it would silently lie about what's still
 * changeable.
 */

import { useState } from 'react';

import type {
  ImportLineId,
  ImportReviewLine,
  LineDecision,
  MatchResult,
  PartId,
} from '../../bindings.gen';
import { usePart } from '../../hooks/inventory';
import { formatPrice, formatQuantity } from '../../lib/format';
import { verdictLabel } from '../part/DuplicatePanel';
import { CreateFromLineDialog } from './CreateFromLineDialog';
import { LineActionEditor } from './LineActionEditor';
import { isCreateNewIncomplete, type LineDecisionContext } from './lineDecisions';
import './ReviewLineTable.css';

const QTY_UNIT = 'each'; // ImportReviewLine carries no quantity_unit — the same documented gap BomTable.tsx lives with.

const KIND_LABELS: Record<string, string> = {
  fee: 'Fee',
  tariff: 'Tariff',
  no_charge: 'No charge',
  unknown: 'Unrecognized',
};

function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

function topMatch(matches: MatchResult[]): MatchResult | undefined {
  if (matches.length === 0) return undefined;
  return matches.slice().sort((a, b) => a.rank - b.rank)[0];
}

export interface ReviewLineTableProps {
  lines: ImportReviewLine[];
  decisions: Map<ImportLineId, LineDecision>;
  /** Reasons a line's decision fell back to the unmappable-proposal default
   * (`lineDecisions.ts`'s `decisionFromProposed`) — distinct from the
   * backend's own `line.warning`; both render inline, whichever apply. */
  decisionWarnings: Map<ImportLineId, string>;
  onChangeDecision: (lineId: ImportLineId, decision: LineDecision) => void;
  context: LineDecisionContext;
  /** True once the import is no longer `parsed` (committed or reversed) —
   * freezes every editor in the table. Defaults `false` so every existing
   * caller/test (pre-Task 4) keeps working unchanged. */
  disabled?: boolean;
}

export function ReviewLineTable({
  lines,
  decisions,
  decisionWarnings,
  onChangeDecision,
  context,
  disabled = false,
}: ReviewLineTableProps) {
  const [editingLineId, setEditingLineId] = useState<ImportLineId | null>(null);
  const editingLine = lines.find((line) => line.line_id === editingLineId);
  const editingDecision = editingLineId ? decisions.get(editingLineId) : undefined;

  function openDraftDialog(lineId: ImportLineId) {
    if (disabled) return;
    setEditingLineId(lineId);
  }

  function saveDraft(decision: LineDecision) {
    if (editingLineId) onChangeDecision(editingLineId, decision);
    setEditingLineId(null);
  }

  return (
    <>
      <table className="review-line-table" aria-label="Import lines">
        <thead>
          <tr>
            <th scope="col">#</th>
            <th scope="col">Item</th>
            <th scope="col">Ordered</th>
            <th scope="col">Shipped</th>
            <th scope="col">Backordered</th>
            <th scope="col">Unit price</th>
            <th scope="col">Match</th>
            <th scope="col">Decision</th>
            <th scope="col">Bin</th>
            <th scope="col" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          {lines.map((line) => {
            const isPart = line.kind === 'part';
            const decision = decisions.get(line.line_id);
            const warning = line.warning ?? decisionWarnings.get(line.line_id) ?? null;
            return (
              <tr
                key={line.line_id}
                className={`review-line-table-row${isPart ? '' : ' review-line-table-row--non-part'}`}
              >
                <td className="review-line-table-cell review-line-table-cell-num">
                  {line.line_number ?? '—'}
                </td>
                <td className="review-line-table-cell review-line-table-cell-item">
                  <div className="review-line-table-item-id">
                    {line.supplier_sku ?? line.mpn ?? '—'}
                  </div>
                  <div className="review-line-table-item-meta">
                    {[line.manufacturer, line.mpn, line.description].filter(Boolean).join(' · ') ||
                      '—'}
                  </div>
                  {warning ? <div className="review-line-table-warning">{warning}</div> : null}
                </td>
                <td className="review-line-table-cell review-line-table-cell-qty">
                  {line.ordered_milli !== null ? formatQuantity(line.ordered_milli, QTY_UNIT) : '—'}
                </td>
                <td className="review-line-table-cell review-line-table-cell-qty review-line-table-cell-shipped">
                  {line.receive_qty_milli !== null
                    ? formatQuantity(line.receive_qty_milli, QTY_UNIT)
                    : isPart
                      ? 'Not received'
                      : '—'}
                </td>
                <td className="review-line-table-cell review-line-table-cell-qty">
                  {line.backordered_milli !== null && line.backordered_milli > 0
                    ? formatQuantity(line.backordered_milli, QTY_UNIT)
                    : '—'}
                </td>
                <td className="review-line-table-cell review-line-table-cell-qty">
                  {formatPrice(line.unit_price_micros, context.currency)}
                </td>
                <td className="review-line-table-cell">
                  {isPart ? (
                    <MatchCell match={topMatch(line.matches)} />
                  ) : (
                    <span className="review-line-table-kind">{kindLabel(line.kind)}</span>
                  )}
                </td>
                <td className="review-line-table-cell">
                  {isPart && decision ? (
                    <DecisionSummary
                      decision={decision}
                      matches={line.matches}
                      disabled={disabled}
                      onEditDraft={() => openDraftDialog(line.line_id)}
                    />
                  ) : (
                    <span className="review-line-table-non-part-label">Not inventory</span>
                  )}
                </td>
                <td className="review-line-table-cell">
                  {isPart && decision ? <BinCell decision={decision} /> : '—'}
                </td>
                <td className="review-line-table-cell review-line-table-cell-actions">
                  {isPart && decision ? (
                    <LineActionEditor
                      line={line}
                      decision={decision}
                      onChange={(next) => onChangeDecision(line.line_id, next)}
                      context={context}
                      disabled={disabled}
                    />
                  ) : null}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      {editingLine && editingDecision && editingDecision.type === 'create_new' && !disabled ? (
        <CreateFromLineDialog
          line={editingLine}
          decision={editingDecision}
          onSave={saveDraft}
          onCancel={() => setEditingLineId(null)}
        />
      ) : null}
    </>
  );
}

interface MatchCellProps {
  match: MatchResult | undefined;
}

function MatchCell({ match }: MatchCellProps) {
  if (!match) {
    return <span className="review-line-table-match review-line-table-match--none">No match</span>;
  }
  return (
    <span className="review-line-table-match" title={match.explanation}>
      <span className="review-line-table-match-verdict">{verdictLabel(match.verdict_kind)}</span>
      <span className="review-line-table-match-explanation">{match.explanation}</span>
    </span>
  );
}

interface DecisionSummaryProps {
  decision: LineDecision;
  matches: MatchResult[];
  disabled: boolean;
  onEditDraft: () => void;
}

/** The current decision's human label. For `add_stock`/`add_as_variant`,
 * resolves the target part's display name — from the line's own `matches`
 * when the target is one of them (no extra query), otherwise via `usePart`
 * (the same per-row resolve-by-id fallback `BomTable.tsx`'s
 * `SubstituteChip` uses for a substitute picked from outside the visible
 * list). A `create_new` decision gets a "Complete draft"/"Edit draft" button
 * (Task 4) — the only way to open `CreateFromLineDialog` — instead of the
 * old static "Draft incomplete" text-only flag. */
function DecisionSummary({ decision, matches, disabled, onEditDraft }: DecisionSummaryProps) {
  switch (decision.type) {
    case 'add_stock':
      return (
        <ResolvedPartDecision
          label="Add stock to"
          partId={decision.part_id}
          knownName={matches.find((m) => m.part_id === decision.part_id)?.display_name}
        />
      );
    case 'add_as_variant':
      return (
        <ResolvedPartDecision
          label="Add as variant to"
          partId={decision.part_id}
          knownName={matches.find((m) => m.part_id === decision.part_id)?.display_name}
        />
      );
    case 'create_new': {
      const incomplete = isCreateNewIncomplete(decision);
      return (
        <span className="review-line-table-decision">
          Create new part
          <button
            type="button"
            className={`review-line-table-decision-flag${
              incomplete ? ' review-line-table-decision-flag--incomplete' : ''
            }`}
            onClick={onEditDraft}
            disabled={disabled}
          >
            {incomplete ? 'Draft incomplete' : 'Edit draft'}
          </button>
        </span>
      );
    }
    case 'skip':
      return (
        <span className="review-line-table-decision review-line-table-decision--skip">Skip</span>
      );
    default:
      return null;
  }
}

interface ResolvedPartDecisionProps {
  label: string;
  partId: PartId;
  knownName: string | undefined;
}

function ResolvedPartDecision({ label, partId, knownName }: ResolvedPartDecisionProps) {
  const partQuery = usePart(knownName === undefined ? partId : undefined);
  const name = knownName ?? partQuery.data?.display_name ?? partId;
  return (
    <span className="review-line-table-decision">
      {label} {name}
    </span>
  );
}

interface BinCellProps {
  decision: LineDecision;
}

/** The Bin column's per-line content (Task 4, module doc comment). Never
 * fetches for `skip` (no part involved) or `create_new` (the draft already
 * carries its own `bin_label`, no query needed). */
function BinCell({ decision }: BinCellProps) {
  switch (decision.type) {
    case 'add_stock':
    case 'add_as_variant':
      return <ResolvedPartBin partId={decision.part_id} />;
    case 'create_new':
      return (
        <span className="review-line-table-bin">
          {decision.draft.bin_label ?? (
            <span className="review-line-table-bin--unassigned">Unassigned</span>
          )}
        </span>
      );
    case 'skip':
    default:
      return <span className="review-line-table-bin--unassigned">—</span>;
  }
}

interface ResolvedPartBinProps {
  partId: PartId;
}

/** The target part's CURRENT bin, resolved via `usePart` — `matches` never
 * carries `bin_label` (see the module doc comment), so this is unconditional
 * (unlike `ResolvedPartDecision`'s `knownName` shortcut, which only applies
 * to the display name). Read-only: this decision doesn't create or move a
 * part, so there's nothing here to edit. */
function ResolvedPartBin({ partId }: ResolvedPartBinProps) {
  const partQuery = usePart(partId);
  const bin = partQuery.data?.bin_label ?? null;
  return (
    <span className="review-line-table-bin">
      {bin ?? <span className="review-line-table-bin--unassigned">Unassigned</span>}
    </span>
  );
}
