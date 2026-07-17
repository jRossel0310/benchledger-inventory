import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      search: vi.fn(),
      applyLedgerOp: vi.fn(),
    },
  };
});

// `RowActions`' Reserve/Check-out (Phase 3 Task 5) open the shared
// QuickAction dialog via this context; mocked here so this suite stays
// focused on the table itself rather than exercising the dialog too (that's
// `QuickAction.test.tsx`'s job).
vi.mock('../quick/QuickActionContext', () => ({
  useQuickAction: () => ({ open: vi.fn() }),
}));

// A row click opens the part-detail inspector drawer (Task 7) via this
// context; mocked here so this suite asserts on *what* the table calls
// rather than exercising the drawer itself (that's `PartInspector.test.tsx`'s
// job).
const openPartInspector = vi.fn();
vi.mock('../part/PartInspectorContext', () => ({
  usePartInspector: () => ({ open: openPartInspector }),
}));

import type { SearchHit } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { InventoryTable } from './InventoryTable';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

/** A search result promise the test controls the settling of — for
 * asserting on the state *between* a query change and its result landing
 * (e.g. that prior rows are still shown, not replaced by a loading state). */
function deferred<T>() {
  let resolve!: (data: T) => void;
  const promise = new Promise<{ status: 'ok'; data: T }>((res) => {
    resolve = (data: T) => res({ status: 'ok', data });
  });
  return { promise, resolve };
}

function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    part_id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_name: 'Resistor',
    bin_label: 'A10',
    available: 450_000,
    reserved: 50_000,
    checked_out: 0,
    low_stock_threshold: 100_000,
    archived: false,
    ...overrides,
  };
}

// @tanstack/react-virtual measures the scroll container via
// offsetHeight/offsetWidth; jsdom always reports 0, which would hide every
// row as "out of view" — pin realistic values for the suite, same fix as
// DataTable.test.tsx.
let offsetHeightSpy: ReturnType<typeof vi.spyOn>;
let offsetWidthSpy: ReturnType<typeof vi.spyOn>;

beforeAll(() => {
  offsetHeightSpy = vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(400);
  offsetWidthSpy = vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(1000);
});

afterAll(() => {
  offsetHeightSpy.mockRestore();
  offsetWidthSpy.mockRestore();
});

beforeEach(() => {
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
});

afterEach(cleanup);

/** A minimal router with just `/inventory` (hosting the table under test)
 * and a `/inventory/$partId` stub, so row-click navigation is real. */
