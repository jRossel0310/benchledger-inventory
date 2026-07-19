import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { GroupRecord, ImportRecord, ImportReview } from '../bindings.gen';
import { commands } from '../lib/commands';
import {
  keys,
  useCommitImport,
  useImportLines,
  useImportReview,
  useImports,
  useParseImport,
  useReverseImport,
} from './imports';

function makeClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
}

function wrapperFor(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('query keys', () => {
  it('extend inventory.ts’s keys with the import entries', () => {
    // The shared entity keys (spread from inventory.ts) are still present...
    expect(keys.allParts).toEqual(['parts']);
    expect(keys.dashboardSummary).toEqual(['dashboardSummary']);
    // ...alongside the new import keys.
    expect(keys.allImports).toEqual(['imports']);
    expect(keys.importReview('imp1')).toEqual(['importReview', 'imp1']);
    expect(keys.importLines('imp1')).toEqual(['importLines', 'imp1']);
  });
});

describe('query hooks', () => {
  it('useImports calls commands.listImports', async () => {
    vi.spyOn(commands, 'listImports').mockResolvedValue({ status: 'ok', data: [] });
    const queryClient = makeClient();

    const { result } = renderHook(() => useImports(), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.listImports).toHaveBeenCalled();
  });

  it('useImportReview calls commands.getImportReview with the given id and is disabled without one', async () => {
    const review = { import: { id: 'imp1' }, lines: [] } as unknown as ImportReview;
    const spy = vi
      .spyOn(commands, 'getImportReview')
      .mockResolvedValue({ status: 'ok', data: review });
    const queryClient = makeClient();

    renderHook(() => useImportReview(undefined), { wrapper: wrapperFor(queryClient) });
    expect(spy).not.toHaveBeenCalled();

    const { result } = renderHook(() => useImportReview('imp1'), {
      wrapper: wrapperFor(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.getImportReview).toHaveBeenCalledWith('imp1');
    expect(result.current.data).toBe(review);
  });

  it('useImportLines calls commands.listImportLines with the given id and is disabled without one', async () => {
    const spy = vi.spyOn(commands, 'listImportLines').mockResolvedValue({ status: 'ok', data: [] });
    const queryClient = makeClient();

    renderHook(() => useImportLines(undefined), { wrapper: wrapperFor(queryClient) });
    expect(spy).not.toHaveBeenCalled();

    const { result } = renderHook(() => useImportLines('imp1'), {
      wrapper: wrapperFor(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.listImportLines).toHaveBeenCalledWith('imp1');
  });
});

describe('useParseImport', () => {
  it('calls parseAndStoreImport with bytes/filename and invalidates the import list', async () => {
    const record = { id: 'imp1', supplier: 'DigiKey' } as unknown as ImportRecord;
    vi.spyOn(commands, 'parseAndStoreImport').mockResolvedValue({ status: 'ok', data: record });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useParseImport(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ bytes: [1, 2, 3], filename: 'order.csv' });
    });

    expect(commands.parseAndStoreImport).toHaveBeenCalledWith([1, 2, 3], 'order.csv');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allImports }),
    );
  });
});

describe('commit/reverse mutation hooks (ledger-mutating)', () => {
  const group = {
    id: 'g1',
    kind: 'import_commit',
    note: '',
    reversed_group_id: null,
    created_at: '',
    transactions: [
      { id: 't1', part_id: 'p1' },
      { id: 't2', part_id: 'p2' },
      { id: 't3', part_id: 'p1' },
    ],
  } as unknown as GroupRecord;

  it('useCommitImport calls commitImport with importId/decisions and invalidates imports + the broad ledger surface', async () => {
    vi.spyOn(commands, 'commitImport').mockResolvedValue({ status: 'ok', data: group });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useCommitImport(), { wrapper: wrapperFor(queryClient) });
    const decisions = [['line1', { type: 'add_stock', part_id: 'p1' }]] as unknown as Parameters<
      typeof commands.commitImport
    >[1];
    await act(async () => {
      await result.current.mutateAsync({ importId: 'imp1', decisions });
    });

    expect(commands.commitImport).toHaveBeenCalledWith('imp1', decisions);

    // Import-specific keys.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allImports }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.importReview('imp1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.importLines('imp1') }),
    );

    // The broad ledger surface, per distinct part touched by the group.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.stock('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.stock('p2') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.transactions('p1') }),
    );
    // Once per distinct part, not once per transaction (p1 appears twice).
    expect(
      invalidateSpy.mock.calls.filter(
        ([arg]) => JSON.stringify(arg?.queryKey) === JSON.stringify(keys.stock('p1')),
      ),
    ).toHaveLength(1);
    // Per-part detail/variants for every part the group's transactions touch
    // — an `add_as_variant` decision adds a variant + listing to an EXISTING
    // part, so without this a cached part detail silently misses the new
    // variant (Finding 1, Phase 5d final review).
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p2') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.variants('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.variants('p2') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allParts }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dashboardSummary }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allRecentTransactions }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allHistory }),
    );
    // A `create_new` decision's part can carry a bin_label — the bin tile
    // grid's counts need the same invalidation `useCreatePart` already gives
    // a plain part creation (Task 4's fix for this gap).
    expect(invalidateSpy).toHaveBeenCalledWith(expect.objectContaining({ queryKey: keys.bins }));
  });

  it('useCommitImport invalidates part/variants for a decision-targeted part even when the group has no transaction for it (fully-backordered add_as_variant)', async () => {
    // p3 gets a new variant via `add_as_variant`, but `shipped_milli` is
    // None/0 so `import_commit.rs` skips the receive op for it — the
    // returned group's `transactions` therefore never mention p3, even
    // though the part now has a stale cached variant list.
    const groupNoReceiveForP3 = {
      id: 'g2',
      kind: 'import_commit',
      note: '',
      reversed_group_id: null,
      created_at: '',
      transactions: [{ id: 't1', part_id: 'p1' }],
    } as unknown as GroupRecord;
    vi.spyOn(commands, 'commitImport').mockResolvedValue({
      status: 'ok',
      data: groupNoReceiveForP3,
    });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useCommitImport(), { wrapper: wrapperFor(queryClient) });
    const decisions = [
      ['line1', { type: 'add_stock', part_id: 'p1' }],
      ['line3', { type: 'add_as_variant', part_id: 'p3', variant: {}, listing: {} }],
    ] as unknown as Parameters<typeof commands.commitImport>[1];
    await act(async () => {
      await result.current.mutateAsync({ importId: 'imp1', decisions });
    });

    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p3') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.variants('p3') }),
    );
  });

  it('useReverseImport calls reverseImport with importId/note and invalidates imports + the broad ledger surface', async () => {
    vi.spyOn(commands, 'reverseImport').mockResolvedValue({ status: 'ok', data: group });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useReverseImport(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ importId: 'imp1', note: 'wrong shipment' });
    });

    expect(commands.reverseImport).toHaveBeenCalledWith('imp1', 'wrong shipment');

    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allImports }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.importReview('imp1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.importLines('imp1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.stock('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.transactions('p2') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dashboardSummary }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allHistory }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(expect.objectContaining({ queryKey: keys.bins }));
    // Same per-part detail/variants invalidation reversal needs too — a
    // reversed `add_as_variant` commit leaves the part's variant list as-is
    // (variants aren't deleted on reversal), but any other reversal-driven
    // change to the part (e.g. stock back to zero) still needs a fresh
    // `keys.part`/`keys.variants` fetch, same as the commit direction.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p2') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.variants('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.variants('p2') }),
    );
  });
});
