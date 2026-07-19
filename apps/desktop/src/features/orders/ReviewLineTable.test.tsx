import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      search: vi.fn(),
      getPart: vi.fn(),
    },
  };
});

import type { ImportLineId, ImportReviewLine, LineDecision } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ReviewLineTable } from './ReviewLineTable';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.search).mockReturnValue(ok([]));
  vi.mocked(commands.getPart).mockReturnValue(
    ok({
      id: 'part9',
      display_name: 'Some other part',
      category_id: 'cat1',
      description: '',
      bin_label: null,
      usage_behavior: 'usually_consumed',
      quantity_unit: 'each',
      low_stock_threshold: null,
      public_notes: '',
      private_notes: '',
      metadata_complete: false,
      archived: false,
      created_at: '2026-07-01 00:00:00',
      modified_at: '2026-07-01 00:00:00',
    }),
  );
});

afterEach(cleanup);

const CONTEXT = { currency: 'USD', orderDate: '2026-07-01' };

function partLine(overrides: Partial<ImportReviewLine> = {}): ImportReviewLine {
  return {
    line_id: 'line1',
    line_number: 1,
    kind: 'part',
    supplier_sku: 'DK123',
    mpn: 'MPN1',
    manufacturer: 'Acme',
    description: 'Resistor 1k',
    receive_qty_milli: 8000,
    ordered_milli: 10_000,
    backordered_milli: 2000,
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

function feeLine(overrides: Partial<ImportReviewLine> = {}): ImportReviewLine {
  return {
    line_id: 'line2',
    line_number: 2,
    kind: 'fee',
    supplier_sku: null,
    mpn: null,
    manufacturer: null,
    description: 'Shipping and handling',
    receive_qty_milli: null,
    ordered_milli: null,
    backordered_milli: null,
    unit_price_micros: 500_000,
    matches: [],
    proposed: { type: 'non_inventory' },
    warning: null,
    ...overrides,
  };
}

function renderTable(props: {
  lines: ImportReviewLine[];
  decisions?: Map<ImportLineId, LineDecision>;
  decisionWarnings?: Map<ImportLineId, string>;
  onChangeDecision?: (lineId: ImportLineId, decision: LineDecision) => void;
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <ReviewLineTable
        lines={props.lines}
        decisions={props.decisions ?? new Map()}
        decisionWarnings={props.decisionWarnings ?? new Map()}
        onChangeDecision={props.onChangeDecision ?? vi.fn()}
        context={CONTEXT}
      />
    </QueryClientProvider>,
  );
}

describe('ReviewLineTable', () => {
  it('renders a part line with shipped highlighted as the receive quantity, distinct from ordered', () => {
    const decisions = new Map<ImportLineId, LineDecision>([
      ['line1', { type: 'add_stock', part_id: 'part1' }],
    ]);
    renderTable({ lines: [partLine()], decisions });

    const row = screen.getByText('DK123').closest('tr');
    expect(row).toBeTruthy();
    const scoped = within(row as HTMLElement);
    expect(scoped.getByText('10')).toBeTruthy(); // ordered
    expect(scoped.getByText('8')).toBeTruthy(); // shipped/receive
    expect(scoped.getByText('2')).toBeTruthy(); // backordered
    expect(scoped.getByText('Add stock to Existing 1k resistor')).toBeTruthy();
    expect(scoped.getByRole('button', { name: /Change decision/ })).toBeTruthy();
  });

  it('shows "Not received" for a part line whose shipped quantity is null (nothing to receive)', () => {
    const decisions = new Map<ImportLineId, LineDecision>([['line1', { type: 'skip' }]]);
    renderTable({ lines: [partLine({ receive_qty_milli: null })], decisions });
    expect(screen.getByText('Not received')).toBeTruthy();
  });

  it('renders a non-part line greyed with its kind badge and no action editor', () => {
    renderTable({ lines: [feeLine()] });

    const row = screen.getByText('Shipping and handling').closest('tr');
    expect(row?.className).toContain('review-line-table-row--non-part');
    const scoped = within(row as HTMLElement);
    expect(scoped.getByText('Fee')).toBeTruthy();
    expect(scoped.getByText('Not inventory')).toBeTruthy();
    expect(scoped.queryByRole('button', { name: /Change decision/ })).toBeNull();
  });

  it('shows an em dash for ordered/shipped/backordered on a non-part line', () => {
    renderTable({ lines: [feeLine()] });
    const row = screen.getByText('Shipping and handling').closest('tr');
    const scoped = within(row as HTMLElement);
    // ordered, shipped, backordered all null for a fee line.
    expect(scoped.getAllByText('—').length).toBeGreaterThanOrEqual(3);
  });

  it('flags a create_new decision with an incomplete draft as "Draft incomplete"', () => {
    const decisions = new Map<ImportLineId, LineDecision>([
      [
        'line1',
        {
          type: 'create_new',
          draft: {
            display_name: 'New part',
            category_id: '',
            description: '',
            bin_label: null,
            usage_behavior: 'usually_consumed',
            quantity_unit: 'each',
            low_stock_threshold: null,
            public_notes: '',
            private_notes: '',
          },
          variant: {
            manufacturer: 'Acme',
            mpn: 'MPN1',
            description: '',
            package: null,
            datasheet_url: null,
            product_url: null,
            lifecycle: null,
            notes: '',
          },
          listing: {
            supplier: 'DigiKey',
            supplier_sku: 'DK123',
            product_url: null,
            packaging: null,
            typical_order: null,
            last_unit_price_micros: 100_000,
            currency: 'USD',
            last_purchase_date: '2026-07-01',
          },
        },
      ],
    ]);
    renderTable({ lines: [partLine()], decisions });
    expect(screen.getByText('Draft incomplete')).toBeTruthy();
  });

  it('shows an inline warning for a line with a backend warning', () => {
    renderTable({
      lines: [partLine({ warning: 'Shipped quantity exceeds ordered — check the invoice.' })],
      decisions: new Map([['line1', { type: 'add_stock', part_id: 'part1' }]]),
    });
    expect(screen.getByText('Shipped quantity exceeds ordered — check the invoice.')).toBeTruthy();
  });
});
