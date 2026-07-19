import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      search: vi.fn(),
      getPart: vi.fn(),
      listCategories: vi.fn(),
      listBins: vi.fn(),
    },
  };
});

import type {
  BinSummary,
  CategoryRecord,
  ImportLineId,
  ImportReviewLine,
  LineDecision,
  PartRecord,
} from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ReviewLineTable } from './ReviewLineTable';
import { placeholderPartDraft, prefillListing, prefillVariant } from './lineDecisions';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function partRecord(overrides: Partial<PartRecord> = {}): PartRecord {
  return {
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
    ...overrides,
  };
}

const CATEGORIES: CategoryRecord[] = [
  { id: 'c-cap', name: 'Capacitor', group_name: 'Passive components', built_in: true },
];
const BINS: BinSummary[] = [];

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.search).mockReturnValue(ok([]));
  vi.mocked(commands.getPart).mockReturnValue(ok(partRecord()));
  vi.mocked(commands.listCategories).mockReturnValue(ok(CATEGORIES));
  vi.mocked(commands.listBins).mockReturnValue(ok(BINS));
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

function incompleteCreateNew(line: ImportReviewLine): LineDecision {
  return {
    type: 'create_new',
    draft: placeholderPartDraft(line),
    variant: prefillVariant(line),
    listing: prefillListing(line, CONTEXT),
  };
}

