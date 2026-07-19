import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
  useParams,
} from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      parseAndStoreImport: vi.fn(),
    },
  };
});

import type { ImportRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { UploadImport } from './UploadImport';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function err(error: { code: string; message: string }) {
  return Promise.resolve({ status: 'error' as const, error });
}

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

function renderUploadImport() {
  const rootRoute = createRootRoute();
  const ordersRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/orders',
    component: UploadImport,
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

/** A `File` whose `arrayBuffer()` resolves to known bytes — jsdom's `File`
 * doesn't implement `arrayBuffer()`, so this stubs it directly. */
function fileWithBytes(name: string, type: string, bytes: number[]): File {
  const file = new File([new Uint8Array(bytes)], name, { type });
  Object.defineProperty(file, 'arrayBuffer', {
    value: () => Promise.resolve(new Uint8Array(bytes).buffer),
  });
  return file;
}

describe('UploadImport', () => {
  it('selecting a file calls useParseImport with the exact bytes and filename', async () => {
    vi.mocked(commands.parseAndStoreImport).mockReturnValue(ok(importRecord()));
    renderUploadImport();

    const bytes = [1, 2, 3, 4, 5];
    const file = fileWithBytes('order.pdf', 'application/pdf', bytes);
    const input = await screen.findByLabelText('Upload order file');
    fireEvent.change(input, { target: { files: [file] } });

    await waitFor(() =>
      expect(commands.parseAndStoreImport).toHaveBeenCalledWith(bytes, 'order.pdf'),
    );
  });

  it('navigates to the new import on success', async () => {
    vi.mocked(commands.parseAndStoreImport).mockReturnValue(
      ok(importRecord({ id: 'imp42', supplier: 'DigiKey' })),
    );
    renderUploadImport();

    const file = fileWithBytes('order.csv', 'text/csv', [9, 9]);
    const input = await screen.findByLabelText('Upload order file');
    fireEvent.change(input, { target: { files: [file] } });

    await waitFor(() => expect(screen.getByText('Import review stub: imp42')).toBeTruthy());
  });

  it('shows the CommandError message and does not navigate on parse failure', async () => {
    vi.mocked(commands.parseAndStoreImport).mockReturnValue(
      err({ code: 'unsupported_import_format', message: 'could not detect a supported format' }),
    );
    renderUploadImport();

    const file = fileWithBytes('order.txt', 'text/plain', [1]);
    const input = await screen.findByLabelText('Upload order file');
    fireEvent.change(input, { target: { files: [file] } });

    await waitFor(() =>
      expect(screen.getByText(/could not detect a supported format/)).toBeTruthy(),
    );
    expect(screen.getByText(/Nothing was stored/)).toBeTruthy();
    expect(screen.queryByText(/Import review stub/)).toBeNull();
  });
});
