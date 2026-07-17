/**
 * Part detail's Transactions tab (Phase 3 Task 7): the full ledger for one
 * part (`list_transactions`, already newest-first — see
 * `Database::list_transactions`'s `ORDER BY t.rowid DESC`), each row showing
 * type/quantity/state transition/project/note/time, with a reverse action.
 *
 * `TransactionRecord` (unlike the dashboard's `RecentTxn`) carries no
 * backend-computed `reversible` flag, but this tab already has the part's
 * *entire* transaction list in hand, so it recomputes the exact same rule
 * `reverse_transaction`/the dashboard's `recent_transactions` query enforce
 * (see `ledger.rs`'s `reverse_in_tx` and `dashboard.rs`'s reversible
 * subquery) client-side rather than firing a further per-row query:
 *   - not a reversal itself (`txn_type != 'reverse'`)
 *   - not part of a group (`group_id == null`)
 *   - not already reversed (no other row's `reversed_txn_id` points at it)
 *
 * A transaction that belongs to a group can't be reversed individually — the
 * backend rejects it with `TransactionInGroup` — so grouped rows get
 * "Reverse group" (`reverse_group`, keyed by the row's own `group_id`)
 * instead of a disabled single reverse. Reversing a group that's already
 * been reversed, or a reversal-of-a-group's own rows, are both still
 * possible to *attempt* here (this tab doesn't fetch each group's own header
 * to pre-check that) — the resulting `AlreadyReversed`/`CannotReverseReversal`
 * error is caught and toasted rather than silently failing.
 */

import type { PartId, ProjectId, QuantityUnit, TransactionRecord } from '../../bindings.gen';
import { useToast } from '../../components/Toast';
import {
  useProjects,
  useReverseGroup,
  useReverseTransaction,
  useTransactions,
} from '../../hooks/inventory';
import {
  errorHint,
  errorMessage,
  formatQuantity,
  formatTimestamp,
  formatTxnType,
} from '../../lib/format';
import './PartDetail.css';

const REVERSE_NOTE = 'Reversed from part detail';

export interface PartDetailTransactionsProps {
  partId: PartId;
  unit: QuantityUnit;
}

function isReversible(txn: TransactionRecord, all: TransactionRecord[]): boolean {
  if (txn.txn_type === 'reverse') return false;
  if (txn.group_id !== null) return false;
  return !all.some((other) => other.reversed_txn_id === txn.id);
}

function stateTransition(txn: TransactionRecord): string {
  if (txn.from_state && txn.to_state) return `${txn.from_state} → ${txn.to_state}`;
  if (txn.to_state) return `→ ${txn.to_state}`;
  if (txn.from_state) return `${txn.from_state} →`;
  return '—';
}

export function PartDetailTransactions({ partId, unit }: PartDetailTransactionsProps) {
  const transactionsQuery = useTransactions(partId);
  const projectsQuery = useProjects();
  const { toast } = useToast();

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

  const reverseGroupMut = useReverseGroup({
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

  if (transactionsQuery.isPending) {
    return <p className="part-detail-status">Loading transactions…</p>;
  }
  if (transactionsQuery.isError) {
    return (
      <p className="part-detail-status part-detail-status-error">
        Could not load transactions: {errorMessage(transactionsQuery.error)}
      </p>
    );
  }

  const transactions = transactionsQuery.data ?? [];
  if (transactions.length === 0) {
    return <p className="part-detail-status">No transactions recorded for this part yet.</p>;
  }

  const projects = projectsQuery.data ?? [];
  function projectName(id: ProjectId | null): string | null {
    if (!id) return null;
    return projects.find((p) => p.id === id)?.name ?? id;
  }

  return (
    <table className="part-detail-table part-detail-transactions-table">
      <thead>
        <tr>
          <th>Type</th>
          <th>Quantity</th>
          <th>State</th>
          <th>Project</th>
          <th>Note</th>
          <th>Time</th>
          <th>Reverse</th>
        </tr>
      </thead>
      <tbody>
        {transactions.map((txn) => {
          const groupId = txn.group_id;
          const reversible = isReversible(txn, transactions);
          const isReversingThis = reverseTxn.isPending && reverseTxn.variables?.txnId === txn.id;
          const isReversingGroup =
            reverseGroupMut.isPending && reverseGroupMut.variables?.groupId === txn.group_id;
          const project = projectName(txn.to_project_id ?? txn.project_id);

          return (
            <tr key={txn.id}>
              <td>{formatTxnType(txn.txn_type)}</td>
              <td className="part-detail-mono">{formatQuantity(txn.quantity, unit)}</td>
              <td className="part-detail-mono">{stateTransition(txn)}</td>
              <td>{project ?? '—'}</td>
              <td>{txn.note || '—'}</td>
              <td className="part-detail-mono">
                <time dateTime={txn.created_at}>{formatTimestamp(txn.created_at)}</time>
              </td>
              <td>
                {txn.txn_type === 'reverse' ? null : reversible ? (
                  <button
                    type="button"
                    className="part-detail-link-button"
                    disabled={isReversingThis}
                    onClick={() => reverseTxn.mutate({ txnId: txn.id, note: REVERSE_NOTE })}
                  >
                    {isReversingThis ? 'Reversing…' : 'Reverse'}
                  </button>
                ) : groupId ? (
                  <span className="part-detail-group-reverse">
                    <span className="part-detail-muted">Part of a group —</span>{' '}
                    <button
                      type="button"
                      className="part-detail-link-button"
                      disabled={isReversingGroup}
                      onClick={() => reverseGroupMut.mutate({ groupId, note: REVERSE_NOTE })}
                    >
                      {isReversingGroup ? 'Reversing…' : 'Reverse group'}
                    </button>
                  </span>
                ) : null}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
