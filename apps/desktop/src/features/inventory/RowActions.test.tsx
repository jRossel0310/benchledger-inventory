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
    },
  };
});

import type { SearchHit, TransactionRecord } from '../../bindings.gen';
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

beforeEach(() => {
  vi.resetAllMocks();
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
  it('lists Add stock, Consume, and a disabled Reserve/Check out with a Ctrl+K hint', async () => {
    renderRowActions();
    openMenu();

    expect(await screen.findByText('Add stock')).toBeTruthy();
    expect(screen.getByText('Consume')).toBeTruthy();

    const reserve = screen.getByText(/Reserve/);
    const checkOut = screen.getByText(/Check out/);
    expect(reserve.closest('[data-disabled]')).toBeTruthy();
    expect(checkOut.closest('[data-disabled]')).toBeTruthy();
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
});
