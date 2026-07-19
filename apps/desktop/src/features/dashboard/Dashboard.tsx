/**
 * The Dashboard screen (Phase 3 Task 3, see
 * docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md §9):
 * answers "what's in inventory, what's low, what's out, what changed" in one
 * glance — summary cards (each linking to the relevant inventory view),
 * an aggregate stock-state gauge, a recent-activity feed with safe reversal,
 * and an honest publish/backup status strip. The publishing row is live as
 * of Phase 6 (`PublishStatusCard` — real status, never a fake "published"
 * claim); backup arrives Phase 7 and still renders the true "not
 * configured yet" state.
 */

import { Link } from '@tanstack/react-router';

import type { RecentTxn } from '../../bindings.gen';
import { StockGauge } from '../../components/StockGauge';
import { useToast } from '../../components/Toast';
import {
  useDashboardSummary,
  useRecentTransactions,
  useReverseTransaction,
} from '../../hooks/inventory';
import {
  errorHint,
  errorMessage,
  formatQuantity,
  formatTimestamp,
  formatTxnType,
} from '../../lib/format';
import { PublishStatusCard } from './PublishStatusCard';
import './Dashboard.css';

const RECENT_LIMIT = 20;

export function Dashboard() {
  const summaryQuery = useDashboardSummary();
  const recentQuery = useRecentTransactions(RECENT_LIMIT);
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

  function handleReverse(txn: RecentTxn) {
    reverseTxn.mutate({ txnId: txn.id, note: 'Reversed from the dashboard' });
  }

  if (summaryQuery.isPending) {
    return <p className="dashboard-status">Loading dashboard…</p>;
  }

  if (summaryQuery.isError) {
    return (
      <p className="dashboard-status dashboard-status-error">
        Could not load the dashboard: {errorMessage(summaryQuery.error)}
      </p>
    );
  }

  const summary = summaryQuery.data;

  if (summary.part_count === 0) {
    return (
      <section className="dashboard dashboard-empty">
        <p className="dashboard-eyebrow">Dashboard</p>
        <h1 className="dashboard-empty-title">No parts yet</h1>
        <p className="dashboard-empty-description">
          Press <kbd>Ctrl</kbd>+<kbd>K</kbd> to create one, or import an order.
        </p>
      </section>
    );
  }

  return (
    <section className="dashboard">
      <header className="dashboard-header">
        <p className="dashboard-eyebrow">Dashboard</p>
        <h1 className="dashboard-title">Inventory at a glance</h1>
      </header>

      <div className="dashboard-cards">
        <DashboardCard
          eyebrow="Available"
          value={formatQuantity(summary.available_units, 'each')}
          detail={`${summary.part_count} part${summary.part_count === 1 ? '' : 's'}`}
          to="/inventory"
        />
        <DashboardCard
          eyebrow="Reserved"
          value={formatQuantity(summary.reserved_units, 'each')}
          to="/inventory"
          search={{ q: 'reserved:>0' }}
        />
        <DashboardCard
          eyebrow="Checked out"
          value={formatQuantity(summary.checked_out_units, 'each')}
          to="/inventory"
          search={{ q: 'checked_out:>0' }}
        />
        <DashboardCard
          eyebrow="Low stock"
          value={String(summary.low_stock_count)}
          to="/inventory"
          search={{ q: 'low stock' }}
          tone={summary.low_stock_count > 0 ? 'low' : undefined}
        />
        <DashboardCard
          eyebrow="Active projects"
          value={String(summary.active_project_count)}
          to="/projects"
        />
        <DashboardCard
          eyebrow="Needs metadata review"
          value={String(summary.metadata_incomplete_count)}
          to="/inventory"
        />
        <DashboardCard
          eyebrow="Without a bin"
          value={String(summary.unbinned_count)}
          to="/inventory"
        />
      </div>

      <div className="dashboard-body">
        <section className="dashboard-panel dashboard-gauge-panel">
          <h2 className="dashboard-panel-title">Stock split</h2>
          <StockGauge
            available={summary.available_units}
            reserved={summary.reserved_units}
            checkedOut={summary.checked_out_units}
            unit="each"
            size="panel"
          />
          <p className="dashboard-panel-hint">
            Every unit across every part, regardless of its own quantity unit — a raw count, not a
            single physical unit.
          </p>
        </section>

        <section className="dashboard-panel dashboard-sync-panel">
          <h2 className="dashboard-panel-title">Publish &amp; backup</h2>
          <ul className="dashboard-sync-list">
            <PublishStatusCard />
            <li className="dashboard-sync-row">
              <span className="dashboard-sync-dot" aria-hidden="true" />
              <span className="dashboard-sync-text">
                Backup not configured — set up in{' '}
                <Link to="/settings" className="dashboard-inline-link">
                  Settings
                </Link>
                .
              </span>
            </li>
          </ul>
        </section>
      </div>

      <section className="dashboard-panel dashboard-activity-panel">
        <h2 className="dashboard-panel-title">Recent activity</h2>
        {recentQuery.isPending ? (
          <p className="dashboard-status">Loading recent activity…</p>
        ) : recentQuery.isError ? (
          <p className="dashboard-status dashboard-status-error">
            Could not load recent activity: {errorMessage(recentQuery.error)}
          </p>
        ) : recentQuery.data.length === 0 ? (
          <p className="dashboard-status">No activity yet.</p>
        ) : (
          <ul className="dashboard-activity-list" aria-label="Recent activity">
            {recentQuery.data.map((txn) => {
              const isReversingThis =
                reverseTxn.isPending && reverseTxn.variables?.txnId === txn.id;
              return (
                <li key={txn.id} className="dashboard-activity-row">
                  <span className="dashboard-activity-action">{formatTxnType(txn.txn_type)}</span>
                  <span className="dashboard-activity-qty">
                    {formatQuantity(txn.quantity, txn.quantity_unit)}
                  </span>
                  <Link
                    to="/inventory/$partId"
                    params={{ partId: txn.part_id }}
                    className="dashboard-activity-part"
                  >
                    {txn.display_name}
                  </Link>
                  <time className="dashboard-activity-time" dateTime={txn.created_at}>
                    {formatTimestamp(txn.created_at)}
                  </time>
                  {txn.reversible ? (
                    <button
                      type="button"
                      className="dashboard-activity-reverse"
                      disabled={isReversingThis}
                      onClick={() => handleReverse(txn)}
                    >
                      {isReversingThis ? 'Reversing…' : 'Reverse'}
                    </button>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </section>
  );
}

interface DashboardCardProps {
  eyebrow: string;
  value: string;
  detail?: string;
  to: string;
  search?: Record<string, string>;
  /** `'low'` paints the value in the low-stock amber tone — used only by
   * the low-stock card, and only once it actually has a nonzero count. */
  tone?: 'low';
}

function DashboardCard({ eyebrow, value, detail, to, search, tone }: DashboardCardProps) {
  return (
    <Link to={to} search={search} className="dashboard-card">
      <p className="dashboard-card-eyebrow">{eyebrow}</p>
      <p className={`dashboard-card-value${tone === 'low' ? ' dashboard-card-value-low' : ''}`}>
        {value}
      </p>
      {detail ? <p className="dashboard-card-detail">{detail}</p> : null}
    </Link>
  );
}
