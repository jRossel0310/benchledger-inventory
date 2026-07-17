import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listTransactions: vi.fn(),
      listProjects: vi.fn(),
      reverseTransaction: vi.fn(),
      reverseGroup: vi.fn(),
    },
  };
});

import type { TransactionRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { PartDetailTransactions } from './PartDetailTransactions';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

function txn(overrides: Partial<TransactionRecord> = {}): TransactionRecord {
  return {
    id: 'txn-1',
    part_id: 'p1',
    group_id: null,
    txn_type: 'receive',
    quantity: 5000,
    from_state: null,
    to_state: 'available',
    project_id: null,
    to_project_id: null,
    note: '',
    reversed_txn_id: null,
    created_at: '2026-07-15 10:00:00',
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.listProjects).mockReturnValue(ok([]));
});

afterEach(cleanup);

function renderTransactions() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <PartDetailTransactions partId="p1" unit="each" />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('PartDetailTransactions', () => {
  it('renders each transaction: type, quantity, from→to state, and time', async () => {
    vi.mocked(commands.listTransactions).mockReturnValue(
      ok([
        txn({
          id: 't1',
          txn_type: 'receive',
          quantity: 25_000,
          from_state: null,
          to_state: 'available',
        }),
      ]),
    );

    renderTransactions();

    await waitFor(() => expect(screen.getByText('Received')).toBeTruthy());
    expect(screen.getByText('25')).toBeTruthy();
    // `from_state` is null on a receive, so only the destination state shows.
    expect(screen.getByText('→ available')).toBeTruthy();
  });

  it('shows a working single reverse action on an ungrouped, non-reversal, not-yet-reversed row', async () => {
    vi.mocked(commands.listTransactions).mockReturnValue(
      ok([txn({ id: 't1', group_id: null, txn_type: 'receive' })]),
    );
    vi.mocked(commands.reverseTransaction).mockReturnValue(
      ok(txn({ id: 't2', txn_type: 'reverse', reversed_txn_id: 't1' })),
    );

    renderTransactions();

    const reverseButton = await screen.findByRole('button', { name: 'Reverse' });
    fireEvent.click(reverseButton);

    await waitFor(() =>
      expect(commands.reverseTransaction).toHaveBeenCalledWith('t1', expect.any(String)),
    );
  });

  it('offers "Reverse group" instead of a single reverse for a transaction that belongs to a group', async () => {
    vi.mocked(commands.listTransactions).mockReturnValue(
      ok([txn({ id: 't1', group_id: 'g1', txn_type: 'check_out' })]),
    );
    vi.mocked(commands.reverseGroup).mockReturnValue(
      ok({
        id: 'g2',
        kind: 'reverse:manual',
        note: '',
        reversed_group_id: 'g1',
        created_at: '',
        transactions: [],
      }),
    );

    renderTransactions();

    await waitFor(() => expect(screen.getByText(/part of a group/i)).toBeTruthy());
    expect(screen.queryByRole('button', { name: 'Reverse' })).toBeNull();
    const reverseGroupButton = screen.getByRole('button', { name: /reverse group/i });

    fireEvent.click(reverseGroupButton);

    await waitFor(() =>
      expect(commands.reverseGroup).toHaveBeenCalledWith('g1', expect.any(String)),
    );
  });

  it('shows no reverse action for a transaction that is itself a reversal', async () => {
    vi.mocked(commands.listTransactions).mockReturnValue(
      ok([txn({ id: 't2', txn_type: 'reverse', reversed_txn_id: 't1' })]),
    );

    renderTransactions();

    await waitFor(() => expect(screen.getByText('Reversed')).toBeTruthy());
    expect(screen.queryByRole('button', { name: 'Reverse' })).toBeNull();
    expect(screen.queryByRole('button', { name: /reverse group/i })).toBeNull();
  });

  it('hides the reverse action on a row that another transaction has already reversed', async () => {
    vi.mocked(commands.listTransactions).mockReturnValue(
      ok([
        txn({ id: 't2', txn_type: 'reverse', reversed_txn_id: 't1' }),
        txn({ id: 't1', txn_type: 'receive', group_id: null }),
      ]),
    );

    renderTransactions();

    await waitFor(() => expect(screen.getByText('Received')).toBeTruthy());
    expect(screen.queryByRole('button', { name: 'Reverse' })).toBeNull();
  });

  it('toasts a hinted error when the reverse mutation fails', async () => {
    vi.mocked(commands.listTransactions).mockReturnValue(
      ok([txn({ id: 't1', group_id: null, txn_type: 'receive' })]),
    );
    vi.mocked(commands.reverseTransaction).mockReturnValue(
      commandError('already_reversed', 'transaction was already reversed'),
    );

    renderTransactions();

    const reverseButton = await screen.findByRole('button', { name: 'Reverse' });
    fireEvent.click(reverseButton);

    await waitFor(() => expect(screen.getByText('Could not reverse transaction')).toBeTruthy());
    expect(screen.getByText('This transaction or group was already reversed.')).toBeTruthy();
  });

  it('resolves a project name for a project-scoped transaction', async () => {
    vi.mocked(commands.listProjects).mockReturnValue(ok([{ id: 'proj-1', name: 'Blinky Board' }]));
    vi.mocked(commands.listTransactions).mockReturnValue(
      ok([
        txn({
          id: 't1',
          txn_type: 'reserve',
          project_id: 'proj-1',
          from_state: 'available',
          to_state: 'reserved',
        }),
      ]),
    );

    renderTransactions();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
  });

  it('shows an empty state when there are no transactions', async () => {
    vi.mocked(commands.listTransactions).mockReturnValue(ok([]));

    renderTransactions();

    await waitFor(() => expect(screen.getByText(/no transactions/i)).toBeTruthy());
  });
});
