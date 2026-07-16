import { FeaturePanel } from '../shared/FeaturePanel';

export function BinsPage() {
  return (
    <FeaturePanel
      eyebrow="Bins"
      title="Physical storage, browsed by location"
      description="A part's storage location is its bin_label field, not a separate bins table — this screen (Phase 3 Task 8) groups parts by that label so you can browse the shelf the way you'd walk it."
    />
  );
}
