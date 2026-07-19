import { useParams } from '@tanstack/react-router';

import { FeaturePanel } from '../shared/FeaturePanel';

/**
 * `/orders/$importId` route shell (Phase 5d Task 2) — a minimal placeholder
 * until Task 3 replaces it with the real Match -> Review screen
 * (`ImportReview.tsx`: summary header, per-line table, per-line action
 * editor). Deliberately tiny: it exists only to confirm `OrdersList`'s row
 * click/`UploadImport`'s post-upload navigation land somewhere real, not to
 * preview Task 3's UI.
 */
export function ImportReviewPage() {
  const { importId } = useParams({ from: '/orders/$importId' });
  return (
    <FeaturePanel
      eyebrow="Orders"
      title={`Import ${importId}`}
      description="Review UI lands in the next task — this placeholder only confirms the import was found."
    />
  );
}