function renderTable(
  query: string,
  extra?: { filter?: (hit: SearchHit) => boolean; emptyMessage?: string },
) {
  const rootRoute = createRootRoute();
  const inventoryRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory',
    component: () => <InventoryTable query={query} {...extra} />,
  });
  const partDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/$partId',
    component: () => <div>Part detail stub</div>,
  });
  const routeTree = rootRoute.addChildren([inventoryRoute, partDetailRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/inventory'] }),
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

/** Same shape as `renderTable`, but the query lives in the harness's own
 * state (with a button to change it) instead of being fixed for the whole
 * render — needed to observe the table's state *while* a query change is in
 * flight, which a fixed `query` prop can't exercise. */
function renderTableWithQuerySwitch(initialQuery: string, nextQuery: string) {
  const rootRoute = createRootRoute();
  const inventoryRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory',
    component: () => {
      const [query, setQuery] = useState(initialQuery);
      return (
        <div>
          <button type="button" onClick={() => setQuery(nextQuery)}>
            change query
          </button>
          <InventoryTable query={query} />
        </div>
      );
    },
  });
  const partDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/$partId',
    component: () => <div>Part detail stub</div>,
  });
  const routeTree = rootRoute.addChildren([inventoryRoute, partDetailRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/inventory'] }),
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('InventoryTable', () => {
  it('renders rows from a mocked search: part, category, bin, and the stock gauge', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));
    renderTable('10k');

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    expect(screen.getByText('Resistor')).toBeTruthy();
    expect(screen.getByText('A10')).toBeTruthy();
    // The gauge's accessible label carries the exact available/reserved/
    // checked-out split — verifying it also proves the row's real Quantity
    // values reached the gauge, without colliding with the separate
    // Avail/Resv/Out numeric columns rendering the same numbers as text.
    expect(
      screen.getByRole('img', { name: '450 available, 50 reserved, 0 checked out' }),
    ).toBeTruthy();
  });

  it('renders the Avail/Resv/Out/Bin columns', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));
    renderTable('10k');

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    expect(screen.getByRole('columnheader', { name: 'Avail' })).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: 'Resv' })).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: 'Out' })).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: 'Bin' })).toBeTruthy();
  });

  it('shows a low-stock chip when available is under the threshold', async () => {
    vi.mocked(commands.search).mockReturnValue(
      ok([hit({ available: 8_000, low_stock_threshold: 10_000 })]),
    );
    renderTable('low stock');

    await waitFor(() => expect(screen.getByText('Low')).toBeTruthy());
  });

  it('does not show a low-stock chip when there is no threshold configured', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit({ low_stock_threshold: null })]));
    renderTable('10k');

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    expect(screen.queryByText('Low')).toBeNull();
  });

  it('shows "No parts match" for a non-empty query with zero results', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    renderTable('zzz-nonexistent');

    await waitFor(() => expect(screen.getByText(/No parts match/i)).toBeTruthy());
  });

  it('shows "No parts yet" for a blank query with zero results (an empty database)', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    renderTable('');

    await waitFor(() => expect(screen.getByText(/No parts yet/i)).toBeTruthy());
    expect(commands.search).toHaveBeenCalledWith('');
  });

  it('opens the part-detail inspector drawer when a row is clicked, instead of navigating away', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit({ part_id: 'p42' })]));
    renderTable('10k');

    const nameCell = await screen.findByText('10k 0603 1% resistor');
    fireEvent.click(nameCell);

    expect(openPartInspector).toHaveBeenCalledWith('p42');
    // Losing list position defeats the point of the drawer — the table
    // itself must still be on screen, not replaced by a routed page.
    expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy();
  });

  it('opens the Add stock dialog from a row action', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));
    renderTable('10k');

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    const trigger = screen.getByRole('button', {
      name: /actions for 10k 0603 1% resistor/i,
    });
    // Radix's `DropdownMenu.Trigger` opens on `pointerdown`, not `click`.
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.click(trigger);

    const addStock = await screen.findByText('Add stock');
    fireEvent.click(addStock);

    expect(await screen.findByRole('dialog')).toBeTruthy();
  });

  it('keeps showing the previous rows while a new search is pending, instead of flickering to Loading', async () => {
    const firstHit = hit({ part_id: 'p1', display_name: '10k 0603 1% resistor' });
    const secondHit = hit({ part_id: 'p2', display_name: '20k 0805 1% resistor' });
    const second = deferred<SearchHit[]>();
    vi.mocked(commands.search).mockImplementation((query: string) =>
      query === '10k' ? ok([firstHit]) : second.promise,
    );

    renderTableWithQuerySwitch('10k', '20k');
    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());

    fireEvent.click(screen.getByText('change query'));

    // The new query ('20k') is now in flight, but its result hasn't landed
    // yet (the mocked promise is still unresolved) — the table must keep
    // showing the previous query's row via `placeholderData`, not unmount
    // into the "Loading inventory…" state.
    expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy();
    expect(screen.queryByText('Loading inventory…')).toBeNull();
    // A subtle pending cue is fine — losing the table is not.
    expect(screen.getByText('Updating…')).toBeTruthy();

    second.resolve([secondHit]);

    await waitFor(() => expect(screen.getByText('20k 0805 1% resistor')).toBeTruthy());
    expect(screen.queryByText('10k 0603 1% resistor')).toBeNull();
    expect(screen.queryByText('Updating…')).toBeNull();
  });

  it('applies an optional client-side filter to the search results (the Bin browser Unassigned bucket)', async () => {
    const binned = hit({ part_id: 'p1', display_name: 'Binned part', bin_label: 'A1' });
    const unbinned = hit({ part_id: 'p2', display_name: 'Unbinned part', bin_label: null });
    vi.mocked(commands.search).mockReturnValue(ok([binned, unbinned]));
    renderTable('', { filter: (row) => row.bin_label === null });

    await waitFor(() => expect(screen.getByText('Unbinned part')).toBeTruthy());
    expect(screen.queryByText('Binned part')).toBeNull();
  });

  it('uses a custom emptyMessage when provided, instead of the default copy', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([]));
    renderTable('', { emptyMessage: 'No unassigned parts — every part has a bin.' });

    await waitFor(() =>
      expect(screen.getByText('No unassigned parts — every part has a bin.')).toBeTruthy(),
    );
    expect(screen.queryByText(/No parts yet/i)).toBeNull();
  });
});
