import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
  useParams,
} from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listImports: vi.fn(),
      parseAndStoreImport: vi.fn(),
    },
  };
});

import type { ImportRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { OrdersList } from './OrdersList';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

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

function importRecord(overrides: Partial<ImportRecord> = {}): ImportRecord {
  return {
    id: 'imp1',
    supplier: 'DigiKey',
    order_number: 'DK12345',
    invoice_number: null,
    shipment_number: null,
    order_date: '2026-07-01',
    currency: 'USD',
    source_format: 'pdf',
    status: 'parsed',
    subtotal_micros: 10_000_000,
    shipping_micros: 500_000,
    tax_micros: 0,
    tariff_micros: null,
    total_micros: 10_500_000,
    web_order_id: null,
    line_count: 6,
    created_at: '2026-07-15 12:00:00',
    ...overrides,
  };
}

function renderOrdersList() {
  const rootRoute = createRootRoute();
  const ordersRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/orders',
    component: OrdersList,
  });
  const orderReviewRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/orders/$importId',
    component: () => {
      const { importId } = useParams({ from: '/orders/$importId' });
      return <div>Import review stub: {importId}</div>;
    },
  });
  const routeTree = rootRoute.addChildren([ordersRoute, orderReviewRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/orders'] }),
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

describe('OrdersList', () => {
  it('renders every import with its status chip, source-format badge, and formatted total', async () => {
    vi.mocked(commands.listImports).mockReturnValue(
      ok([
        importRecord({
          id: 'imp1',
          supplier: 'DigiKey',
          status: 'parsed',
          total_micros: 10_500_000,
        }),
        importRecord({
          id: 'imp2',
          supplier: 'Mouser',
          status: 'committed',
          source_format: 'csv',
          order_number: null,
          invoice_number: 'INV-9',
          total_micros: 2_000_000,
        }),
      ]),
    );

    renderOrdersList();

    await waitFor(() => expect(screen.getByText('DigiKey')).toBeTruthy());
    const grid = within(screen.getByRole('grid', { name: 'Orders' }));
    expect(grid.getByText('Mouser')).toBeTruthy();
    expect(grid.getByText('Parsed')).toBeTruthy();
    expect(grid.getByText('Committed')).toBeTruthy();
    expect(grid.getByText('PDF')).toBeTruthy();
    expect(grid.getByText('CSV')).toBeTruthy();
    expect(grid.getByText('$10.50')).toBeTruthy();
    expect(grid.getByText('$2.00')).toBeTruthy();
    expect(grid.getByText('DK12345')).toBeTruthy();
    expect(grid.getByText('INV-9')).toBeTruthy();
  });

  it('shows the empty state and an always-visible upload zone when there are no imports', async () => {
    vi.mocked(commands.listImports).mockReturnValue(ok([]));
    renderOrdersList();

    await waitFor(() =>
      expect(
        screen.getByText(
          'No orders imported yet — upload a DigiKey order confirmation or invoice above to get started.',
        ),
      ).toBeTruthy(),
    );
    expect(screen.getByRole('button', { name: 'Choose file' })).toBeTruthy();
  });

  it('activating a row navigates to that import review route', async () => {
    vi.mocked(commands.listImports).mockReturnValue(ok([importRecord({ id: 'imp1' })]));
    renderOrdersList();

    await waitFor(() => expect(screen.getByText('DigiKey')).toBeTruthy());
    fireEvent.click(screen.getByText('DigiKey'));

    await waitFor(() => expect(screen.getByText('Import review stub: imp1')).toBeTruthy());
  });
});
