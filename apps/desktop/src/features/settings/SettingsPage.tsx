import { StatusPanel } from '../dashboard/StatusPanel';
import { FeaturePanel } from '../shared/FeaturePanel';
import { DigiKeySettings } from './DigiKeySettings';

export function SettingsPage() {
  return (
    <FeaturePanel
      eyebrow="Settings"
      title="Application status"
      description="Theme, low-stock defaults, and backup/sync status controls land in later phases. For now, this panel shows read-only application status and DigiKey enrichment configuration."
    >
      <StatusPanel />
      <DigiKeySettings />
    </FeaturePanel>
  );
}
