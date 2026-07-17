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
      listParts: vi.fn(),
      search: vi.fn(),
      reverseTransaction: vi.fn(),
      reverseGroup: vi.fn(),
      setPartArchived: vi.fn(),
      getGroup: vi.fn(),
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
    group_total: 0,
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

function groupTransaction(overrides: Partial<TransactionRecord> = {}): TransactionRecord {
  return {
    id: 't1',
    part_id: 'p1',
    group_id: 'g1',
    txn_type: 'receive',
    quantity: 1000,
    from_state: null,
    to_state: null,
    project_id: null,
    to_project_id: null,
    note: '',
    reversed_txn_id: null,
    created_at: '2026-07-01 00:00:00',
    ...overrides,
  };
}

function groupRecord(overrides: Partial<GroupRecord> = {}): GroupRecord {
  return {
    id: 'g1',
    kind: 'receive_batch',
    note: '',
    reversed_group_id: null,
    created_at: '2026-07-01 00:00:00',
    transactions: [groupTransaction()],
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.listProjects).mockReturnValue(ok([]));
  vi.mocked(commands.search).mockReturnValue(ok([]));
  vi.mocked(commands.listParts).mockReturnValue(ok([]));
  // Most tests that open the reverse-group confirmation don't care about its
  // content — only the tests that assert on the true full list override
  // this with a specific `groupRecord()`.
  vi.mocked(commands.getGroup).mockReturnValue(ok(groupRecord()));
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
            group_total: 1,
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
            group_total: 1,
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
            group_total: 2,
            reversible: false,
          }),
          historyRow({
            id: 't2',
            display_name: 'Cap B',
            group_id: 'g1',
            group_kind: 'receive_batch',
            group_total: 2,
            reversible: false,
          }),
        ]),
      ),
    );
    // The confirmation's op list is sourced from `get_group`, not from the
    // visible `members` — matching ids here (t1/t2) lets it resolve each
    // transaction's name from the already-visible rows.
    vi.mocked(commands.getGroup).mockReturnValue(
      ok(
        groupRecord({
          transactions: [
            groupTransaction({ id: 't1', part_id: 'p1' }),
            groupTransaction({ id: 't2', part_id: 'p2' }),
          ],
        }),
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
    expect(await within(dialog).findByText(/Cap A/)).toBeTruthy();
    expect(await within(dialog).findByText(/Cap B/)).toBeTruthy();

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
            group_total: 1,
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

  it('the group header shows the true group_total and flags hidden members when the visible count is smaller', async () => {
    // Only one row of a real two-member group is on this (filtered/paged)
    // result — `group_total` still reports the group's real size.
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            display_name: 'Only visible member',
            group_id: 'g1',
            group_kind: 'receive_batch',
            group_total: 2,
            reversible: false,
          }),
        ]),
      ),
    );
    renderHistory();

    const toggle = await screen.findByRole('button', { name: /Receive Batch/ });
    expect(within(toggle).getByText('2 operations')).toBeTruthy();
    expect(within(toggle).getByText(/some not shown under current filter/)).toBeTruthy();
  });

  it('the group header does not flag hidden members when every member is visible', async () => {
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            display_name: 'Cap A',
            group_id: 'g1',
            group_kind: 'receive_batch',
            group_total: 2,
            reversible: false,
          }),
          historyRow({
            id: 't2',
            display_name: 'Cap B',
            group_id: 'g1',
            group_kind: 'receive_batch',
            group_total: 2,
            reversible: false,
          }),
        ]),
      ),
    );
    renderHistory();

    const toggle = await screen.findByRole('button', { name: /Receive Batch/ });
    expect(within(toggle).getByText('2 operations')).toBeTruthy();
    expect(screen.queryByText(/some not shown under current filter/)).toBeNull();
  });

  it('the reverse-group confirmation shows the TRUE full group from get_group even when only one member is visible, and disables confirm until it loads', async () => {
    // A part filter (or a page boundary) means this History page only ever
    // saw one of the group's two real members.
    vi.mocked(commands.listHistory).mockReturnValue(
      ok(
        historyPage([
          historyRow({
            id: 't1',
            part_id: 'p1',
            display_name: 'Visible Cap',
            group_id: 'g1',
            group_kind: 'receive_batch',
            group_total: 2,
            reversible: false,
          }),
        ]),
      ),
    );

    type GetGroupResult = Awaited<ReturnType<typeof commands.getGroup>>;
    let resolveGetGroup: (value: GetGroupResult) => void = () => {};
    vi.mocked(commands.getGroup).mockReturnValue(
      new Promise<GetGroupResult>((resolve) => {
        resolveGetGroup = resolve;
      }),
    );

    renderHistory();
    fireEvent.click(await screen.findByRole('button', { name: 'Reverse group' }));
    const dialog = await screen.findByRole('dialog');

    // Still loading the true group: no partial op list, and confirm is
    // disabled — never let the user confirm against an incomplete list.
    expect(await within(dialog).findByText(/Loading the full group/)).toBeTruthy();
    const submit = within(dialog).getByRole('button', {
      name: 'Reverse group',
    }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    expect(within(dialog).queryByText('Visible Cap')).toBeNull();

    resolveGetGroup({
      status: 'ok',
      data: groupRecord({
        transactions: [
          groupTransaction({ id: 't1', part_id: 'p1' }),
          // t2 was never on the visible History page, so its name can
          // only come from get_group's own data — proving the dialog
          // renders the TRUE full list, not just what was on screen.
          groupTransaction({ id: 't2', part_id: 'p2' }),
        ],
      }),
    });

    await waitFor(() => expect(submit.disabled).toBe(false));
    expect(within(dialog).getByText(/All 2 operations will be undone/)).toBeTruthy();
    expect(within(dialog).getByText(/Visible Cap/)).toBeTruthy();
    // No visible-row/part-list name available for p2 — still rendered
    // honestly via its raw part id rather than being dropped.
    expect(within(dialog).getByText(/p2/)).toBeTruthy();
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
