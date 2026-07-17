/**
 * One transaction group in the History screen (Phase 3 Task 9): a header
 * (kind, time, member count) that expands to its member rows, plus a single
 * "Reverse group" action on the header — never on individual members, since
 * a grouped transaction can't be reversed standalone (the backend rejects it
 * with `TransactionInGroup`; T7's `PartDetailTransactions.tsx` documents the
 * same rule).
 *
 * Both the header count and the reverse confirmation must be honest about
 * the group's real membership, which `members` (whatever rows of this group
 * `list_history` happened to return for the *current* filter/page) can
 * understate: a part/project filter or a pagination boundary can hide some
 * of a group's members entirely (see `groupHistoryRows.ts`'s doc comment).
 * The backend's `reverse_group` always reverses the ENTIRE group regardless
 * of what's visible, so a confirmation built only from `members` can promise
 * "N operations" while silently reversing more.
 *
 * The header count uses `group_total` (`HistoryRow`, sourced server-side by
 * `list_history` via a correlated subquery — see `history.rs`) rather than
 * `members.length`, and flags when the visible set is a subset. The reverse
 * confirmation goes further: it fetches the group's TRUE full member list
 * (`useGroup` -> `get_group`) when it opens and renders only that — never a
 * partial list — disabling "Reverse group" until it has loaded.
 */

import * as Dialog from '@radix-ui/react-dialog';
import { useState } from 'react';

import type {
  GroupId,
  HistoryRow,
  PartId,
  PartRecord,
  TransactionRecord,
} from '../../bindings.gen';
import { useGroup, useParts } from '../../hooks/inventory';
import {
  errorMessage,
  formatGroupKind,
  formatQuantity,
  formatTimestamp,
  formatTxnType,
} from '../../lib/format';
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

/** Resolves one `get_group` transaction's display name/unit for the
 * confirmation list. `TransactionRecord` (unlike `HistoryRow`) carries no
 * joined part name/unit, so this fills them in from whatever's available:
 * first the exact same transaction if it's one of the currently-visible
 * `members` (an exact match by transaction id — always accurate, and the
 * common case when a group isn't filtered/paged apart); otherwise the
 * just-fetched full parts list keyed by `part_id` (covers a member hidden
 * from the current History page, which `members` never has); otherwise the
 * raw part id rather than fabricating a name. */
function resolveTxnDisplay(
  txn: TransactionRecord,
  membersById: Map<string, HistoryRow>,
  partsById: Map<PartId, PartRecord> | null,
): { name: string; unit: string } {
  const member = membersById.get(txn.id);
  if (member) return { name: member.display_name, unit: member.quantity_unit };
  const part = partsById?.get(txn.part_id);
  if (part) return { name: part.display_name, unit: part.quantity_unit };
  return { name: txn.part_id, unit: 'each' };
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
  // Every member row carries the same server-computed `group_total` (it's a
  // property of the group, not the individual row) — falling back to
  // `members.length` only guards a `members` array that's somehow empty.
  const groupTotal = members[0]?.group_total ?? members.length;
  const someHidden = members.length < groupTotal;

  // Fetched only once the confirmation dialog is open (`enabled` gate inside
  // each hook) — never on every group header render.
  const groupQuery = useGroup(confirming ? groupId : undefined);
  const partsQuery = useParts(true, confirming);
  const confirmedGroup = groupQuery.data ?? null;
  const isGroupLoading = confirming && groupQuery.isPending;
  const groupLoadFailed = confirming && groupQuery.isError;

  function confirmReverse() {
    setConfirming(false);
    onReverseGroup(groupId);
  }

  const membersById = new Map(members.map((row) => [row.id, row]));
  const partsById = partsQuery.data
    ? new Map(partsQuery.data.map((part) => [part.id, part]))
    : null;

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
            {groupTotal} operation{groupTotal === 1 ? '' : 's'}
          </span>
          {someHidden ? (
            <span className="history-group-partial-note">
              · some not shown under current filter
            </span>
          ) : null}
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
              <Dialog.Title className="history-dialog-title">
                {confirmedGroup
                  ? `Reverse the entire "${formatGroupKind(confirmedGroup.kind)}" group?`
                  : 'Reverse this group?'}
              </Dialog.Title>
              <Dialog.Description
                className={`history-dialog-description${
                  groupLoadFailed || confirmedGroup === null ? ' history-status-error' : ''
                }`}
              >
                {isGroupLoading
                  ? 'Loading the full group before you confirm…'
                  : groupLoadFailed
                    ? `Could not load the full group${
                        groupQuery.error ? `: ${errorMessage(groupQuery.error)}` : ''
                      } — try again.`
                    : confirmedGroup === null
                      ? 'This group could not be found — it may have already been reversed elsewhere.'
                      : `All ${confirmedGroup.transactions.length} operation${
                          confirmedGroup.transactions.length === 1 ? '' : 's'
                        } will be undone:`}
              </Dialog.Description>
              {confirmedGroup ? (
                <ul className="history-dialog-op-list">
                  {confirmedGroup.transactions.map((txn) => {
                    const info = resolveTxnDisplay(txn, membersById, partsById);
                    return (
                      <li key={txn.id}>
                        {formatTxnType(txn.txn_type)} {formatQuantity(txn.quantity, info.unit)} —{' '}
                        {info.name}
                      </li>
                    );
                  })}
                </ul>
              ) : null}
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
                  disabled={isReversing || isGroupLoading || confirmedGroup === null}
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
