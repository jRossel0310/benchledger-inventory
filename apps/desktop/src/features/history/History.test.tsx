import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listHistory: vi.fn(),
      listProjects: vi.fn(),
      search: vi.fn(),
      reverseTransaction: vi.fn(),
      reverseGroup: vi.fn(),
      setPartArchived: vi.fn(),
    },
  };
});

import type { GroupRecord, HistoryPage, HistoryRow, TransactionRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { History } from './History';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

function historyRow(overrides: Partial<HistoryRow> = {}): HistoryRow {
  return {
    id: 't1',
    part_id: 'p1',
    display_name: 'Test resistor',
    quantity_unit: 'each',
    part_archived: false,
    txn_type: 'receive',
    quantity: 1000,
    from_state: null,
    to_state: null,
    project_id: null,
    to_project_id: null,
    project_name: null,
    note: '',
    group_id: null,
    group_kind: null,
    reversed_txn_id: null,
    import_id: null,
    created_at: '2026-07-01 00:00:00',
    reversible: true,
    ...overrides,
  };
}

function historyPage(rows: HistoryRow[], total?: number): HistoryPage {
  return { rows, total: total ?? rows.length };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.listProjects).mockReturnValue(ok([]));
  vi.mocked(commands.search).mockReturnValue(ok([]));
});

afterEach(cleanup);

