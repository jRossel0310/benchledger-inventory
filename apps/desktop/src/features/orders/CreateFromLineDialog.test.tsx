import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listCategories: vi.fn(),
      listBins: vi.fn(),
    },
  };
});

import type {
  BinSummary,
  CategoryRecord,
  ImportReviewLine,
  LineDecision,
} from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { CreateFromLineDialog } from './CreateFromLineDialog';
import { placeholderPartDraft, prefillListing, prefillVariant } from './lineDecisions';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function input(label: string): HTMLInputElement {
  return screen.getByLabelText(label) as HTMLInputElement;
}

function select(label: string): HTMLSelectElement {
  return screen.getByLabelText(label) as HTMLSelectElement;
}

function button(name: string): HTMLButtonElement {
  return screen.getByRole('button', { name }) as HTMLButtonElement;
}

/** Categories load asynchronously (`useCategories`); the `<select>`'s
 * options don't include `c-cap` until that resolves, so setting `.value` to
 * it beforehand silently no-ops (jsdom leaves an unmatched `<select>` value
 * at `''`). Every test that picks a category waits for this first. */
async function waitForCategoriesLoaded() {
  await waitFor(() => expect(screen.getByText('Capacitor')).toBeTruthy());
}

const CATEGORIES: CategoryRecord[] = [
  { id: 'c-res', name: 'Resistor', group_name: 'Passive components', built_in: true },
  { id: 'c-cap', name: 'Capacitor', group_name: 'Passive components', built_in: true },
];

const BINS: BinSummary[] = [
  { bin_label: 'A12', part_count: 3 },
  { bin_label: null, part_count: 1 },
];

const CONTEXT = { currency: 'USD', orderDate: '2026-07-01' };

function line(overrides: Partial<ImportReviewLine> = {}): ImportReviewLine {
  return {
    line_id: 'line1',
    line_number: 2,
    kind: 'part',
    supplier_sku: 'DK456',
    mpn: 'MPN2',
    manufacturer: 'Acme',
    description: 'Capacitor 10uF',
    receive_qty_milli: 5000,
    ordered_milli: 5000,
    backordered_milli: 0,
    unit_price_micros: 250_000,
    matches: [],
    proposed: { type: 'create_new' },
    warning: null,
    ...overrides,
  };
}

function incompleteDecision(l: ImportReviewLine): Extract<LineDecision, { type: 'create_new' }> {
  return {
    type: 'create_new',
    draft: placeholderPartDraft(l),
    variant: prefillVariant(l),
    listing: prefillListing(l, CONTEXT),
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.listCategories).mockReturnValue(ok(CATEGORIES));
  vi.mocked(commands.listBins).mockReturnValue(ok(BINS));
});

afterEach(cleanup);

function renderDialog(props: {
  line: ImportReviewLine;
  decision: Extract<LineDecision, { type: 'create_new' }>;
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const onSave = vi.fn();
  const onCancel = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <CreateFromLineDialog
        line={props.line}
        decision={props.decision}
        onSave={onSave}
        onCancel={onCancel}
      />
    </QueryClientProvider>,
  );
  return { onSave, onCancel };
}

