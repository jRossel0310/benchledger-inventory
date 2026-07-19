import { StatusPanel } from '../dashboard/StatusPanel';
import { FeaturePanel } from '../shared/FeaturePanel';
import { DigiKeySettings } from './DigiKeySettings';
import { PublishSettings } from './PublishSettings';

export function SettingsPage() {
  return (
    <FeaturePanel
      eyebrow="Settings"
      title="Application status"
      description="Theme, low-stock defaults, and backup status controls land in later phases. For now, this panel shows read-only application status, DigiKey enrichment configuration, and public-web publishing configuration."
    >
      <StatusPanel />
      <DigiKeySettings />
      <PublishSettings />
    </FeaturePanel>
  );
}
