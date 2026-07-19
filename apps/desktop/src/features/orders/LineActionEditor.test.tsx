import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
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

import type { ImportReviewLine, LineDecision, SearchHit } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { LineActionEditor } from './LineActionEditor';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.search).mockReturnValue(ok([]));
});

afterEach(cleanup);

function line(overrides: Partial<ImportReviewLine> = {}): ImportReviewLine {
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

function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    part_id: 'part2',
    display_name: '5mm red LED',
    category_name: 'LED',
    bin_label: null,
    available: 0,
    reserved: 0,
    checked_out: 0,
    low_stock_threshold: null,
    archived: false,
    ...overrides,
  };
}

const CONTEXT = { currency: 'USD', orderDate: '2026-07-01' };

function renderEditor(props: { line?: ImportReviewLine; decision?: LineDecision }) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const onChange = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <LineActionEditor
        line={props.line ?? line()}
        decision={props.decision ?? { type: 'add_stock', part_id: 'part1' }}
        onChange={onChange}
        context={CONTEXT}
      />
    </QueryClientProvider>,
  );
  return { onChange };
}

function openPopover() {
  fireEvent.click(screen.getByRole('button', { name: /Change decision/ }));
}

describe('LineActionEditor', () => {
  it('picking a listed match calls onChange with add_stock for that part', async () => {
    const { onChange } = renderEditor({});
    openPopover();
    fireEvent.click(await screen.findByText('Existing 1k resistor'));
    expect(onChange).toHaveBeenCalledWith({ type: 'add_stock', part_id: 'part1' });
  });

  it('"Match other part…" searches and calls onChange with add_stock for the picked part', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));
    const { onChange } = renderEditor({});
    openPopover();
    fireEvent.click(screen.getByRole('button', { name: 'Match other part…' }));
    fireEvent.change(screen.getByPlaceholderText('Search parts…'), {
      target: { value: 'LED' },
    });
    fireEvent.click(await screen.findByText('5mm red LED'));
    expect(onChange).toHaveBeenCalledWith({ type: 'add_stock', part_id: 'part2' });
  });

  it('"Add as variant to…" searches and calls onChange with add_as_variant + prefilled drafts', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));
    const { onChange } = renderEditor({});
    openPopover();
    fireEvent.click(screen.getByRole('button', { name: 'Add as variant to…' }));
    fireEvent.change(screen.getByPlaceholderText('Search parts…'), {
      target: { value: 'LED' },
    });
    fireEvent.click(await screen.findByText('5mm red LED'));

    await waitFor(() => expect(onChange).toHaveBeenCalled());
    const call = onChange.mock.calls[0]?.[0] as LineDecision;
    expect(call.type).toBe('add_as_variant');
    if (call.type === 'add_as_variant') {
      expect(call.part_id).toBe('part2');
      expect(call.variant.manufacturer).toBe('Acme');
      expect(call.variant.mpn).toBe('MPN1');
      expect(call.listing.supplier).toBe('DigiKey');
      expect(call.listing.supplier_sku).toBe('DK123');
    }
  });

  it('"Create new part" calls onChange with an incomplete-draft create_new decision', () => {
    const { onChange } = renderEditor({});
    openPopover();
    fireEvent.click(screen.getByRole('button', { name: 'Create new part' }));

    expect(onChange).toHaveBeenCalledTimes(1);
    const call = onChange.mock.calls[0]?.[0] as LineDecision;
    expect(call.type).toBe('create_new');
    if (call.type === 'create_new') {
      expect(call.draft.category_id).toBe('');
      expect(call.draft.display_name).toBe('Resistor 1k');
    }
  });

  it('"Skip" calls onChange with a skip decision', () => {
    const { onChange } = renderEditor({});
    openPopover();
    fireEvent.click(screen.getByRole('button', { name: 'Skip' }));
    expect(onChange).toHaveBeenCalledWith({ type: 'skip' });
  });
});
