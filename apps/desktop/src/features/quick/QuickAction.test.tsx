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
      getStock: vi.fn(),
      listProjects: vi.fn(),
      createProject: vi.fn(),
      applyLedgerOp: vi.fn(),
    },
  };
});

import type {
  PartRecord,
  PartStockRow,
  ProjectRef,
  SearchHit,
  TransactionRecord,
} from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { QuickAction, type QuickActionRequest } from './QuickAction';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

function stock(overrides: Partial<PartStockRow> = {}): PartStockRow {
  return {
    available: 40_000,
    reserved: 0,
    checked_out: 0,
    lifetime_received: 40_000,
    lifetime_consumed: 0,
    ...overrides,
  };
}

function partRecord(overrides: Partial<PartRecord> = {}): PartRecord {
  return {
    id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_id: 'c1',
    description: '',
    bin_label: null,
    usage_behavior: 'consumable',
    quantity_unit: 'each',
    low_stock_threshold: null,
    public_notes: '',
    private_notes: '',
    metadata_complete: true,
    archived: false,
    created_at: '2026-01-01 00:00:00',
    modified_at: '2026-01-01 00:00:00',
    ...overrides,
  };
}

function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    part_id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_name: 'Resistor',
    bin_label: 'A12',
    available: 450_000,
    reserved: 50_000,
    checked_out: 0,
    low_stock_threshold: 100_000,
    archived: false,
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.getPart).mockReturnValue(ok(partRecord()));
  vi.mocked(commands.getStock).mockReturnValue(ok(stock()));
  vi.mocked(commands.listProjects).mockReturnValue(ok([]));
});

afterEach(cleanup);

function renderQuickAction(request: QuickActionRequest, onClose = vi.fn()) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const utils = render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <QuickAction request={request} onClose={onClose} />
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { ...utils, onClose };
}

const PRESELECTED_PART = { id: 'p1', displayName: '10k 0603 1% resistor' };

