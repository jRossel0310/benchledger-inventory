import { FeaturePanel } from '../shared/FeaturePanel';

export function DashboardPage() {
  return (
    <FeaturePanel
      eyebrow="Dashboard"
      title="Inventory at a glance"
      description="An aggregate stock-state gauge, low-stock alerts, and recent ledger activity land here in Phase 3 Task 3, once the signature StockGauge component (Task 2) exists to render them."
    />
  );
}
