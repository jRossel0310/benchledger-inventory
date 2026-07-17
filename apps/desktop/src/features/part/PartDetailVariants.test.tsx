import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listVariants: vi.fn(),
      setPreferredVariant: vi.fn(),
    },
  };
});

import type { VariantRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { PartDetailVariants } from './PartDetailVariants';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function variant(overrides: Partial<VariantRecord> = {}): VariantRecord {
  return {
    id: 'v1',
    part_id: 'p1',
    manufacturer: 'Yageo',
    mpn: 'RC0603FR-0710KL',
    description: '10k 1% 0603 chip resistor',
    package: '0603',
    datasheet_url: 'https://example.com/datasheet.pdf',
    product_url: 'https://example.com/product',
    lifecycle: 'active',
    is_preferred: false,
    notes: '',
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
});

afterEach(cleanup);

function renderVariants() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <PartDetailVariants partId="p1" />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('PartDetailVariants', () => {
  it('renders manufacturer, MPN, package, and datasheet/product links', async () => {
    vi.mocked(commands.listVariants).mockReturnValue(ok([variant()]));

    renderVariants();

    await waitFor(() => expect(screen.getByText('Yageo')).toBeTruthy());
    expect(screen.getByText('RC0603FR-0710KL')).toBeTruthy();
    expect(screen.getByText('0603')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Datasheet' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Product page' })).toBeTruthy();
  });

  it('shows a "Preferred" badge for the preferred variant, and a "Set preferred" action otherwise', async () => {
    vi.mocked(commands.listVariants).mockReturnValue(
      ok([
        variant({ id: 'v1', manufacturer: 'Yageo', is_preferred: true }),
        variant({ id: 'v2', manufacturer: 'Vishay', is_preferred: false }),
      ]),
    );

    renderVariants();

    await waitFor(() => expect(screen.getByText('Yageo')).toBeTruthy());
    expect(screen.getByText('Preferred')).toBeTruthy();
    expect(screen.getByRole('button', { name: /set preferred/i })).toBeTruthy();
  });

  it('calls set_preferred_variant when "Set preferred" is clicked', async () => {
    vi.mocked(commands.listVariants).mockReturnValue(
      ok([variant({ id: 'v2', is_preferred: false })]),
    );
    vi.mocked(commands.setPreferredVariant).mockReturnValue(ok(null));

    renderVariants();

    const button = await screen.findByRole('button', { name: /set preferred/i });
    fireEvent.click(button);

    await waitFor(() => expect(commands.setPreferredVariant).toHaveBeenCalledWith('p1', 'v2'));
  });

  it('shows an empty state when there are no variants', async () => {
    vi.mocked(commands.listVariants).mockReturnValue(ok([]));

    renderVariants();

    await waitFor(() => expect(screen.getByText(/no manufacturer variants/i)).toBeTruthy());
  });
});
