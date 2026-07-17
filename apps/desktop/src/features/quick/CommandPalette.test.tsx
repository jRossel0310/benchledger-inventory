import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      search: vi.fn(),
    },
  };
});

const openQuickAction = vi.fn();
vi.mock('./QuickActionContext', () => ({
  useQuickAction: () => ({ open: openQuickAction }),
}));

import type { SearchHit } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { CommandPalette } from './CommandPalette';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    part_id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_name: 'Resistor',
    bin_label: 'A12',
    available: 450_000,
    reserved: 50_000,
    checked_out: 0,
    low_stock_threshold: 100_000,
    archived: false,
    ...overrides,
  };
}

beforeEach(() => {
  // TanStack Router's scroll restoration calls `window.scrollTo` on every
  // navigation, which jsdom doesn't implement (throws "Not implemented").
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
});

afterEach(cleanup);

/** A minimal router hosting `CommandPalette` at the root plus every route it
 * can navigate to, so palette selections are real navigations (same pattern
 * as `InventoryTable.test.tsx`). Awaits `router.load()` before rendering:
 * `RouterProvider` resolves its initial route match asynchronously, so
 * firing a `Ctrl+K` keydown synchronously right after `render()` can race
 * ahead of `CommandPalette` even mounting (and therefore its keydown
 * listener registering) — `router.load()` primes the match first so the
 * root route's component (and this test's `openPalette()` calls) always run
 * against an already-mounted tree. */
async function renderPalette() {
  const rootRoute = createRootRoute({
    component: () => (
      <>
        <CommandPalette />
        <Outlet />
      </>
    ),
  });
  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: () => <div>Dashboard stub</div>,
  });
  const inventoryRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory',
    component: function InventoryStub() {
      return <div>Inventory stub</div>;
    },
    validateSearch: (search: Record<string, unknown>) => ({
      q: typeof search.q === 'string' ? search.q : '',
    }),
  });
  const partDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/$partId',
    component: () => <div>Part detail stub</div>,
  });
  const newPartRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/new',
    component: () => <div>Create part stub</div>,
  });
  const ordersRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/orders',
    component: () => <div>Orders stub</div>,
  });
  const routeTree = rootRoute.addChildren([
    indexRoute,
    inventoryRoute,
    partDetailRoute,
    newPartRoute,
    ordersRoute,
  ]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  });
  await router.load();
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

function openPalette() {
  fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
}

describe('CommandPalette', () => {
  it('is closed until Ctrl+K is pressed', async () => {
    await renderPalette();
    expect(screen.queryByRole('combobox')).toBeNull();
  });

  it('opens on Ctrl+K and closes on a second Ctrl+K', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    await renderPalette();

    openPalette();
    expect(await screen.findByRole('combobox')).toBeTruthy();

    openPalette();
    await waitFor(() => expect(screen.queryByRole('combobox')).toBeNull());
  });

  it('opens on Cmd+K (metaKey) too', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    await renderPalette();

    fireEvent.keyDown(window, { key: 'k', metaKey: true });
    expect(await screen.findByRole('combobox')).toBeTruthy();
  });

  it('closes on Escape', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    await renderPalette();
    openPalette();
    expect(await screen.findByRole('combobox')).toBeTruthy();

    fireEvent.keyDown(screen.getByRole('combobox'), { key: 'Escape' });

    await waitFor(() => expect(screen.queryByRole('combobox')).toBeNull());
  });

  it('lists every quick action by default (empty query)', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    await renderPalette();
    openPalette();

    await screen.findByRole('combobox');
    expect(screen.getByText('Add stock')).toBeTruthy();
    expect(screen.getByText('Consume')).toBeTruthy();
    expect(screen.getByText('Reserve for project')).toBeTruthy();
    expect(screen.getByText('Release reservation')).toBeTruthy();
    expect(screen.getByText('Check out')).toBeTruthy();
    expect(screen.getByText('Return')).toBeTruthy();
    expect(screen.getByText('Create part')).toBeTruthy();
    expect(screen.getByText('Import order')).toBeTruthy();
  });

  it('filters actions by typed text', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    await renderPalette();
    openPalette();
    const input = await screen.findByRole('combobox');

    fireEvent.change(input, { target: { value: 'reserve' } });

    await waitFor(() => expect(screen.getByText('Reserve for project')).toBeTruthy());
    // "Release reservation" does not contain the substring "reserve" (its
    // stem is "reservation"), so it should be filtered out.
    expect(screen.queryByText('Release reservation')).toBeNull();
    expect(screen.queryByText('Consume')).toBeNull();
  });

  it('selecting a quick action closes the palette and opens the QuickAction dialog with no preselected part', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    await renderPalette();
    openPalette();

    fireEvent.click(await screen.findByText('Add stock'));

    expect(openQuickAction).toHaveBeenCalledWith({ kind: 'receive' });
    await waitFor(() => expect(screen.queryByRole('combobox')).toBeNull());
  });

  it('fuzzy-matches parts via useSearch as the query changes', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));
    await renderPalette();
    openPalette();
    const input = await screen.findByRole('combobox');

    fireEvent.change(input, { target: { value: '10k' } });

    await waitFor(() => expect(commands.search).toHaveBeenCalledWith('10k'));
    expect(await screen.findByText('10k 0603 1% resistor')).toBeTruthy();
  });

  it('selecting a part navigates to its detail route', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit({ part_id: 'p42' })]));
    await renderPalette();
    openPalette();
    const input = await screen.findByRole('combobox');
    fireEvent.change(input, { target: { value: '10k' } });

    fireEvent.click(await screen.findByText('10k 0603 1% resistor'));

    await waitFor(() => expect(screen.getByText('Part detail stub')).toBeTruthy());
  });

  it('shows a Bins group for distinct bin labels matching the query and navigates via a bin: filter', async () => {
    vi.mocked(commands.search).mockReturnValue(
      ok([
        hit({ part_id: 'p1', display_name: 'Resistor in A12', bin_label: 'A12' }),
        hit({ part_id: 'p2', display_name: 'Capacitor also in A12', bin_label: 'A12' }),
        hit({ part_id: 'p3', display_name: 'Something in B02', bin_label: 'B02' }),
      ]),
    );
    const router = await renderPalette();
    openPalette();
    fireEvent.change(await screen.findByRole('combobox'), { target: { value: 'a12' } });

    // A12 appears once (deduped) even though two hits share it; B02 (which
    // doesn't match the "a12" query) is excluded.
    expect(await screen.findByText('Bin A12')).toBeTruthy();
    expect(screen.queryByText('Bin B02')).toBeNull();

    fireEvent.click(screen.getByText('Bin A12'));

    await waitFor(() => expect(screen.getByText('Inventory stub')).toBeTruthy());
    await waitFor(() =>
      expect(router.state.location.search).toEqual(expect.objectContaining({ q: 'bin:A12' })),
    );
  });

  it('Create part navigates to the create-part stub route and closes the palette', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    await renderPalette();
    openPalette();

    fireEvent.click(await screen.findByText('Create part'));

    await waitFor(() => expect(screen.getByText('Create part stub')).toBeTruthy());
    expect(screen.queryByRole('combobox')).toBeNull();
  });

  it('Import order navigates to the Orders route', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    await renderPalette();
    openPalette();

    fireEvent.click(await screen.findByText('Import order'));

    await waitFor(() => expect(screen.getByText('Orders stub')).toBeTruthy());
  });
});
