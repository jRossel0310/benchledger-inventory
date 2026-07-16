import { FeaturePanel } from '../shared/FeaturePanel';

export function ProjectsPage() {
  return (
    <FeaturePanel
      eyebrow="Projects"
      title="Coming in Phase 4"
      description="Projects track reservations and checkouts against a build: create a project, reserve parts against it, transfer reservations between projects, and see a running per-project cost. The ledger already supports project-scoped operations — this screen, BOM import, and project cost rollups arrive with Phase 4."
    />
  );
}
