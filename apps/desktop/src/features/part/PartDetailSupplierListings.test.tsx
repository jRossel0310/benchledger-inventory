import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listVariants: vi.fn(),
      listSupplierListings: vi.fn(),
    },
  };
});

import type { ListingRecord, VariantRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { PartDetailSupplierListings } from './PartDetailSupplierListings';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function variant(overrides: Partial<VariantRecord> = {}): VariantRecord {
  return {
    id: 'v1',
    part_id: 'p1',
    manufacturer: 'Yageo',
    mpn: 'RC0603FR-0710KL',
    description: '',
    package: '0603',
    datasheet_url: null,
    product_url: null,
    lifecycle: null,
    is_preferred: true,
    notes: '',
    ...overrides,
  };
}

function listing(overrides: Partial<ListingRecord> = {}): ListingRecord {
  return {
    id: 'l1',
    variant_id: 'v1',
    supplier: 'Digikey',
    supplier_sku: '311-10.0KCRCT-ND',
    product_url: 'https://digikey.com/x',
    packaging: 'Cut Tape',
    typical_order: 100_000,
    last_unit_price_micros: 12_000,
    currency: 'USD',
    last_purchase_date: '2026-01-01',
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
});

afterEach(cleanup);

function renderListings() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <PartDetailSupplierListings partId="p1" />
    </QueryClientProvider>,
  );
}

describe('PartDetailSupplierListings', () => {
  it('renders listings grouped under their variant: supplier, SKU, price, and packaging', async () => {
    vi.mocked(commands.listVariants).mockReturnValue(ok([variant()]));
    vi.mocked(commands.listSupplierListings).mockReturnValue(ok([listing()]));

    renderListings();

    await waitFor(() => expect(screen.getByText('Digikey')).toBeTruthy());
    expect(screen.getByText('311-10.0KCRCT-ND')).toBeTruthy();
    expect(screen.getByText('$0.01')).toBeTruthy();
    expect(screen.getByText('Cut Tape')).toBeTruthy();
    // Grouped under the variant it belongs to.
    expect(screen.getByText('RC0603FR-0710KL')).toBeTruthy();
  });

  it('shows a per-variant empty state when a variant has no listings', async () => {
    vi.mocked(commands.listVariants).mockReturnValue(ok([variant()]));
    vi.mocked(commands.listSupplierListings).mockReturnValue(ok([]));

    renderListings();

    await waitFor(() => expect(screen.getByText(/no supplier listings/i)).toBeTruthy());
  });

  it('shows an empty state when the part has no variants at all', async () => {
    vi.mocked(commands.listVariants).mockReturnValue(ok([]));

    renderListings();

    await waitFor(() => expect(screen.getByText(/no manufacturer variants/i)).toBeTruthy());
  });
});
