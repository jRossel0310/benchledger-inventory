import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
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
      dashboardSummary: vi.fn(),
      recentTransactions: vi.fn(),
      reverseTransaction: vi.fn(),
    },
  };
});

import type { DashboardSummary, RecentTxn } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { Dashboard } from './Dashboard';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

const FULL_SUMMARY: DashboardSummary = {
  available_units: 125_000,
  part_count: 42,
  reserved_units: 30_000,
  checked_out_units: 5_000,
  low_stock_count: 3,
  active_project_count: 2,
  metadata_incomplete_count: 7,
  unbinned_count: 4,
};

const EMPTY_SUMMARY: DashboardSummary = {
  available_units: 0,
  part_count: 0,
  reserved_units: 0,
  checked_out_units: 0,
  low_stock_count: 0,
  active_project_count: 0,
  metadata_incomplete_count: 0,
  unbinned_count: 0,
};

function recentTxn(overrides: Partial<RecentTxn>): RecentTxn {
  return {
    id: 'txn-1',
    part_id: 'part-1',
    display_name: 'Test resistor',
    txn_type: 'receive',
    quantity: 5000,
    quantity_unit: 'each',
    created_at: '2026-07-15 10:00:00',
    group_id: null,
    reversible: true,
    ...overrides,
  };
}

// jsdom doesn't implement scrollTo; the router's scroll-restoration effect
// calls it on every navigation.
beforeEach(() => {
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
});

afterEach(cleanup);

/** A minimal router covering only the routes Dashboard links to, plus a
 * fresh QueryClient per render so mocked command results from one test
 * never leak into the next via a shared cache. */