describe('CreateFromLineDialog', () => {
  it('prefills every field exactly from the line and the incoming decision', async () => {
    const l = line();
    renderDialog({ line: l, decision: incompleteDecision(l) });

    await waitFor(() => expect(screen.getByText('Create new part')).toBeTruthy());
    expect(input('Display name').value).toBe('Capacitor 10uF');
    expect(input('Description').value).toBe('Capacitor 10uF');
    expect(select('Category').value).toBe(''); // placeholder draft: unset
    expect(select('Quantity unit').value).toBe('each');
    expect(select('Usage behavior').value).toBe('usually_consumed');
    expect(input('Bin').value).toBe('');
    expect(input('Manufacturer').value).toBe('Acme');
    expect(input('MPN').value).toBe('MPN2');
    expect(input('Supplier SKU').value).toBe('DK456');
    expect(input('Unit price').value).toBe('0.25');
  });

  it('re-opening on a previously-completed draft shows what was actually entered, not the raw line', async () => {
    const l = line();
    const completed: Extract<LineDecision, { type: 'create_new' }> = {
      type: 'create_new',
      draft: {
        display_name: 'Edited name',
        category_id: 'c-cap',
        description: 'Edited description',
        bin_label: 'B03',
        usage_behavior: 'ask',
        quantity_unit: 'each',
        low_stock_threshold: 10_000,
        public_notes: 'note',
        private_notes: '',
      },
      variant: prefillVariant(l),
      listing: prefillListing(l, CONTEXT),
    };
    renderDialog({ line: l, decision: completed });

    await waitForCategoriesLoaded();
    expect(input('Display name').value).toBe('Edited name');
    expect(select('Category').value).toBe('c-cap');
    expect(input('Bin').value).toBe('B03');
    expect(select('Usage behavior').value).toBe('ask');
    expect(input('Low-stock threshold').value).toBe('10');
  });

  it('Save is disabled until a category is chosen (the only required field beyond display name)', async () => {
    const l = line();
    renderDialog({ line: l, decision: incompleteDecision(l) });

    await waitForCategoriesLoaded();
    expect(button('Save draft').disabled).toBe(true);

    fireEvent.change(select('Category'), { target: { value: 'c-cap' } });
    expect(button('Save draft').disabled).toBe(false);
  });

  it('Save calls onSave with a complete create_new decision built from the edited fields', async () => {
    const l = line();
    const { onSave } = renderDialog({ line: l, decision: incompleteDecision(l) });

    await waitForCategoriesLoaded();
    fireEvent.change(input('Display name'), { target: { value: 'My cap' } });
    fireEvent.change(select('Category'), { target: { value: 'c-cap' } });
    fireEvent.change(input('Bin'), { target: { value: 'C07' } });
    fireEvent.change(input('Low-stock threshold'), { target: { value: '5' } });
    fireEvent.click(button('Save draft'));

    expect(onSave).toHaveBeenCalledTimes(1);
    const decision = onSave.mock.calls[0]![0] as LineDecision;
    expect(decision.type).toBe('create_new');
    if (decision.type !== 'create_new') throw new Error('unreachable');
    expect(decision.draft.display_name).toBe('My cap');
    expect(decision.draft.category_id).toBe('c-cap');
    expect(decision.draft.bin_label).toBe('C07');
    expect(decision.draft.low_stock_threshold).toBe(5000);
    expect(decision.variant.manufacturer).toBe('Acme');
    expect(decision.variant.mpn).toBe('MPN2');
    expect(decision.listing.supplier_sku).toBe('DK456');
  });

  it('blank bin saves as null (unassigned), not an empty string', async () => {
    const l = line();
    const { onSave } = renderDialog({ line: l, decision: incompleteDecision(l) });
    await waitForCategoriesLoaded();
    fireEvent.change(select('Category'), { target: { value: 'c-cap' } });
    fireEvent.click(button('Save draft'));

    const decision = onSave.mock.calls[0]![0] as LineDecision;
    if (decision.type !== 'create_new') throw new Error('unreachable');
    expect(decision.draft.bin_label).toBeNull();
  });

  it('warns (without blocking) when the entered bin already holds other parts', async () => {
    const l = line();
    renderDialog({ line: l, decision: incompleteDecision(l) });
    await waitForCategoriesLoaded();

    fireEvent.change(select('Category'), { target: { value: 'c-cap' } });
    fireEvent.change(input('Bin'), { target: { value: 'a12' } });

    await waitFor(() => expect(screen.getByText(/already holds 3 parts/)).toBeTruthy());
    // Warn-not-block: Save stays enabled.
    expect(button('Save draft').disabled).toBe(false);
  });

  it('Cancel calls onCancel without calling onSave', async () => {
    const l = line();
    const { onSave, onCancel } = renderDialog({ line: l, decision: incompleteDecision(l) });
    await waitFor(() => expect(screen.getByText('Create new part')).toBeTruthy());

    fireEvent.change(input('Display name'), { target: { value: 'Should be discarded' } });
    fireEvent.click(button('Cancel'));

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSave).not.toHaveBeenCalled();
  });
});
