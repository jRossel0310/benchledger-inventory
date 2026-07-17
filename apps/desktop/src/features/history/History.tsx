/**
 * The History screen (Phase 3 Task 9, spec §9): the comprehensive, filtered
 * view over every ledger transaction — `list_history`
 * (`crates/inventory-db/src/history.rs`), paged and joined with the human
 * context (part name, project name, group kind, archived state) each row
 * needs. Transactions that belong to a group render together under one
 * expandable header (`GroupRow.tsx`, `groupHistoryRows.ts`); everything else
 * renders as a flat row (`HistoryRowView.tsx`). Reversal (single or whole
 * group), "restore archived part", and the Phase 5 "view original import"
 * stub are all wired here; the filter bar (`HistoryFilters.tsx`) drives the
 * query.
 */

import { useState } from 'react';

import type { GroupId, HistoryFilter, PartId, TransactionId } from '../../bindings.gen';
import { useToast } from '../../components/Toast';
import {
  useHistory,
  useReverseGroup,
  useReverseTransaction,
  useSetArchived,
} from '../../hooks/inventory';
import { errorHint, errorMessage } from '../../lib/format';
import { GroupRow } from './GroupRow';
import { groupHistoryRows } from './groupHistoryRows';
import './History.css';
import {
  EMPTY_HISTORY_FILTER_STATE,
  HistoryFilters,
  type HistoryFilterState,
} from './HistoryFilters';
import { HistoryRowView } from './HistoryRowView';

const PAGE_SIZE = 25;
const REVERSE_NOTE = 'Reversed from History';

export interface HistoryProps {
  /** A deep-linked group to view exclusively (e.g. a future import's "view
   * this batch" link) — pre-fills the group filter; cleared via the filter
   * bar's "View all" chip. */
  initialGroupId?: GroupId | null;
}

function toHistoryFilter(state: HistoryFilterState, offset: number): HistoryFilter {
  return {
    date_from: state.dateFrom || null,
    date_to: state.dateTo || null,
    txn_type: state.txnType || null,
    part_id: state.partId,
    project_id: state.projectId,
    group_id: state.groupId,
    limit: PAGE_SIZE,
    offset,
  };
}

export function History({ initialGroupId = null }: HistoryProps) {
  const [filterState, setFilterState] = useState<HistoryFilterState>({
    ...EMPTY_HISTORY_FILTER_STATE,
    groupId: initialGroupId,
  });
  const [offset, setOffset] = useState(0);
  const { toast } = useToast();

  function handleFiltersChange(next: HistoryFilterState) {
    setFilterState(next);
    setOffset(0);
  }

  const filter = toHistoryFilter(filterState, offset);
  const historyQuery = useHistory(filter);

  const reverseTxn = useReverseTransaction({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not reverse transaction',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Transaction reversed', kind: 'success' });
    },
  });

  const reverseGroup = useReverseGroup({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not reverse group',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Group reversed', kind: 'success' });
    },
  });

  const setArchived = useSetArchived({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not restore part',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Part restored', kind: 'success' });
    },
  });

  function handleReverse(txnId: TransactionId) {
    reverseTxn.mutate({ txnId, note: REVERSE_NOTE });
  }

  function handleReverseGroup(groupId: GroupId) {
    reverseGroup.mutate({ groupId, note: REVERSE_NOTE });
  }

  function handleRestorePart(partId: PartId) {
    setArchived.mutate({ partId, archived: false });
  }

  function handleViewImport() {
    toast({
      title: 'Import viewer arrives in Phase 5',
      description: 'Viewing the original imported order isn’t built yet.',
      kind: 'warning',
    });
  }

  const reversingTxnId = reverseTxn.isPending ? (reverseTxn.variables?.txnId ?? null) : null;
  const reversingGroupId = reverseGroup.isPending
    ? (reverseGroup.variables?.groupId ?? null)
    : null;
  const restoringPartId = setArchived.isPending ? (setArchived.variables?.partId ?? null) : null;

  return (
    <section className="history-page">
      <header className="history-page-header">
        <p className="history-eyebrow">History</p>
        <h1 className="history-title">The transaction ledger</h1>
      </header>

      <HistoryFilters value={filterState} onChange={handleFiltersChange} />

      {historyQuery.isPending ? (
        <p className="history-status">Loading history…</p>
      ) : historyQuery.isError ? (
        <p className="history-status history-status-error">
          Could not load history: {errorMessage(historyQuery.error)}
        </p>
      ) : historyQuery.data.rows.length === 0 ? (
        <p className="history-status">No transactions match these filters.</p>
      ) : (
        <>
          <div className="history-list">
            <div className="history-row history-row-head" aria-hidden="true">
              <span className="history-cell history-cell-part">Part</span>
              <span className="history-cell">Type</span>
              <span className="history-cell">Quantity</span>
              <span className="history-cell">State</span>
              <span className="history-cell">Project</span>
              <span className="history-cell history-cell-note">Note</span>
              <span className="history-cell">Time</span>
              <span className="history-cell history-cell-actions">Actions</span>
            </div>
            {groupHistoryRows(historyQuery.data.rows).map((entry) =>
              entry.kind === 'row' ? (
                <HistoryRowView
                  key={entry.row.id}
                  row={entry.row}
                  isReversing={reversingTxnId === entry.row.id}
                  onReverse={() => handleReverse(entry.row.id)}
                  onRestorePart={handleRestorePart}
                  restoringPartId={restoringPartId}
                  onViewImport={handleViewImport}
                />
              ) : (
                <GroupRow
                  key={entry.groupId}
                  groupId={entry.groupId}
                  groupKind={entry.groupKind}
                  members={entry.members}
                  isReversing={reversingGroupId === entry.groupId}
                  onReverseGroup={handleReverseGroup}
                  onRestorePart={handleRestorePart}
                  restoringPartId={restoringPartId}
                  onViewImport={handleViewImport}
                />
              ),
            )}
          </div>

          <HistoryPagination
            offset={offset}
            pageSize={PAGE_SIZE}
            rowCount={historyQuery.data.rows.length}
            total={historyQuery.data.total}
            onOffsetChange={setOffset}
            isFetching={historyQuery.isFetching}
          />
        </>
      )}
    </section>
  );
}

interface HistoryPaginationProps {
  offset: number;
  pageSize: number;
  rowCount: number;
  total: number;
  onOffsetChange: (offset: number) => void;
  isFetching: boolean;
}

function HistoryPagination({
  offset,
  pageSize,
  rowCount,
  total,
  onOffsetChange,
  isFetching,
}: HistoryPaginationProps) {
  const from = total === 0 ? 0 : offset + 1;
  const to = offset + rowCount;
  const hasPrev = offset > 0;
  const hasNext = offset + rowCount < total;

  return (
    <div className="history-pagination">
      <span className="history-pagination-summary">
        {from}–{to} of {total}
      </span>
      <div className="history-pagination-buttons">
        <button
          type="button"
          className="history-pagination-button"
          disabled={!hasPrev || isFetching}
          onClick={() => onOffsetChange(Math.max(0, offset - pageSize))}
        >
          Prev
        </button>
        <button
          type="button"
          className="history-pagination-button"
          disabled={!hasNext || isFetching}
          onClick={() => onOffsetChange(offset + pageSize)}
        >
          Next
        </button>
      </div>
    </div>
  );
}
