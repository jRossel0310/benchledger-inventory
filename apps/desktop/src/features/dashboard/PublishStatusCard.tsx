/**
 * The Dashboard's publish-status row (Phase 6 Task 5): the publishing line
 * of the "Publish & backup" panel, driven by `usePublishStatus()` instead
 * of the Phase 3 hard-coded "not configured" copy.
 *
 * Honest simplification (per the Phase 6 plan, Task 5): the app has no
 * change-tracking, so "unpublished changes" cannot be detected — the only
 * proof of up-to-dateness is `publish_now` returning `unchanged`, which
 * this card never runs. It therefore shows exactly what is known:
 *
 * - "Publishing not configured" + a Settings link,
 * - "Configured — nothing published yet." before the first publish,
 * - "Last published <ts>" (normal tone) once something has shipped,
 * - "Publish pending — will retry on launch" (warning tone + Settings
 *   link) when a failed publish left the pending marker — the pending
 *   flag is the ONLY thing that ever paints this card in an error tone.
 */

import { Link } from '@tanstack/react-router';

import { usePublishStatus } from '../../hooks/publish';
import { formatTimestamp } from '../../lib/format';
import './PublishStatusCard.css';

export function PublishStatusCard() {
  const statusQuery = usePublishStatus();

  if (statusQuery.isPending) {
    return (
      <li className="dashboard-sync-row">
        <span className="dashboard-sync-dot" aria-hidden="true" />
        <span className="dashboard-sync-text">Loading publish status…</span>
      </li>
    );
  }

  if (statusQuery.isError) {
    return (
      <li className="dashboard-sync-row">
        <span className="dashboard-sync-dot" aria-hidden="true" />
        <span className="dashboard-sync-text">Publish status unavailable.</span>
      </li>
    );
  }

  const status = statusQuery.data;

  if (!status.configured) {
    return (
      <li className="dashboard-sync-row">
        <span className="dashboard-sync-dot" aria-hidden="true" />
        <span className="dashboard-sync-text">
          Publishing not configured — set up in{' '}
          <Link to="/settings" className="dashboard-inline-link">
            Settings
          </Link>
          .
        </span>
      </li>
    );
  }

  if (status.pending) {
    return (
      <li className="dashboard-sync-row">
        <span className="dashboard-sync-dot publish-status-card-dot-warn" aria-hidden="true" />
        <span className="dashboard-sync-text publish-status-card-warn">
          Publish pending — will retry on launch. Retry now in{' '}
          <Link to="/settings" className="dashboard-inline-link">
            Settings
          </Link>
          .
        </span>
      </li>
    );
  }

  return (
    <li className="dashboard-sync-row">
      <span className="dashboard-sync-dot publish-status-card-dot-ok" aria-hidden="true" />
      <span className="dashboard-sync-text">
        {status.last_published_at
          ? `Last published ${formatTimestamp(status.last_published_at)}.`
          : 'Configured — nothing published yet.'}
      </span>
    </li>
  );
}
