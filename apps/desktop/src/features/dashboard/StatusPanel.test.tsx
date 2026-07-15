import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({
    appVersion: '0.1.0',
    schemaVersion: 1,
    dataDir: 'C:\\Users\\x\\AppData\\Roaming\\ElectronicsInventory',
  }),
}));

import { StatusPanel } from './StatusPanel';

describe('StatusPanel', () => {
  it('renders app status returned by the app_status command', async () => {
    render(<StatusPanel />);
    await waitFor(() => {
      expect(screen.getByText(/0\.1\.0/)).toBeTruthy();
      expect(screen.getByText(/schema v1/i)).toBeTruthy();
      expect(screen.getByText(/ElectronicsInventory/)).toBeTruthy();
    });
  });
});
