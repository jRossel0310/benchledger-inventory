/**
 * The import review's per-line table (Phase 5d Task 3): line #, item
 * identity, ordered/shipped/backordered quantities (shipped highlighted as
 * the actual receive quantity — spec §10's "shipped, never ordered"), unit
 * price, top match verdict, current decision, and a "Change…" trigger
 * (`LineActionEditor`) for `part`-kind lines only. Non-`part` kinds (fee/
 * tariff/no_charge/unknown) render greyed with their kind badge and no
 * action editor — they never create inventory (`ProposedAction::
 * NonInventory`/`Ignore`), so there is nothing to decide.
 *
 * A plain (non-virtualized) `<table>`, not `DataTable.tsx`: `DataTable` is
 * built for the 10k-row Inventory browser and needs a fixed per-row pixel
 * height, which doesn't fit this screen's variable-height rows (a warning
 * line, a two-line item cell) at the handful-to-dozens of lines one order
 * actually has — the same "no gauge/DataTable forcing" reasoning
 * `BomTable.tsx`'s module doc comment gives for its own dense-table choice.
 */

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
}

export function ReviewLineTable({
  lines,
  decisions,
  decisionWarnings,
  onChangeDecision,
  context,
}: ReviewLineTableProps) {
  return (
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
                  <DecisionSummary decision={decision} matches={line.matches} />
                ) : (
                  <span className="review-line-table-non-part-label">Not inventory</span>
                )}
              </td>
              <td className="review-line-table-cell review-line-table-cell-actions">
                {isPart && decision ? (
                  <LineActionEditor
                    line={line}
                    decision={decision}
                    onChange={(next) => onChangeDecision(line.line_id, next)}
                    context={context}
                  />
                ) : null}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
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
}

/** The current decision's human label. For `add_stock`/`add_as_variant`,
 * resolves the target part's display name — from the line's own `matches`
 * when the target is one of them (no extra query), otherwise via `usePart`
 * (the same per-row resolve-by-id fallback `BomTable.tsx`'s
 * `SubstituteChip` uses for a substitute picked from outside the visible
 * list). */
function DecisionSummary({ decision, matches }: DecisionSummaryProps) {
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
    case 'create_new':
      return (
        <span className="review-line-table-decision">
          Create new part
          {isCreateNewIncomplete(decision) ? (
            <span className="review-line-table-decision-flag">Draft incomplete</span>
          ) : null}
        </span>
      );
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
