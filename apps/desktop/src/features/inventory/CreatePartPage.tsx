import { FeaturePanel } from '../shared/FeaturePanel';

/**
 * `/inventory/new` — the Ctrl+K palette's "Create part" action and Task 6's
 * upcoming create-part form both target this one route, so the palette has
 * a real navigable destination today rather than a dead end or a silently
 * discarded action; Task 6 fills in the actual form here.
 */
export function CreatePartPage() {
  return (
    <FeaturePanel
      eyebrow="Create part"
      title="Coming in Phase 3 Task 6"
      description="A guided form for adding a new part — category, identity attributes, bin, quantity unit, and initial stock — lands in Task 6. The Ctrl+K palette's “Create part” action already routes here."
    />
  );
}
