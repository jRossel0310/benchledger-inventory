import { useParams } from '@tanstack/react-router';

import { ImportReview } from './ImportReview';

/**
 * `/orders/$importId` route wrapper (Phase 5d Task 3): the Phase 5d Task 2
 * placeholder shell is retired now that `ImportReview` is real — the same
 * thin route-wrapper-around-a-reusable-body pattern `OrdersPage.tsx` uses
 * for `OrdersList`.
 */
export function ImportReviewPage() {
  const { importId } = useParams({ from: '/orders/$importId' });
  return <ImportReview importId={importId} />;
}
