import { useParams } from '@tanstack/react-router';
import { FeaturePanel } from '../shared/FeaturePanel';

export function PartDetailPage() {
  const { partId } = useParams({ strict: false });

  return (
    <FeaturePanel
      eyebrow="Part detail"
      title={partId ? `Part ${partId}` : 'Part detail'}
      description="The full part record — variants, supplier listings, dimensions, attributes, tags, and ledger history, with reversal — lands in Phase 3 Task 7. A faster right-hand inspector drawer over the inventory table (Task 4/7) covers the same data without leaving the list."
    />
  );
}