function renderDashboard() {
  const rootRoute = createRootRoute();
  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: Dashboard,
  });
  const inventoryRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory',
    component: () => <div>Inventory stub</div>,
  });
  const partDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/$partId',
    component: () => <div>Part detail stub</div>,
  });
  const projectsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/projects',
    component: () => <div>Projects stub</div>,
  });
  const settingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/settings',
    component: () => <div>Settings stub</div>,
  });
  const routeTree = rootRoute.addChildren([
    indexRoute,
    inventoryRoute,
    partDetailRoute,
    projectsRoute,
    settingsRoute,
  ]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/'] }),
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

describe('Dashboard', () => {
  it('renders every summary card from dashboard_summary', async () => {
    vi.mocked(commands.dashboardSummary).mockReturnValue(ok(FULL_SUMMARY));
    vi.mocked(commands.recentTransactions).mockReturnValue(ok([]));

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('125')).toBeTruthy(); // available units
    });
    expect(screen.getByText('42 parts')).toBeTruthy();
    expect(screen.getByText('30')).toBeTruthy(); // reserved units
    expect(screen.getByText('5')).toBeTruthy(); // checked-out units
    expect(screen.getByText('3')).toBeTruthy(); // low-stock count
    expect(screen.getByText('2')).toBeTruthy(); // active projects
    expect(screen.getByText('7')).toBeTruthy(); // metadata-incomplete count
    expect(screen.getByText('4')).toBeTruthy(); // unbinned count
  });

  it('renders the aggregate stock gauge split', async () => {
    vi.mocked(commands.dashboardSummary).mockReturnValue(ok(FULL_SUMMARY));
    vi.mocked(commands.recentTransactions).mockReturnValue(ok([]));

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('125 available')).toBeTruthy();
    });
    expect(screen.getByText('30 reserved')).toBeTruthy();
    expect(screen.getByText('5 checked out')).toBeTruthy();
  });

  it('links summary cards to the relevant filtered inventory view', async () => {
    vi.mocked(commands.dashboardSummary).mockReturnValue(ok(FULL_SUMMARY));
    vi.mocked(commands.recentTransactions).mockReturnValue(ok([]));

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('125')).toBeTruthy();
    });

    const lowStockLink = screen.getByText('3').closest('a');
    expect(lowStockLink?.getAttribute('href')).toContain('low');
    expect(lowStockLink?.getAttribute('href')).toContain('stock');

    const activeProjectsLink = screen.getByText('2').closest('a');
    expect(activeProjectsLink?.getAttribute('href')).toBe('/projects');
  });

  it('renders the recent-activity feed with part, action, quantity, and time', async () => {
    vi.mocked(commands.dashboardSummary).mockReturnValue(ok(FULL_SUMMARY));
    vi.mocked(commands.recentTransactions).mockReturnValue(
      ok([
        recentTxn({
          id: 'txn-receive',
          display_name: '10k 0603 resistor',
          txn_type: 'receive',
          quantity: 25_000,
          quantity_unit: 'each',
        }),
      ]),
    );

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('Received')).toBeTruthy();
    });
    expect(screen.getByText('25')).toBeTruthy();
    expect(screen.getByText('10k 0603 resistor')).toBeTruthy();
  });

  it('shows the reverse action only on reversible rows, and hides it otherwise', async () => {
    vi.mocked(commands.dashboardSummary).mockReturnValue(ok(FULL_SUMMARY));
    vi.mocked(commands.recentTransactions).mockReturnValue(
      ok([
        recentTxn({ id: 'txn-reversible', display_name: 'Reversible part', reversible: true }),
        recentTxn({ id: 'txn-grouped', display_name: 'Grouped part', reversible: false }),
      ]),
    );

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('Reversible part')).toBeTruthy();
    });
    expect(screen.getByText('Grouped part')).toBeTruthy();
    // Exactly one reverse button — for the reversible row only.
    expect(screen.getAllByRole('button', { name: 'Reverse' })).toHaveLength(1);
  });

  it('calls the reverse mutation and toasts success on click', async () => {
    vi.mocked(commands.dashboardSummary).mockReturnValue(ok(FULL_SUMMARY));
    vi.mocked(commands.recentTransactions).mockReturnValue(
      ok([recentTxn({ id: 'txn-to-reverse', display_name: 'Reverse me' })]),
    );
    vi.mocked(commands.reverseTransaction).mockReturnValue(
      ok({
        id: 'txn-reversal',
        part_id: 'part-1',
        group_id: null,
        txn_type: 'reverse',
        quantity: 5000,
        from_state: 'available',
        to_state: 'available',
        project_id: null,
        to_project_id: null,
        note: 'Reversed from the dashboard',
        reversed_txn_id: 'txn-to-reverse',
        created_at: '2026-07-15 10:05:00',
      }),
    );

    renderDashboard();

    const reverseButton = await screen.findByRole('button', { name: 'Reverse' });
    fireEvent.click(reverseButton);

    await waitFor(() => {
      expect(commands.reverseTransaction).toHaveBeenCalledWith(
        'txn-to-reverse',
        'Reversed from the dashboard',
      );
    });
    await waitFor(() => {
      expect(screen.getByText('Transaction reversed')).toBeTruthy();
    });
  });

  it('toasts an error when the reverse mutation fails', async () => {
    vi.mocked(commands.dashboardSummary).mockReturnValue(ok(FULL_SUMMARY));
    vi.mocked(commands.recentTransactions).mockReturnValue(
      ok([recentTxn({ id: 'txn-fails', display_name: 'Will fail' })]),
    );
    vi.mocked(commands.reverseTransaction).mockReturnValue(
      commandError('already_reversed', 'transaction was already reversed'),
    );

    renderDashboard();

    const reverseButton = await screen.findByRole('button', { name: 'Reverse' });
    fireEvent.click(reverseButton);

    await waitFor(() => {
      expect(screen.getByText('Could not reverse transaction')).toBeTruthy();
    });
    expect(screen.getByText('This transaction or group was already reversed.')).toBeTruthy();
  });

  it('shows an inviting empty state instead of zeroed cards when there are no parts', async () => {
    vi.mocked(commands.dashboardSummary).mockReturnValue(ok(EMPTY_SUMMARY));
    vi.mocked(commands.recentTransactions).mockReturnValue(ok([]));

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('No parts yet')).toBeTruthy();
    });
    expect(screen.queryByText('Inventory at a glance')).toBeNull();
  });
});
