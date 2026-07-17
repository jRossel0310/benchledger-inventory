import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      applyLedgerOp: vi.fn(),
      listBins: vi.fn(),
      getPart: vi.fn(),
      updatePart: vi.fn(),
    },
  };
});

const openQuickAction = vi.fn();
vi.mock('../quick/QuickActionContext', () => ({
  useQuickAction: () => ({ open: openQuickAction }),
}));

import type { BinSummary, PartRecord, SearchHit, TransactionRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { RowActions } from './RowActions';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

const ROW: SearchHit = {
  part_id: 'p1',
  display_name: '10k 0603 1% resistor',
  category_name: 'Resistor',
  bin_label: 'A10',
  available: 450_000,
  reserved: 50_000,
  checked_out: 0,
  low_stock_threshold: 100_000,
  archived: false,
};

function partRecord(overrides: Partial<PartRecord> = {}): PartRecord {
  return {
    id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_id: 'cat-resistor',
    description: '',
    bin_label: 'A10',
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each',
    low_stock_threshold: 100_000,
    public_notes: '',
    private_notes: '',
    metadata_complete: true,
    archived: false,
    created_at: '2026-01-01 00:00:00',
    modified_at: '2026-01-02 00:00:00',
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  // Default: no bins occupied, so tests that don't care about the
  // occupied-bin warning never accidentally trip it.
  vi.mocked(commands.listBins).mockReturnValue(ok<BinSummary[]>([]));
});

afterEach(cleanup);

function renderRowActions(row: SearchHit = ROW) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <RowActions row={row} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

function openMenu() {
  // Radix's `DropdownMenuTrigger` opens on `pointerdown`, not `click` (see
  // `@radix-ui/react-dropdown-menu`'s trigger source) — `fireEvent.click`
  // alone never dispatches a `pointerdown`, so the menu would stay closed.
  const trigger = screen.getByRole('button', { name: /actions for 10k 0603 1% resistor/i });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.click(trigger);
}

describe('RowActions', () => {
  it('lists Add stock, Consume, Reserve, and Check out, all enabled', async () => {
    renderRowActions();
    openMenu();

    expect(await screen.findByText('Add stock')).toBeTruthy();
    expect(screen.getByText('Consume')).toBeTruthy();

    const reserve = screen.getByText('Reserve');
    const checkOut = screen.getByText('Check out');
    expect(reserve.closest('[data-disabled]')).toBeFalsy();
    expect(checkOut.closest('[data-disabled]')).toBeFalsy();
  });

  it('Reserve opens the QuickAction dialog with this row preselected', async () => {
    renderRowActions();
    openMenu();

    fireEvent.click(await screen.findByText('Reserve'));

    expect(openQuickAction).toHaveBeenCalledWith({
      kind: 'reserve',
      part: { id: 'p1', displayName: '10k 0603 1% resistor' },
    });
  });

  it('Check out opens the QuickAction dialog with this row preselected', async () => {
    renderRowActions();
    openMenu();

    fireEvent.click(await screen.findByText('Check out'));

    expect(openQuickAction).toHaveBeenCalledWith({
      kind: 'check_out',
      part: { id: 'p1', displayName: '10k 0603 1% resistor' },
    });
  });

  it('Add stock calls applyLedgerOp with a receive op and toasts the received amount', async () => {
    const txn = { id: 't1', part_id: 'p1', quantity: 10_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    renderRowActions();
    openMenu();

    fireEvent.click(await screen.findByText('Add stock'));
    expect(await screen.findByRole('dialog')).toBeTruthy();

    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '10' } });
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
  });

  it('Consume calls applyLedgerOp with a consume_available op (no project)', async () => {
    const txn = { id: 't2', part_id: 'p1', quantity: 5_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    renderRowActions();
    openMenu();

    fireEvent.click(await screen.findByText('Consume'));
    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '5' } });
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
    await waitFor(() => expect(screen.getByText('Consumed 5')).toBeTruthy());
  });

  it('passes the note field through to the op', async () => {
    const txn = { id: 't3', part_id: 'p1', quantity: 1_000 } as unknown as TransactionRecord;
    vi.mocked(commands.applyLedgerOp).mockReturnValue(ok(txn));
    renderRowActions();
    openMenu();

    fireEvent.click(await screen.findByText('Add stock'));
    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '1' } });
    fireEvent.change(screen.getByLabelText('Note'), { target: { value: 'from digikey order' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add stock' }));

    await waitFor(() =>
      expect(commands.applyLedgerOp).toHaveBeenCalledWith(
        expect.objectContaining({ note: 'from digikey order' }),
      ),
    );
  });

  it('disables submit until a positive quantity is entered', async () => {
    renderRowActions();
    openMenu();
    fireEvent.click(await screen.findByText('Add stock'));

    const submit = () => screen.getByRole('button', { name: 'Add stock' }) as HTMLButtonElement;
    expect(submit().disabled).toBe(true);
    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '0' } });
    expect(submit().disabled).toBe(true);
    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '3' } });
    expect(submit().disabled).toBe(false);
  });

  it('cancel closes the dialog without calling applyLedgerOp', async () => {
    renderRowActions();
    openMenu();
    fireEvent.click(await screen.findByText('Add stock'));
    expect(await screen.findByRole('dialog')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(commands.applyLedgerOp).not.toHaveBeenCalled();
  });

  it('toasts a hinted error when the mutation fails', async () => {
    vi.mocked(commands.applyLedgerOp).mockReturnValue(
      commandError('insufficient_stock', 'insufficient stock: only 2 available'),
    );
    renderRowActions();
    openMenu();

    fireEvent.click(await screen.findByText('Consume'));
    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '99' } });
    fireEvent.click(screen.getByRole('button', { name: 'Consume' }));

    await waitFor(() => expect(screen.getByText('Could not consume')).toBeTruthy());
    expect(
      screen.getByText(
        'Not enough stock is available for this action — receive more or lower the quantity.',
      ),
    ).toBeTruthy();
  });

  describe('Assign bin', () => {
    /** Opens the dialog and waits for `useBins()` to settle (the submit
     * button is disabled until then — see `RowActions.tsx`'s
     * `submitDisabled` — so the occupied-bin check is never raced). */
    async function openAssignBinDialog() {
      openMenu();
      fireEvent.click(screen.getByText('Assign bin'));
      await waitFor(() =>
        expect((screen.getByRole('button', { name: 'Assign' }) as HTMLButtonElement).disabled).toBe(
          false,
        ),
      );
    }

    it('opens a dialog prefilled with the row’s current bin', async () => {
      renderRowActions();
      await openAssignBinDialog();

      expect(screen.getByRole('dialog')).toBeTruthy();
      expect((screen.getByLabelText('Bin') as HTMLInputElement).value).toBe('A10');
    });

    it('assigning an unoccupied bin loads the part, saves, and toasts the new bin', async () => {
      vi.mocked(commands.getPart).mockReturnValue(ok(partRecord()));
      vi.mocked(commands.updatePart).mockReturnValue(ok(null));
      renderRowActions();
      await openAssignBinDialog();

      fireEvent.change(screen.getByLabelText('Bin'), { target: { value: 'B2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Assign' }));

      await waitFor(() => expect(commands.getPart).toHaveBeenCalledWith('p1'));
      await waitFor(() =>
        expect(commands.updatePart).toHaveBeenCalledWith(
          expect.objectContaining({ id: 'p1', bin_label: 'B2' }),
        ),
      );
      await waitFor(() => expect(screen.getByText('Moved to bin B2')).toBeTruthy());
    });

    it('normalizes a blank bin to null (clears the bin), not an empty string', async () => {
      vi.mocked(commands.getPart).mockReturnValue(ok(partRecord()));
      vi.mocked(commands.updatePart).mockReturnValue(ok(null));
      renderRowActions();
      await openAssignBinDialog();

      fireEvent.change(screen.getByLabelText('Bin'), { target: { value: '   ' } });
      fireEvent.click(screen.getByRole('button', { name: 'Assign' }));

      await waitFor(() =>
        expect(commands.updatePart).toHaveBeenCalledWith(
          expect.objectContaining({ bin_label: null }),
        ),
      );
      await waitFor(() => expect(screen.getByText('Bin cleared')).toBeTruthy());
    });

    it('submitting the same bin unchanged closes without saving', async () => {
      renderRowActions();
      await openAssignBinDialog();

      fireEvent.click(screen.getByRole('button', { name: 'Assign' }));

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      expect(commands.getPart).not.toHaveBeenCalled();
      expect(commands.updatePart).not.toHaveBeenCalled();
    });

    it('assigning into an occupied bin warns but does not block — proceeds on "Assign anyway"', async () => {
      vi.mocked(commands.listBins).mockReturnValue(
        ok<BinSummary[]>([{ bin_label: 'B2', part_count: 3 }]),
      );
      vi.mocked(commands.getPart).mockReturnValue(ok(partRecord()));
      vi.mocked(commands.updatePart).mockReturnValue(ok(null));
      renderRowActions();
      await openAssignBinDialog();

      fireEvent.change(screen.getByLabelText('Bin'), { target: { value: 'B2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Assign' }));

      expect(await screen.findByText('Bin B2 already holds 3 parts — assign anyway?')).toBeTruthy();
      // Non-blocking: nothing saved yet, but the action is still available.
      expect(commands.updatePart).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole('button', { name: 'Assign anyway' }));

      await waitFor(() =>
        expect(commands.updatePart).toHaveBeenCalledWith(
          expect.objectContaining({ bin_label: 'B2' }),
        ),
      );
    });

    it('canceling the occupied-bin warning does not save and returns to the form', async () => {
      vi.mocked(commands.listBins).mockReturnValue(
        ok<BinSummary[]>([{ bin_label: 'B2', part_count: 3 }]),
      );
      renderRowActions();
      await openAssignBinDialog();

      fireEvent.change(screen.getByLabelText('Bin'), { target: { value: 'B2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Assign' }));
      expect(await screen.findByText(/already holds 3 parts/)).toBeTruthy();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      expect(commands.updatePart).not.toHaveBeenCalled();
      expect(await screen.findByLabelText('Bin')).toBeTruthy();
    });

    it('reassigning to the same bin (case-insensitive) never warns, even if that bin is "occupied"', async () => {
      vi.mocked(commands.listBins).mockReturnValue(
        ok<BinSummary[]>([{ bin_label: 'A10', part_count: 5 }]),
      );
      renderRowActions();
      await openAssignBinDialog();

      fireEvent.change(screen.getByLabelText('Bin'), { target: { value: 'a10' } });
      fireEvent.click(screen.getByRole('button', { name: 'Assign' }));

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      expect(commands.updatePart).not.toHaveBeenCalled();
    });

    it('cancel closes the dialog without calling getPart or updatePart', async () => {
      renderRowActions();
      await openAssignBinDialog();
      expect(await screen.findByRole('dialog')).toBeTruthy();

      fireEvent.change(screen.getByLabelText('Bin'), { target: { value: 'B2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      expect(commands.getPart).not.toHaveBeenCalled();
      expect(commands.updatePart).not.toHaveBeenCalled();
    });

    it('toasts a hinted error when the save fails', async () => {
      vi.mocked(commands.getPart).mockReturnValue(ok(partRecord()));
      vi.mocked(commands.updatePart).mockReturnValue(
        commandError('part_not_found', 'part not found'),
      );
      renderRowActions();
      await openAssignBinDialog();

      fireEvent.change(screen.getByLabelText('Bin'), { target: { value: 'B2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Assign' }));

      await waitFor(() => expect(screen.getByText('Could not assign bin')).toBeTruthy());
      expect(
        screen.getByText('This part no longer exists — it may have been deleted elsewhere.'),
      ).toBeTruthy();
    });
  });
});