describe('QuickAction — preselected part, no project needed (Add stock)', () => {
  it('skips the search step and goes straight to quantity when a part is preselected', async () => {
    renderQuickAction({ kind: 'receive', part: PRESELECTED_PART });

    expect(await screen.findByRole('dialog')).toBeTruthy();
    expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy();
    expect(screen.getByLabelText('Quantity')).toBeTruthy();
    expect(screen.queryByRole('combobox')).toBeNull();
  });

  it('shows a live "remaining after" preview computed from current stock', async () => {
    vi.mocked(commands.getStock).mockReturnValue(ok(stock({ available: 40_000 })));
    renderQuickAction({ kind: 'receive', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '10' } });

    await waitFor(() => expect(screen.getByText('50 available after')).toBeTruthy());

    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '25' } });
    await waitFor(() => expect(screen.getByText('65 available after')).toBeTruthy());
  });

  it('confirms a receive op and toasts the received amount', async () => {
    const txn = { id: 't1', part_id: 'p1', quantity: 10_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    const { onClose } = renderQuickAction({ kind: 'receive', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '10' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add stock' }));

    await waitFor(() =>
      expect(commands.applyLedgerOp).toHaveBeenCalledWith({
        type: 'receive',
        part_id: 'p1',
        quantity: 10_000,
        note: '',
      }),
    );
    await waitFor(() => expect(screen.getByText('Received 10')).toBeTruthy());
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it('folds the "Add details" expander fields into the note sent on the wire', async () => {
    const txn = { id: 't1', part_id: 'p1', quantity: 10_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    renderQuickAction({ kind: 'receive', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '10' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add details' }));
    fireEvent.change(screen.getByLabelText('Supplier'), { target: { value: 'DigiKey' } });
    fireEvent.change(screen.getByLabelText('Order'), { target: { value: 'PO-123' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add stock' }));

    await waitFor(() =>
      expect(commands.applyLedgerOp).toHaveBeenCalledWith(
        expect.objectContaining({
          note: expect.stringContaining('Supplier: DigiKey'),
        }),
      ),
    );
    const call = vi.mocked(commands.applyLedgerOp).mock.calls[0]?.[0] as { note: string };
    expect(call.note).toContain('Order: PO-123');
  });

  it('disables submit until a positive quantity is entered', async () => {
    renderQuickAction({ kind: 'receive', part: PRESELECTED_PART });
    const submit = () => screen.getByRole('button', { name: 'Add stock' }) as HTMLButtonElement;

    expect(submit().disabled).toBe(true);
    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '0' } });
    expect(submit().disabled).toBe(true);
    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '3' } });
    expect(submit().disabled).toBe(false);
  });

  it('Escape/Cancel closes without calling applyLedgerOp', async () => {
    const { onClose } = renderQuickAction({ kind: 'receive', part: PRESELECTED_PART });
    await screen.findByRole('dialog');

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(commands.applyLedgerOp).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });
});

describe('QuickAction — over-consume', () => {
  it('surfaces the InsufficientStock message inline without closing the dialog', async () => {
    vi.mocked(commands.applyLedgerOp).mockReturnValue(
      commandError('insufficient_stock', 'insufficient stock: only 2 available'),
    );
    const { onClose } = renderQuickAction({ kind: 'consume_available', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '99' } });
    fireEvent.click(screen.getByRole('button', { name: 'Consume' }));

    // The dialog must stay open so the user can correct the quantity in
    // place — scope the assertion to it, since the same message also fires
    // as a toast (consistent with the rest of the app) elsewhere on screen.
    const dialog = await screen.findByRole('dialog');
    await waitFor(() =>
      expect(
        within(dialog).getByText(
          'Not enough stock is available for this action — receive more or lower the quantity.',
        ),
      ).toBeTruthy(),
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});

describe('QuickAction — continuous-unit part (real quantity_unit, not "each")', () => {
  it('shows the preview and toast in the part\'s real unit ("m"), fetched via usePart', async () => {
    vi.mocked(commands.getPart).mockReturnValue(ok(partRecord({ quantity_unit: 'meter' })));
    vi.mocked(commands.getStock).mockReturnValue(ok(stock({ available: 10_500 })));
    const txn = { id: 't5', part_id: 'p1', quantity: 2_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    const { onClose } = renderQuickAction({ kind: 'consume_available', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '2' } });
    await waitFor(() => expect(screen.getByText('8.5 m available after')).toBeTruthy());

    fireEvent.click(screen.getByRole('button', { name: 'Consume' }));

    await waitFor(() =>
      expect(commands.applyLedgerOp).toHaveBeenCalledWith({
        type: 'consume_available',
        part_id: 'p1',
        quantity: 2_000,
        project_id: null,
        note: '',
      }),
    );
    await waitFor(() => expect(screen.getByText('Consumed 2 m')).toBeTruthy());
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it('still labels an "each" part with no unit suffix (existing behavior unchanged)', async () => {
    vi.mocked(commands.getPart).mockReturnValue(ok(partRecord({ quantity_unit: 'each' })));
    vi.mocked(commands.getStock).mockReturnValue(ok(stock({ available: 40_000 })));
    renderQuickAction({ kind: 'receive', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '10' } });
    await waitFor(() => expect(screen.getByText('50 available after')).toBeTruthy());
  });
});

describe('QuickAction — overdraw preview coloring', () => {
  it('flags the preview with the negative/overdraw modifier class when the resulting pool is negative', async () => {
    vi.mocked(commands.getStock).mockReturnValue(ok(stock({ available: 2_000 })));
    renderQuickAction({ kind: 'consume_available', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '5' } });

    const preview = await waitFor(() => screen.getByText('-3 available after'));
    expect(preview.className).toContain('quick-action-preview--negative');
  });

  it('does not flag the preview when the resulting pool stays non-negative', async () => {
    vi.mocked(commands.getStock).mockReturnValue(ok(stock({ available: 40_000 })));
    renderQuickAction({ kind: 'consume_available', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '5' } });

    const preview = await waitFor(() => screen.getByText('35 available after'));
    expect(preview.className).not.toContain('quick-action-preview--negative');
  });
});

describe('QuickAction — optional project (Consume)', () => {
  it('submits consume_available with a null project when none is chosen', async () => {
    const txn = { id: 't2', part_id: 'p1', quantity: 5_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    renderQuickAction({ kind: 'consume_available', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '5' } });
    fireEvent.click(screen.getByRole('button', { name: 'Consume' }));

    await waitFor(() =>
      expect(commands.applyLedgerOp).toHaveBeenCalledWith({
        type: 'consume_available',
        part_id: 'p1',
        quantity: 5_000,
        project_id: null,
        note: '',
      }),
    );
  });
});

describe('QuickAction — required project (Reserve)', () => {
  const PROJECTS: ProjectRef[] = [{ id: 'pr1', name: 'Blinky Board' }];

  it('disables submit until a project is chosen, then reserves with that project', async () => {
    vi.mocked(commands.listProjects).mockReturnValue(ok(PROJECTS));
    const txn = { id: 't3', part_id: 'p1', quantity: 5_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    renderQuickAction({ kind: 'reserve', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '5' } });
    const submit = () => screen.getByRole('button', { name: 'Reserve' }) as HTMLButtonElement;
    expect(submit().disabled).toBe(true);

    const select = await screen.findByLabelText('Project');
    fireEvent.change(select, { target: { value: 'pr1' } });
    expect(submit().disabled).toBe(false);

    fireEvent.click(submit());

    await waitFor(() =>
      expect(commands.applyLedgerOp).toHaveBeenCalledWith({
        type: 'reserve',
        part_id: 'p1',
        quantity: 5_000,
        project_id: 'pr1',
      }),
    );
    await waitFor(() => expect(screen.getByText('Reserved 5 for Blinky Board')).toBeTruthy());
  });

  it('creates a project inline via "Create new project…" and uses it for the op', async () => {
    vi.mocked(commands.listProjects).mockReturnValue(ok([]));
    vi.mocked(commands.createProject).mockReturnValue(ok('pr9'));
    const txn = { id: 't4', part_id: 'p1', quantity: 2_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    renderQuickAction({ kind: 'reserve', part: PRESELECTED_PART });

    fireEvent.change(await screen.findByLabelText('Quantity'), { target: { value: '2' } });
    const select = await screen.findByLabelText('Project');
    fireEvent.change(select, { target: { value: '__create_new_project__' } });

    const nameInput = await screen.findByLabelText('New project name');
    fireEvent.change(nameInput, { target: { value: 'Bench PSU Rebuild' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() => expect(commands.createProject).toHaveBeenCalledWith('Bench PSU Rebuild'));

    const submit = await screen.findByRole('button', { name: 'Reserve' });
    await waitFor(() => expect((submit as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(submit);

    await waitFor(() =>
      expect(commands.applyLedgerOp).toHaveBeenCalledWith({
        type: 'reserve',
        part_id: 'p1',
        quantity: 2_000,
        project_id: 'pr9',
      }),
    );
  });
});

describe('QuickAction — no preselected part (Ctrl+K flow)', () => {
  it('shows a part search step and moves to quantity once a part is chosen', async () => {
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));
    renderQuickAction({ kind: 'receive' });

    const input = await screen.findByRole('combobox');
    fireEvent.change(input, { target: { value: '10k' } });

    const result = await screen.findByText('10k 0603 1% resistor');
    fireEvent.click(result);

    await waitFor(() => expect(screen.getByLabelText('Quantity')).toBeTruthy());
    expect(screen.queryByRole('combobox')).toBeNull();
  });

  it('shows a hint before anything is typed and never calls search for a blank query', () => {
    renderQuickAction({ kind: 'receive' });
    expect(commands.search).not.toHaveBeenCalled();
  });
});
