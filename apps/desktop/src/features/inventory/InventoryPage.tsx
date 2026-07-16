import { FeaturePanel } from '../shared/FeaturePanel';

export function InventoryPage() {
  return (
    <FeaturePanel
      eyebrow="Inventory"
      title="The parts library"
      description="A dense, virtualized table of every part — bin, quantity unit, and an inline available/reserved/checked-out gauge per row — lands here in Phase 3 Task 4, scaling to the 10k-part target with TanStack Virtual."
    />
  );
}
