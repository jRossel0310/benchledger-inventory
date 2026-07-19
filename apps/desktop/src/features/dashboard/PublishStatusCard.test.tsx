import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { PublishStatus } from '../../bindings.gen';
import { commands } from '../../lib/commands';
import { PublishStatusCard } from './PublishStatusCard';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function publishStatus(overrides: Partial<PublishStatus> = {}): PublishStatus {
  return {
    configured: false,
    repo: null,
    last_published_at: null,
    pending: false,
    vercel_url: null,
    ...overrides,
  };
}

// jsdom doesn't implement scrollTo; the router's scroll-restoration effect
// calls it on every navigation.
beforeEach(() => {
  window.scrollTo = vi.fn();
  vi.spyOn(commands, 'getPublishStatus').mockReturnValue(ok(publishStatus()));
});

afterEach(() => {
  vi.restoreAllMocks();
  cleanup();
});

/** The card is an `<li>` in the Dashboard's publish/backup sync list — the
 * harness renders it inside a bare `<ul>` with only the routes it links to. */
function renderCard() {
  const rootRoute = createRootRoute();
  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: () => (
      <ul>
        <PublishStatusCard />
      </ul>
    ),
  });
  const settingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/settings',
    component: () => <div>Settings stub</div>,
  });
  const routeTree = rootRoute.addChildren([indexRoute, settingsRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

describe('PublishStatusCard', () => {
  it('shows the not-configured state with a link to Settings', async () => {
    renderCard();

    expect(await screen.findByText(/Publishing not configured/)).toBeTruthy();
    const settingsLink = screen.getByRole('link', { name: 'Settings' });
    expect(settingsLink.getAttribute('href')).toBe('/settings');
    expect(screen.queryByText(/Publish pending/)).toBeNull();
  });

  it('shows the last-published time, normal-toned, when configured and not pending', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(
        publishStatus({
          configured: true,
          repo: 'example/site',
          last_published_at: '2026-07-18 09:30:00',
        }),
      ),
    );
    renderCard();

    const text = await screen.findByText(/Last published/);
    // Formatted timestamp, not the raw database string.
    expect(text.textContent).not.toContain('2026-07-18 09:30:00');
    expect(text.textContent).toContain('2026');
    expect(text.className).not.toContain('publish-status-card-warn');
    expect(screen.queryByText(/Publish pending/)).toBeNull();
  });

  it('shows an honest never-published state when configured but nothing has shipped', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(publishStatus({ configured: true, repo: 'example/site' })),
    );
    renderCard();

    expect(await screen.findByText(/Configured — nothing published yet/)).toBeTruthy();
  });

  it('shows the pending warning with a link to Settings when a publish is pending', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(
        publishStatus({
          configured: true,
          repo: 'example/site',
          last_published_at: '2026-07-18 09:30:00',
          pending: true,
        }),
      ),
    );
    renderCard();

    const warning = await screen.findByText(/Publish pending — will retry on launch/);
    expect(warning.className).toContain('publish-status-card-warn');
    const settingsLink = screen.getByRole('link', { name: 'Settings' });
    expect(settingsLink.getAttribute('href')).toBe('/settings');
  });

  it('falls back to the muted unavailable state when the status query fails', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      Promise.resolve({
        status: 'error' as const,
        error: { code: 'internal', message: 'boom' },
      }),
    );
    renderCard();

    await waitFor(() => expect(screen.getByText(/Publish status unavailable/)).toBeTruthy());
  });
});