function renderTable(props: {
  lines: ImportReviewLine[];
  decisions?: Map<ImportLineId, LineDecision>;
  decisionWarnings?: Map<ImportLineId, string>;
  disabled?: boolean;
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const onChangeDecision = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <ReviewLineTable
        lines={props.lines}
        decisions={props.decisions ?? new Map()}
        decisionWarnings={props.decisionWarnings ?? new Map()}
        onChangeDecision={onChangeDecision}
        context={CONTEXT}
        disabled={props.disabled}
      />
    </QueryClientProvider>,
  );
  return { onChangeDecision };
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
    // ordered, shipped, backordered, and bin all null/none for a fee line.
    expect(scoped.getAllByText('—').length).toBeGreaterThanOrEqual(3);
  });

  it('flags a create_new decision with an incomplete draft as "Draft incomplete"', () => {
    const line = partLine();
    const decisions = new Map<ImportLineId, LineDecision>([['line1', incompleteCreateNew(line)]]);
    renderTable({ lines: [line], decisions });
    expect(screen.getByRole('button', { name: 'Draft incomplete' })).toBeTruthy();
  });

  it('shows an inline warning for a line with a backend warning', () => {
    renderTable({
      lines: [partLine({ warning: 'Shipped quantity exceeds ordered — check the invoice.' })],
      decisions: new Map([['line1', { type: 'add_stock', part_id: 'part1' }]]),
    });
    expect(screen.getByText('Shipped quantity exceeds ordered — check the invoice.')).toBeTruthy();
  });

  describe('Bin column (Task 4)', () => {
    it("shows the target part's current bin for an add_stock decision", async () => {
      vi.mocked(commands.getPart).mockReturnValue(
        ok(partRecord({ id: 'part1', bin_label: 'A10' })),
      );
      const decisions = new Map<ImportLineId, LineDecision>([
        ['line1', { type: 'add_stock', part_id: 'part1' }],
      ]);
      renderTable({ lines: [partLine()], decisions });
      await waitFor(() => expect(screen.getByText('A10')).toBeTruthy());
    });

    it('shows "Unassigned" when the target part has no bin', async () => {
      vi.mocked(commands.getPart).mockReturnValue(ok(partRecord({ id: 'part1', bin_label: null })));
      const decisions = new Map<ImportLineId, LineDecision>([
        ['line1', { type: 'add_stock', part_id: 'part1' }],
      ]);
      renderTable({ lines: [partLine()], decisions });
      await waitFor(() => expect(screen.getByText('Unassigned')).toBeTruthy());
    });

    it("shows the draft's own bin for a create_new decision, without any part query", () => {
      const line = partLine();
      const decision = incompleteCreateNew(line);
      if (decision.type !== 'create_new') throw new Error('unreachable');
      decision.draft.bin_label = 'C07';
      renderTable({ lines: [line], decisions: new Map([['line1', decision]]) });
      expect(screen.getByText('C07')).toBeTruthy();
      expect(commands.getPart).not.toHaveBeenCalled();
    });

    it('shows "Unassigned" for a create_new draft with no bin yet', () => {
      const line = partLine();
      renderTable({ lines: [line], decisions: new Map([['line1', incompleteCreateNew(line)]]) });
      expect(screen.getByText('Unassigned')).toBeTruthy();
    });

    it('shows an em dash in the Bin column for a skip decision', () => {
      renderTable({
        lines: [partLine()],
        decisions: new Map<ImportLineId, LineDecision>([['line1', { type: 'skip' }]]),
      });
      const row = screen.getByText('DK123').closest('tr') as HTMLElement;
      expect(within(row).getByText('—')).toBeTruthy();
    });
  });

  describe('create-from-line dialog trigger (Task 4)', () => {
    it('clicking "Draft incomplete" opens CreateFromLineDialog prefilled from the line', async () => {
      const line = partLine({ description: 'Resistor 1k', mpn: 'MPN1' });
      renderTable({ lines: [line], decisions: new Map([['line1', incompleteCreateNew(line)]]) });

      fireEvent.click(screen.getByRole('button', { name: 'Draft incomplete' }));
      await waitFor(() => expect(screen.getByRole('dialog')).toBeTruthy());
      expect((screen.getByLabelText('Display name') as HTMLInputElement).value).toBe('Resistor 1k');
    });

    it('saving the dialog replaces the decision via onChangeDecision and the flag becomes "Edit draft"', async () => {
      const line = partLine({ description: 'Resistor 1k' });
      const { onChangeDecision } = renderTable({
        lines: [line],
        decisions: new Map([['line1', incompleteCreateNew(line)]]),
      });

      fireEvent.click(screen.getByRole('button', { name: 'Draft incomplete' }));
      await waitFor(() => expect(screen.getByText('Capacitor')).toBeTruthy());
      fireEvent.change(screen.getByLabelText('Category'), { target: { value: 'c-cap' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save draft' }));

      expect(onChangeDecision).toHaveBeenCalledWith(
        'line1',
        expect.objectContaining({ type: 'create_new' }),
      );
      const [, saved] = onChangeDecision.mock.calls[0] as [ImportLineId, LineDecision];
      if (saved.type !== 'create_new') throw new Error('unreachable');
      expect(saved.draft.category_id).toBe('c-cap');
    });

    it('Cancel closes the dialog without calling onChangeDecision', async () => {
      const line = partLine();
      const { onChangeDecision } = renderTable({
        lines: [line],
        decisions: new Map([['line1', incompleteCreateNew(line)]]),
      });

      fireEvent.click(screen.getByRole('button', { name: 'Draft incomplete' }));
      await waitFor(() => expect(screen.getByRole('dialog')).toBeTruthy());
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      expect(screen.queryByRole('dialog')).toBeNull();
      expect(onChangeDecision).not.toHaveBeenCalled();
    });
  });

  describe('disabled (Task 4 — freezes a committed/reversed import)', () => {
    it('disables the "Change…" trigger and the draft flag, and never opens the dialog', () => {
      const line = partLine();
      renderTable({
        lines: [line],
        decisions: new Map([['line1', incompleteCreateNew(line)]]),
        disabled: true,
      });

      expect(
        (screen.getByRole('button', { name: /Change decision/ }) as HTMLButtonElement).disabled,
      ).toBe(true);
      const flag = screen.getByRole('button', { name: 'Draft incomplete' }) as HTMLButtonElement;
      expect(flag.disabled).toBe(true);

      fireEvent.click(flag);
      expect(screen.queryByRole('dialog')).toBeNull();
    });
  });
});
