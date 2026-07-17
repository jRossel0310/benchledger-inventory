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
      getPart: vi.fn(),
      getStock: vi.fn(),
      listCategories: vi.fn(),
      getAttributes: vi.fn(),
      categoryAttributeDefs: vi.fn(),
      getTags: vi.fn(),
      listDimensions: vi.fn(),
      listVariants: vi.fn(),
      listTransactions: vi.fn(),
      listProjects: vi.fn(),
    },
  };
});

vi.mock('../quick/QuickActionContext', () => ({
  useQuickAction: () => ({ open: vi.fn() }),
}));

import type { PartRecord, PartStockRow } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { PartInspector } from './PartInspector';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function part(overrides: Partial<PartRecord> = {}): PartRecord {
  return {
    id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_id: 'cat-resistor',
    description: '',
    bin_label: 'A12',
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each',
    low_stock_threshold: null,
    public_notes: '',
    private_notes: '',
    metadata_complete: true,
    archived: false,
    created_at: '2026-01-01 00:00:00',
    modified_at: '2026-01-01 00:00:00',
    ...overrides,
  };
}

function stock(): PartStockRow {
  return {
    available: 100_000,
    reserved: 0,
    checked_out: 0,
    lifetime_received: 100_000,
    lifetime_consumed: 0,
  };
}

function mockDefaults() {
  vi.mocked(commands.getPart).mockReturnValue(ok(part()));
  vi.mocked(commands.getStock).mockReturnValue(ok(stock()));
  vi.mocked(commands.listCategories).mockReturnValue(ok([]));
  vi.mocked(commands.getAttributes).mockReturnValue(ok([]));
  vi.mocked(commands.categoryAttributeDefs).mockReturnValue(ok([]));
  vi.mocked(commands.getTags).mockReturnValue(ok([]));
  vi.mocked(commands.listDimensions).mockReturnValue(ok([]));
  vi.mocked(commands.listVariants).mockReturnValue(ok([]));
  vi.mocked(commands.listTransactions).mockReturnValue(ok([]));
  vi.mocked(commands.listProjects).mockReturnValue(ok([]));
}

beforeEach(() => {
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
});

afterEach(cleanup);

function renderInspector(onClose = vi.fn()) {
  const rootRoute = createRootRoute();
  const inventoryRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory',
    component: () => <PartInspector partId="p1" onClose={onClose} />,
  });
  const partDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/$partId',
    component: () => <div>Part detail page stub</div>,
  });
  const editRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/$partId/edit',
    component: () => <div>Edit part stub</div>,
  });
  const routeTree = rootRoute.addChildren([inventoryRoute, partDetailRoute, editRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/inventory'] }),
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return {
    onClose,
    ...render(
      <QueryClientProvider client={queryClient}>
        <ToastProvider>
          <RouterProvider router={router} />
        </ToastProvider>
      </QueryClientProvider>,
    ),
  };
}

describe('PartInspector', () => {
  it('renders as a dialog containing the PartDetail body for the given part', async () => {
    mockDefaults();
    renderInspector();

    expect(await screen.findByRole('dialog')).toBeTruthy();
    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
  });

  it('calls onClose when the close control is clicked', async () => {
    mockDefaults();
    const { onClose } = renderInspector();

    const closeButton = await screen.findByRole('button', { name: /close/i });
    fireEvent.click(closeButton);

    expect(onClose).toHaveBeenCalled();
  });

  it('calls onClose on Escape', async () => {
    mockDefaults();
    const { onClose } = renderInspector();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape', code: 'Escape' });

    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it('the "Open full page" link navigates to the standalone route and closes the drawer', async () => {
    mockDefaults();
    const { onClose } = renderInspector();

    const link = await screen.findByRole('link', { name: /open full page/i });
    fireEvent.click(link);

    expect(onClose).toHaveBeenCalled();
  });
});
