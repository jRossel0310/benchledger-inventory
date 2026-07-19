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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      getImportReview: vi.fn(),
      search: vi.fn(),
      getPart: vi.fn(),
    },
  };
});

import type {
  ImportRecord,
  ImportReview as ImportReviewData,
  ImportReviewLine,
} from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ImportReview } from './ImportReview';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

beforeEach(() => {
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
  vi.mocked(commands.search).mockReturnValue(ok([]));
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
    line_count: 2,
    created_at: '2026-07-15 12:00:00',
    ...overrides,
  };
}

function partLine(overrides: Partial<ImportReviewLine> = {}): ImportReviewLine {
  return {
    line_id: 'line1',
    line_number: 1,
    kind: 'part',
    supplier_sku: 'DK123',
    mpn: 'MPN1',
    manufacturer: 'Acme',
    description: 'Resistor 1k',
    receive_qty_milli: 5000,
    ordered_milli: 5000,
    backordered_milli: 0,
    unit_price_micros: 100_000,
    matches: [
      {
        part_id: 'part1',
        display_name: 'Existing 1k resistor',
        verdict_kind: 'exact_sku',
        explanation: 'Same supplier SKU on file.',
        rank: 1,
      },
    ],
    proposed: { type: 'add_stock_to_existing', part_id: 'part1', verdict_kind: 'exact_sku' },
    warning: null,
    ...overrides,
  };
}

function secondPartLine(overrides: Partial<ImportReviewLine> = {}): ImportReviewLine {
  return partLine({
    line_id: 'line2',
    line_number: 2,
    supplier_sku: 'DK456',
    mpn: 'MPN2',
    description: 'Capacitor 10uF',
    matches: [],
    proposed: { type: 'create_new' },
    ...overrides,
  });
}

function reviewData(overrides: Partial<ImportReviewData> = {}): ImportReviewData {
  return {
    import: importRecord(),
    lines: [partLine(), secondPartLine()],
    duplicate_of: [],
    total_receive_lines: 2,
    ...overrides,
  };
}

function renderImportReview() {
  const rootRoute = createRootRoute();
  const reviewRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/orders/$importId',
    component: () => {
      const { importId } = useParams({ from: '/orders/$importId' });
      return importId === 'imp1' ? (
        <ImportReview importId="imp1" />
      ) : (
        <div>Prior import stub: {importId}</div>
      );
    },
  });
  const routeTree = rootRoute.addChildren([reviewRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/orders/imp1'] }),
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

describe('ImportReview', () => {
  it('renders order metadata and the financial block', async () => {
    vi.mocked(commands.getImportReview).mockReturnValue(ok(reviewData()));
    renderImportReview();

    await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
    expect(screen.getByText('DK12345')).toBeTruthy();
    expect(screen.getByText('2026-07-01')).toBeTruthy();
    expect(screen.getByText('$10.00')).toBeTruthy(); // subtotal
    expect(screen.getByText('$0.50')).toBeTruthy(); // shipping
    expect(screen.getByText('$10.50')).toBeTruthy(); // total
    expect(screen.getByText('2 will receive stock')).toBeTruthy();
  });

  it('shows a backorder count when any line is backordered', async () => {
    vi.mocked(commands.getImportReview).mockReturnValue(
      ok(reviewData({ lines: [partLine({ backordered_milli: 2000 }), secondPartLine()] })),
    );
    renderImportReview();
    await waitFor(() => expect(screen.getByText('1 line backordered')).toBeTruthy());
  });

  it('shows no backorder count when nothing is backordered', async () => {
    vi.mocked(commands.getImportReview).mockReturnValue(ok(reviewData()));
    renderImportReview();
    await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
    expect(screen.queryByText(/backordered/)).toBeNull();
  });

  it('shows a prominent, non-blocking duplicate warning linking to prior imports', async () => {
    const prior = importRecord({
      id: 'imp0',
      order_number: 'DK-OLD',
      created_at: '2026-06-01 00:00:00',
    });
    vi.mocked(commands.getImportReview).mockReturnValue(ok(reviewData({ duplicate_of: [prior] })));
    renderImportReview();

    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy());
    expect(screen.getByText(/Possible duplicate/)).toBeTruthy();
    expect(screen.getByText(/You can still review and commit/)).toBeTruthy();
    const link = screen.getByRole('link', { name: /DK-OLD/ });
    fireEvent.click(link);
    await waitFor(() => expect(screen.getByText('Prior import stub: imp0')).toBeTruthy());
  });

  it("initializes decisions from each line's proposed action", async () => {
    vi.mocked(commands.getImportReview).mockReturnValue(ok(reviewData()));
    renderImportReview();

    await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
    // line1's proposed is add_stock_to_existing part1, whose display name is
    // in its own matches -> resolved without a further usePart query.
    expect(screen.getByText('Add stock to Existing 1k resistor')).toBeTruthy();
    // line2's proposed is create_new -> placeholder draft, flagged incomplete.
    expect(screen.getByText('Draft incomplete')).toBeTruthy();
  });

  it("changing one line's decision does not clobber the other line's decision", async () => {
    vi.mocked(commands.getImportReview).mockReturnValue(ok(reviewData()));
    renderImportReview();

    await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
    const line1Row = screen.getByText('DK123').closest('tr') as HTMLElement;
    fireEvent.click(within(line1Row).getByRole('button', { name: /Change decision/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Skip' }));

    // line1 is now Skip...
    await waitFor(() => expect(within(line1Row).getByText('Skip')).toBeTruthy());
    // ...but line2's independently-initialized decision is untouched.
    expect(screen.getByText('Draft incomplete')).toBeTruthy();
  });
});
