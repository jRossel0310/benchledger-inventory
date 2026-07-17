/**
 * One ledger row's cells, shared by History's flat (ungrouped) rows and
 * `GroupRow`'s expanded members — the same column layout either way so the
 * list reads as one instrument, not two different tables stitched together.
 * `variant="member"` (a row inside an expanded group) never renders an
 * individual reverse control: a grouped transaction can't be reversed
 * standalone (see `GroupRow.tsx`'s doc comment) — only `onReverse` being
 * supplied at all turns the action on, so flat rows opt in and members
 * simply don't pass it.
 */

import type { HistoryRow, PartId } from '../../bindings.gen';
import { formatQuantity, formatTimestamp, formatTxnType } from '../../lib/format';
import { stateTransition } from './historyRow';
import './History.css';

export interface HistoryRowViewProps {
  row: HistoryRow;
  variant?: 'flat' | 'member';
  isReversing?: boolean;
  onReverse?: () => void;
  onRestorePart: (partId: PartId) => void;
  restoringPartId: PartId | null;
  onViewImport: () => void;
}

export function HistoryRowView({
  row,
  variant = 'flat',
  isReversing,
  onReverse,
  onRestorePart,
  restoringPartId,
  onViewImport,
}: HistoryRowViewProps) {
  return (
    <div className={`history-row${variant === 'member' ? ' history-row-member' : ''}`}>
      <span className="history-cell history-cell-part">{row.display_name}</span>
      <span className="history-cell">{formatTxnType(row.txn_type)}</span>
      <span className="history-cell history-mono">
        {formatQuantity(row.quantity, row.quantity_unit)}
      </span>
      <span className="history-cell history-mono">{stateTransition(row)}</span>
      <span className="history-cell">{row.project_name ?? '—'}</span>
      <span className="history-cell history-cell-note">{row.note || '—'}</span>
      <span className="history-cell history-mono">
        <time dateTime={row.created_at}>{formatTimestamp(row.created_at)}</time>
      </span>
      <span className="history-cell history-cell-actions">
        {onReverse && row.reversible ? (
          <button
            type="button"
            className="history-link-button"
            disabled={isReversing}
            onClick={onReverse}
          >
            {isReversing ? 'Reversing…' : 'Reverse'}
          </button>
        ) : null}
        {row.part_archived ? (
          <button
            type="button"
            className="history-link-button"
            disabled={restoringPartId === row.part_id}
            onClick={() => onRestorePart(row.part_id)}
          >
            {restoringPartId === row.part_id ? 'Restoring…' : 'Restore part'}
          </button>
        ) : null}
        {row.import_id ? (
          <button type="button" className="history-link-button" onClick={onViewImport}>
            View original import
          </button>
        ) : null}
      </span>
    </div>
  );
}
