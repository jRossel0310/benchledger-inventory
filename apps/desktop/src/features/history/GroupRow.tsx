/**
 * One transaction group in the History screen (Phase 3 Task 9): a header
 * (kind, time, member count) that expands to its member rows, plus a single
 * "Reverse group" action on the header — never on individual members, since
 * a grouped transaction can't be reversed standalone (the backend rejects it
 * with `TransactionInGroup`; T7's `PartDetailTransactions.tsx` documents the
 * same rule). The reverse action opens a confirmation listing exactly what
 * will happen (one line per member op) before calling `useReverseGroup`.
 */

import * as Dialog from '@radix-ui/react-dialog';
import { useState } from 'react';

import type { GroupId, HistoryRow, PartId } from '../../bindings.gen';
import { formatGroupKind, formatQuantity, formatTimestamp, formatTxnType } from '../../lib/format';
import { HistoryRowView } from './HistoryRowView';
import './History.css';

export interface GroupRowProps {
  groupId: GroupId;
  groupKind: string;
  members: HistoryRow[];
  isReversing: boolean;
  onReverseGroup: (groupId: GroupId) => void;
  onRestorePart: (partId: PartId) => void;
  restoringPartId: PartId | null;
  onViewImport: () => void;
}

export function GroupRow({
  groupId,
  groupKind,
  members,
  isReversing,
  onReverseGroup,
  onRestorePart,
  restoringPartId,
  onViewImport,
}: GroupRowProps) {
  const [expanded, setExpanded] = useState(false);
  const [confirming, setConfirming] = useState(false);
  // Members arrive newest-first (see `groupHistoryRows`'s doc); the most
  // recent member's timestamp reads as "the group's time" for the header.
  const headerTime = members[0]?.created_at;

  function confirmReverse() {
    setConfirming(false);
    onReverseGroup(groupId);
  }

  return (
    <div className="history-group">
      <div className="history-group-header">
        <button
          type="button"
          className="history-group-toggle"
          aria-expanded={expanded}
          onClick={() => setExpanded((v) => !v)}
        >
          <span className="history-group-toggle-chevron" aria-hidden="true">
            {expanded ? '▾' : '▸'}
          </span>
          <span className="history-group-kind">{formatGroupKind(groupKind)}</span>
          <span className="history-group-count">
            {members.length} operation{members.length === 1 ? '' : 's'}
          </span>
          {headerTime ? (
            <time className="history-group-time" dateTime={headerTime}>
              {formatTimestamp(headerTime)}
            </time>
          ) : null}
        </button>
        <button
          type="button"
          className="history-link-button history-group-reverse"
          disabled={isReversing}
          onClick={() => setConfirming(true)}
        >
          {isReversing ? 'Reversing…' : 'Reverse group'}
        </button>
      </div>

      {expanded ? (
        <div className="history-group-members">
          {members.map((row) => (
            <HistoryRowView
              key={row.id}
              row={row}
              variant="member"
              onRestorePart={onRestorePart}
              restoringPartId={restoringPartId}
              onViewImport={onViewImport}
            />
          ))}
        </div>
      ) : null}

      {confirming ? (
        <Dialog.Root open onOpenChange={(open) => !open && setConfirming(false)}>
          <Dialog.Portal>
            <Dialog.Overlay className="history-dialog-overlay" />
            <Dialog.Content className="history-dialog-content">
              <Dialog.Title className="history-dialog-title">Reverse this group?</Dialog.Title>
              <Dialog.Description className="history-dialog-description">
                {formatGroupKind(groupKind)} — every operation below will be undone:
              </Dialog.Description>
              <ul className="history-dialog-op-list">
                {members.map((row) => (
                  <li key={row.id}>
                    {formatTxnType(row.txn_type)} {formatQuantity(row.quantity, row.quantity_unit)}{' '}
                    — {row.display_name}
                  </li>
                ))}
              </ul>
              <div className="history-dialog-buttons">
                <button
                  type="button"
                  className="history-dialog-cancel"
                  onClick={() => setConfirming(false)}
                  disabled={isReversing}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  className="history-dialog-submit"
                  onClick={confirmReverse}
                  disabled={isReversing}
                >
                  {isReversing ? 'Reversing…' : 'Reverse group'}
                </button>
              </div>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      ) : null}
    </div>
  );
}
