import { QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRouter, RouterProvider } from '@tanstack/react-router';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Command-aware: the Dashboard screen (mounted at "/") calls dashboard_summary
// and recent_transactions on render, and expects their own shapes back — a
// single fixed mockResolvedValue (this suite's pre-Dashboard shape) would
// hand Dashboard an AppStatus-shaped object instead, crashing its render.
// An empty DashboardSummary (part_count: 0) keeps Dashboard on its harmless
// empty-state path, which is all this suite (rail links, command bar,
// per-route stub panels) needs.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn((cmd: string) => {
    if (cmd === 'dashboard_summary') {
      return Promise.resolve({
        available_units: 0,
        part_count: 0,
        reserved_units: 0,
        checked_out_units: 0,
        low_stock_count: 0,
        active_project_count: 0,
        metadata_incomplete_count: 0,
        unbinned_count: 0,
      });
    }
    if (cmd === 'recent_transactions') {
      return Promise.resolve([]);
    }
    return Promise.resolve({
      appVersion: '0.1.0',
      schemaVersion: 4,
      dataDir: 'C:\\scratch\\ElectronicsInventory',
    });
  }),
}));

// jsdom doesn't implement scrollTo; the router's scroll-restoration effect
// calls it on every navigation.
beforeEach(() => {
  window.scrollTo = vi.fn();
});

// This suite renders more than once per file (unlike the single-render
// StatusPanel test); without explicit cleanup each render leaks into the
// next test's document.
afterEach(cleanup);

import { ToastProvider } from '../components/Toast';
import { queryClient } from './queryClient';
import { routeTree } from './routes';

function renderShellAt(path: string) {
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('AppShell', () => {
  it('renders the left rail with every top-level section', async () => {
    renderShellAt('/');
    for (const label of [
      'Dashboard',
      'Inventory',
      'Bins',
      'Projects',
      'Orders',
      'History',
      'Settings',
    ]) {
      await waitFor(() => {
        expect(screen.getByRole('link', { name: new RegExp(label) })).toBeTruthy();
      });
    }
  });

  it('renders the top command bar search input and the Ctrl+K affordance', async () => {
    renderShellAt('/');
    await waitFor(() => {
      expect(screen.getByRole('searchbox', { name: /search inventory/i })).toBeTruthy();
    });
    expect(screen.getByText('Ctrl K')).toBeTruthy();
  });

  it('routes to the stub panels for each section', async () => {
    renderShellAt('/bins');
    await waitFor(() => {
      expect(screen.getByText(/Physical storage, browsed by location/i)).toBeTruthy();
    });
  });

  it('routes the projects and orders stubs to informative "coming later" panels', async () => {
    renderShellAt('/projects');
    await waitFor(() => {
      expect(screen.getByText(/Coming in Phase 4/i)).toBeTruthy();
    });
  });
});
