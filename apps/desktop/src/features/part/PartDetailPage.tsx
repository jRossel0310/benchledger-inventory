import { useParams } from '@tanstack/react-router';

import type { PartId } from '../../bindings.gen';
import { PartDetail } from './PartDetail';
import './PartDetail.css';

/**
 * `/inventory/$partId` — the full-page, deep-link/back-button-friendly part
 * detail view (Phase 3 Task 7). Shares the exact same `PartDetail` body as
 * `PartInspector` (the drawer, the fast path from the Inventory table); this
 * wrapper renders it with no `onClose` (there's no drawer chrome to close —
 * "back" is the browser/router history), inside a page-level container so it
 * reads as a standalone screen rather than a floating panel.
 */
export function PartDetailPage() {
  const { partId } = useParams({ from: '/inventory/$partId' });
  return (
    <div className="part-detail-page">
      <PartDetail partId={partId as PartId} />
    </div>
  );
}
