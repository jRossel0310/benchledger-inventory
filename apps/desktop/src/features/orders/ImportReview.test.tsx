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
import { useState } from 'react';
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
      commitImport: vi.fn(),
      reverseImport: vi.fn(),
    },
  };
});

import type {
  GroupRecord,
  ImportId,
  ImportRecord,
  ImportReview as ImportReviewData,
  ImportReviewLine,
} from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { ImportReview } from './ImportReview';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
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

function group(overrides: Partial<GroupRecord> = {}): GroupRecord {
  return {
    id: 'g1',
    kind: 'import_commit',
    note: '',
    reversed_group_id: null,
    created_at: '2026-07-16 00:00:00',
    transactions: [
      {
        id: 't1',
        part_id: 'part1',
        group_id: 'g1',
        txn_type: 'receive',
        quantity: 5000,
        from_state: null,
        to_state: 'available',
        project_id: null,
        to_project_id: null,
        note: '',
        reversed_txn_id: null,
        created_at: '2026-07-16 00:00:00',
      },
    ],
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
  const ordersRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/orders',
    component: () => <div>Orders list stub</div>,
  });
  const routeTree = rootRoute.addChildren([reviewRoute, ordersRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/orders/imp1'] }),
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

/** Swaps which import `ImportReview` is mounted with, entirely via a prop
 * change (no route/query invalidation involved) — proves the reinit guard
 * in `ImportReview.tsx` keys off `review.import.id`, not merely "the query
 * refetched", so navigating from one import's review straight to another's
 * (e.g. via the Orders list) always rebuilds `decisions` from the NEW
 * import's proposals rather than carrying over stale entries keyed by a
 * line id that happens to collide between the two imports. Neither import
 * used here has a `duplicate_of` entry, so `ImportReview`'s only
 * router-`Link` usage (`DuplicateWarning`) never mounts — no `RouterProvider`
 * needed. */
function SwapHarness() {
  const [importId, setImportId] = useState<ImportId>('imp1');
  return (
    <>
      <button type="button" onClick={() => setImportId('imp2')}>
        Switch to import B
      </button>
      <ImportReview importId={importId} />
    </>
  );
}

function renderSwapHarness() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <SwapHarness />
      </ToastProvider>
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

  it('shows an error state with a link back to Orders when the review fails to load', async () => {
    vi.mocked(commands.getImportReview).mockReturnValue(
      commandError('not_found', 'import not found'),
    );
    renderImportReview();

    await waitFor(() => expect(screen.getByText(/Could not load this import/)).toBeTruthy());
    fireEvent.click(screen.getByText('Back to Orders'));
    await waitFor(() => expect(screen.getByText('Orders list stub')).toBeTruthy());
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
    expect(screen.getByRole('button', { name: 'Draft incomplete' })).toBeTruthy();
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
    expect(screen.getByRole('button', { name: 'Draft incomplete' })).toBeTruthy();
  });

  it('swapping to a different import reinitializes decisions from its own proposals, with no leakage from the prior import', async () => {
    // Import B deliberately reuses line id "line1" so a bug that merged
    // instead of rebuilding the decisions map would surface as a leaked
    // "Skip" decision here.
    const importB = importRecord({ id: 'imp2', order_number: 'DK-B99' });
    const lineB = partLine({
      line_id: 'line1',
      supplier_sku: 'DK999',
      mpn: 'MPN9',
      description: 'A totally different part',
      matches: [],
      proposed: { type: 'create_new' },
    });
    const reviewB: ImportReviewData = {
      import: importB,
      lines: [lineB],
      duplicate_of: [],
      total_receive_lines: 1,
    };

    vi.mocked(commands.getImportReview).mockImplementation((id: string) =>
      id === 'imp2' ? ok(reviewB) : ok(reviewData()),
    );
    renderSwapHarness();

    await waitFor(() => expect(screen.getByText('DK123')).toBeTruthy());
    const line1Row = screen.getByText('DK123').closest('tr') as HTMLElement;
    fireEvent.click(within(line1Row).getByRole('button', { name: /Change decision/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Skip' }));
    await waitFor(() => expect(within(line1Row).getByText('Skip')).toBeTruthy());

    fireEvent.click(screen.getByRole('button', { name: 'Switch to import B' }));

    // Import B's own line (same line_id "line1") shows up with ITS proposed
    // action (create_new -> incomplete draft placeholder), not a leaked
    // "Skip" carried over from import A's edited decision.
    await waitFor(() => expect(screen.getByText('DK999')).toBeTruthy());
    expect(screen.queryByText('DK123')).toBeNull();
    expect(screen.getByRole('button', { name: 'Draft incomplete' })).toBeTruthy();
    expect(screen.queryByText('Skip')).toBeNull();
  });

  describe('commit bar (Task 4)', () => {
    function commitButton() {
      return screen.getByRole('button', { name: /Commit import|Committing…/ }) as HTMLButtonElement;
    }

    it('shows summary counts derived from the decisions map, excluding a zero-shipped receive', async () => {
      // line1: add_stock, receive_qty 5000 -> a receive.
      // line2: create_new (incomplete), receive_qty 5000 -> a receive + a new part.
      // line3: add_stock, receive_qty 0 -> NOT a receive (zero-shipped excluded).
      // line4: fee -> non-inventory.
      const zeroShipped = partLine({
        line_id: 'line3',
        line_number: 3,
        receive_qty_milli: 0,
        proposed: { type: 'add_stock_to_existing', part_id: 'part1', verdict_kind: 'exact_sku' },
      });
      const fee: ImportReviewLine = {
        line_id: 'line4',
        line_number: 4,
        kind: 'fee',
        supplier_sku: null,
        mpn: null,
        manufacturer: null,
        description: 'Shipping',
        receive_qty_milli: null,
        ordered_milli: null,
        backordered_milli: null,
        unit_price_micros: 500_000,
        matches: [],
        proposed: { type: 'non_inventory' },
        warning: null,
      };
      vi.mocked(commands.getImportReview).mockReturnValue(
        ok(reviewData({ lines: [partLine(), secondPartLine(), zeroShipped, fee] })),
      );
      renderImportReview();

      await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
      expect(screen.getByText('2 receives')).toBeTruthy();
      expect(screen.getByText('1 new part')).toBeTruthy();
      expect(screen.getByText('1 non-inventory')).toBeTruthy();
    });

    it('disables Commit while any part line is an incomplete create_new draft, and enables it once completed', async () => {
      vi.mocked(commands.getImportReview).mockReturnValue(ok(reviewData()));
      renderImportReview();
      await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
      expect(commitButton().disabled).toBe(true);

      // Skip line2 instead of completing its draft -> no more incomplete drafts.
      const line2Row = screen.getByText('DK456').closest('tr') as HTMLElement;
      fireEvent.click(within(line2Row).getByRole('button', { name: /Change decision/ }));
      fireEvent.click(await screen.findByRole('button', { name: 'Skip' }));

      await waitFor(() => expect(commitButton().disabled).toBe(false));
    });

    it('Commit calls useCommitImport with the exact decisions the map holds, as [lineId, decision] pairs', async () => {
      // Both lines resolve to clean, complete decisions from `proposed` —
      // line1 add_stock (no dialog needed), line2 skip (chosen explicitly).
      vi.mocked(commands.getImportReview).mockReturnValue(
        ok(
          reviewData({
            lines: [partLine(), secondPartLine({ proposed: { type: 'ignore' } })],
          }),
        ),
      );
      vi.mocked(commands.commitImport).mockReturnValue(ok(group()));
      renderImportReview();
      await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
      await waitFor(() => expect(commitButton().disabled).toBe(false));

      fireEvent.click(commitButton());

      await waitFor(() => expect(commands.commitImport).toHaveBeenCalledTimes(1));
      expect(commands.commitImport).toHaveBeenCalledWith('imp1', [
        ['line1', { type: 'add_stock', part_id: 'part1' }],
        ['line2', { type: 'skip' }],
      ]);
    });

    it('a successful commit shows "Received N lines" and freezes the editors once the review refetches as committed', async () => {
      vi.mocked(commands.getImportReview)
        .mockReturnValueOnce(
          ok(reviewData({ lines: [partLine(), secondPartLine({ proposed: { type: 'ignore' } })] })),
        )
        .mockReturnValue(
          ok(
            reviewData({
              import: importRecord({ status: 'committed' }),
              lines: [partLine(), secondPartLine({ proposed: { type: 'ignore' } })],
            }),
          ),
        );
      vi.mocked(commands.commitImport).mockReturnValue(ok(group()));
      renderImportReview();
      await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
      await waitFor(() => expect(commitButton().disabled).toBe(false));

      fireEvent.click(commitButton());

      await waitFor(() => expect(screen.getByText('Received 1 line')).toBeTruthy());
      await waitFor(() => {
        const triggers = screen.getAllByRole('button', { name: /Change decision/ });
        expect(triggers.every((t) => (t as HTMLButtonElement).disabled)).toBe(true);
      });
      expect(screen.getByRole('button', { name: 'Reverse import' })).toBeTruthy();
    });

    it('a commit failure surfaces the CommandError message plainly', async () => {
      vi.mocked(commands.getImportReview).mockReturnValue(
        ok(reviewData({ lines: [partLine(), secondPartLine({ proposed: { type: 'ignore' } })] })),
      );
      vi.mocked(commands.commitImport).mockReturnValue(
        commandError('insufficient_stock', 'not enough stock'),
      );
      renderImportReview();
      await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
      await waitFor(() => expect(commitButton().disabled).toBe(false));

      fireEvent.click(commitButton());

      await waitFor(() => expect(screen.getByText('Could not commit this import')).toBeTruthy());
      // Nothing froze — the import is still `parsed`, editors stay live.
      const triggers = screen.getAllByRole('button', { name: /Change decision/ });
      expect(triggers.every((t) => (t as HTMLButtonElement).disabled)).toBe(false);
    });
  });

  describe('reverse (Task 4)', () => {
    it('confirming Reverse import calls useReverseImport with the fixed note, and the status flips to reversed', async () => {
      vi.mocked(commands.getImportReview)
        .mockReturnValueOnce(ok(reviewData({ import: importRecord({ status: 'committed' }) })))
        .mockReturnValue(ok(reviewData({ import: importRecord({ status: 'reversed' }) })));
      vi.mocked(commands.reverseImport).mockReturnValue(ok(group()));
      renderImportReview();

      await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
      fireEvent.click(screen.getByRole('button', { name: 'Reverse import' }));
      await waitFor(() => expect(screen.getByRole('dialog')).toBeTruthy());
      fireEvent.click(
        within(screen.getByRole('dialog')).getByRole('button', { name: 'Reverse import' }),
      );

      await waitFor(() =>
        expect(commands.reverseImport).toHaveBeenCalledWith('imp1', expect.any(String)),
      );
      await waitFor(() => expect(screen.getByText('Import reversed')).toBeTruthy());
      await waitFor(() =>
        expect(screen.queryByRole('button', { name: 'Reverse import' })).toBeNull(),
      );
    });

    it('Cancel on the reverse confirmation does not call useReverseImport', async () => {
      vi.mocked(commands.getImportReview).mockReturnValue(
        ok(reviewData({ import: importRecord({ status: 'committed' }) })),
      );
      renderImportReview();

      await waitFor(() => expect(screen.getByText('DigiKey order')).toBeTruthy());
      fireEvent.click(screen.getByRole('button', { name: 'Reverse import' }));
      await waitFor(() => expect(screen.getByRole('dialog')).toBeTruthy());
      fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' }));

      expect(screen.queryByRole('dialog')).toBeNull();
      expect(commands.reverseImport).not.toHaveBeenCalled();
    });
  });
});