function renderHistory(initialGroupId: string | null = null) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <History initialGroupId={initialGroupId} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('History', () => {
  it('renders history rows from list_history', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(historyPage([historyRow({ id: 't1', display_name: 'Receive row' })])),
    );
    renderHistory();

    expect(await screen.findByText('Receive row')).toBeTruthy();
  });

  it('shows a friendly message when nothing matches', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(ok(historyPage([])));
    renderHistory();

    expect(await screen.findByText('No transactions match these filters.')).toBeTruthy();
  });

  it('shows a hinted error when the query fails', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(commandError('sqlite', 'database is locked'));
    renderHistory();

    expect(await screen.findByText(/Could not load history/)).toBeTruthy();
  });

  it('changing the type filter re-queries with the selected txn_type', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(historyPage([historyRow({ id: 't1', display_name: 'Any row' })])),
    );
    renderHistory();
    await screen.findByText('Any row');

    fireEvent.change(screen.getByLabelText('Type'), { target: { value: 'reserve' } });

    await waitFor(() =>
      expect(commands.listHistory).toHaveBeenCalledWith(
        expect.objectContaining({ txn_type: 'reserve', offset: 0 }),
      ),
    );
  });

  it('a group row expands to show its member rows', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            display_name: 'Member cap',
            group_id: 'g1',
            group_kind: 'receive_batch',
            reversible: false,
          }),
        ]),
      ),
    );
    renderHistory();

    const toggle = await screen.findByRole('button', { name: /Receive Batch/ });
    expect(screen.queryByText('Member cap')).toBeNull();

    fireEvent.click(toggle);

    expect(await screen.findByText('Member cap')).toBeTruthy();
  });

  it('a grouped member row has no individual reverse control', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            display_name: 'Member x',
            group_id: 'g1',
            group_kind: 'receive_batch',
            reversible: false,
          }),
        ]),
      ),
    );
    renderHistory();

    fireEvent.click(await screen.findByRole('button', { name: /Receive Batch/ }));
    await screen.findByText('Member x');

    expect(screen.queryByRole('button', { name: 'Reverse' })).toBeNull();
  });

  it('reversing a group opens a confirmation listing member ops, then calls useReverseGroup', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            display_name: 'Cap A',
            group_id: 'g1',
            group_kind: 'receive_batch',
            reversible: false,
          }),
          historyRow({
            id: 't2',
            display_name: 'Cap B',
            group_id: 'g1',
            group_kind: 'receive_batch',
            reversible: false,
          }),
        ]),
      ),
    );
    vi.mocked(commands.reverseGroup).mockReturnValue(
      ok({
        id: 'g1r',
        kind: 'reverse:receive_batch',
        note: '',
        reversed_group_id: 'g1',
        created_at: '',
        transactions: [],
      } as unknown as GroupRecord),
    );
    renderHistory();

    fireEvent.click(await screen.findByRole('button', { name: 'Reverse group' }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText(/Cap A/)).toBeTruthy();
    expect(within(dialog).getByText(/Cap B/)).toBeTruthy();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Reverse group' }));

    await waitFor(() =>
      expect(commands.reverseGroup).toHaveBeenCalledWith('g1', expect.any(String)),
    );
    expect(await screen.findByText('Group reversed')).toBeTruthy();
  });

  it('cancelling the reverse-group confirmation does not call the command', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            display_name: 'Cap A',
            group_id: 'g1',
            group_kind: 'receive_batch',
            reversible: false,
          }),
        ]),
      ),
    );
    renderHistory();

    fireEvent.click(await screen.findByRole('button', { name: 'Reverse group' }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));

    expect(commands.reverseGroup).not.toHaveBeenCalled();
  });

  it('reversing an individual row calls useReverseTransaction and toasts', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(historyPage([historyRow({ id: 't1', display_name: 'Solo receive', reversible: true })])),
    );
    vi.mocked(commands.reverseTransaction).mockReturnValue(
      ok({ id: 't2', part_id: 'p1', reversed_txn_id: 't1' } as unknown as TransactionRecord),
    );
    renderHistory();

    fireEvent.click(await screen.findByRole('button', { name: 'Reverse' }));

    await waitFor(() =>
      expect(commands.reverseTransaction).toHaveBeenCalledWith('t1', expect.any(String)),
    );
    expect(await screen.findByText('Transaction reversed')).toBeTruthy();
  });

  it('a reversal row shows no reverse control', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't2',
            display_name: 'Reversal row',
            txn_type: 'reverse',
            reversed_txn_id: 't1',
            reversible: false,
          }),
        ]),
      ),
    );
    renderHistory();

    await screen.findByText('Reversal row');
    expect(screen.queryByRole('button', { name: 'Reverse' })).toBeNull();
  });

  it('an already-reversed original row shows no reverse control', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            display_name: 'Already reversed original',
            reversible: false,
          }),
        ]),
      ),
    );
    renderHistory();

    await screen.findByText('Already reversed original');
    expect(screen.queryByRole('button', { name: 'Reverse' })).toBeNull();
  });

  it('shows a restore action for a row referencing an archived part', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            display_name: 'Archived part row',
            part_archived: true,
            reversible: false,
          }),
        ]),
      ),
    );
    vi.mocked(commands.setPartArchived).mockReturnValue(ok(null));
    renderHistory();

    fireEvent.click(await screen.findByRole('button', { name: 'Restore part' }));

    await waitFor(() => expect(commands.setPartArchived).toHaveBeenCalledWith('p1', false));
    expect(await screen.findByText('Part restored')).toBeTruthy();
  });

  it('the "view original import" stub toasts a Phase 5 message rather than doing nothing', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([historyRow({ id: 't1', display_name: 'Imported row', import_id: 'imp_123' })]),
      ),
    );
    renderHistory();

    fireEvent.click(await screen.findByRole('button', { name: 'View original import' }));

    expect(await screen.findByText('Import viewer arrives in Phase 5')).toBeTruthy();
  });

  it('a row with no import_id never shows a "view original import" link', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(historyPage([historyRow({ id: 't1', display_name: 'No import row' })])),
    );
    renderHistory();

    await screen.findByText('No import row');
    expect(screen.queryByRole('button', { name: 'View original import' })).toBeNull();
  });

  it('paginates via prev/next, requesting the right offset', async () => {
    const rows = Array.from({ length: 25 }, (_, i) =>
      historyRow({ id: `t${i}`, display_name: `Row ${i}` }),
    );
    vi.mocked(commands.listHistory).mockReturnValue(ok(historyPage(rows, 40)));
    renderHistory();

    expect(await screen.findByText('1–25 of 40')).toBeTruthy();
    const prevButton = screen.getByRole('button', { name: 'Prev' }) as HTMLButtonElement;
    const nextButton = screen.getByRole('button', { name: 'Next' }) as HTMLButtonElement;
    expect(prevButton.disabled).toBe(true);
    expect(nextButton.disabled).toBe(false);

    fireEvent.click(nextButton);

    await waitFor(() =>
      expect(commands.listHistory).toHaveBeenCalledWith(expect.objectContaining({ offset: 25 })),
    );
  });

  it('an initialGroupId pre-fills the group filter and shows a clear chip', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(ok(historyPage([])));
    renderHistory('g1');

    await waitFor(() =>
      expect(commands.listHistory).toHaveBeenCalledWith(
        expect.objectContaining({ group_id: 'g1' }),
      ),
    );
    expect(await screen.findByText('Viewing one group')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'View all' }));

    await waitFor(() =>
      expect(commands.listHistory).toHaveBeenCalledWith(
        expect.objectContaining({ group_id: null }),
      ),
    );
    expect(screen.queryByText('Viewing one group')).toBeNull();
  });
});
