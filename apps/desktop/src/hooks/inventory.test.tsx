import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { CommandError, GroupRecord, PartRecord, TransactionRecord } from '../bindings.gen';
import { commands } from '../lib/commands';
import {
  keys,
  useAddAlias,
  useAddDimension,
  useAddSupplierListing,
  useAddVariant,
  useApplyLedgerOp,
  useCategories,
  useCreatePart,
  usePart,
  useParts,
  useRecordEquivalence,
  useReverseGroup,
  useReverseTransaction,
  useSearch,
  useSetArchived,
  useSetAttribute,
  useSetTags,
  useStock,
  useUpdatePart,
} from './inventory';

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
  it('are stable arrays keyed by entity and id', () => {
    expect(keys.parts(false)).toEqual(['parts', false]);
    expect(keys.part('p1')).toEqual(['part', 'p1']);
    expect(keys.stock('p1')).toEqual(['stock', 'p1']);
    expect(keys.search('resistor')).toEqual(['search', 'resistor']);
    // Every entity-scoped key starts with the same prefix as its "list"
    // key, so `invalidateQueries({ queryKey: keys.allParts })` fuzzy-matches
    // every `useParts(...)` variant regardless of the includeArchived flag.
    expect(keys.parts(true)[0]).toBe(keys.allParts[0]);
    expect(keys.parts(false)[0]).toBe(keys.allParts[0]);
  });
});

describe('query hooks', () => {
  it('useParts calls commands.listParts with includeArchived and returns its data', async () => {
    const parts = [{ id: 'p1', display_name: 'Resistor' }] as unknown as PartRecord[];
    vi.spyOn(commands, 'listParts').mockResolvedValue({ status: 'ok', data: parts });
    const queryClient = makeClient();

    const { result } = renderHook(() => useParts(false), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.listParts).toHaveBeenCalledWith(false);
    expect(result.current.data).toBe(parts);
  });

  it('usePart calls commands.getPart with the given id', async () => {
    const part = { id: 'p1', display_name: 'Resistor' } as unknown as PartRecord;
    vi.spyOn(commands, 'getPart').mockResolvedValue({ status: 'ok', data: part });
    const queryClient = makeClient();

    const { result } = renderHook(() => usePart('p1'), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.getPart).toHaveBeenCalledWith('p1');
    expect(result.current.data).toBe(part);
  });

  it('usePart does not call commands.getPart when no id is given (nothing selected yet)', () => {
    const spy = vi.spyOn(commands, 'getPart');
    const queryClient = makeClient();

    renderHook(() => usePart(undefined), { wrapper: wrapperFor(queryClient) });

    expect(spy).not.toHaveBeenCalled();
  });

  it('useStock calls commands.getStock with the given id', async () => {
    const stock = {
      available: 5000,
      reserved: 0,
      checked_out: 0,
      lifetime_received: 5000,
      lifetime_consumed: 0,
    };
    vi.spyOn(commands, 'getStock').mockResolvedValue({ status: 'ok', data: stock });
    const queryClient = makeClient();

    const { result } = renderHook(() => useStock('p1'), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.getStock).toHaveBeenCalledWith('p1');
    expect(result.current.data).toBe(stock);
  });

  it('useSearch calls commands.search only once a non-empty query is given', async () => {
    const spy = vi.spyOn(commands, 'search').mockResolvedValue({ status: 'ok', data: [] });
    const queryClient = makeClient();

    const { rerender } = renderHook(({ q }) => useSearch(q), {
      wrapper: wrapperFor(queryClient),
      initialProps: { q: '' },
    });
    expect(spy).not.toHaveBeenCalled();

    rerender({ q: 'resistor' });
    await waitFor(() => expect(spy).toHaveBeenCalledWith('resistor'));
  });

  it('useCategories calls commands.listCategories with no arguments', async () => {
    vi.spyOn(commands, 'listCategories').mockResolvedValue({ status: 'ok', data: [] });
    const queryClient = makeClient();

    const { result } = renderHook(() => useCategories(), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.listCategories).toHaveBeenCalledWith();
  });

  it('surfaces a CommandError through isError/error like any rejected query', async () => {
    const error: CommandError = { code: 'part_not_found', message: 'no such part' };
    vi.spyOn(commands, 'getPart').mockResolvedValue({ status: 'error', error });
    const queryClient = makeClient();

    const { result } = renderHook(() => usePart('missing'), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.error).toEqual(error);
  });
});

describe('mutation hooks', () => {
  it('useCreatePart calls commands.createPart and invalidates the parts list', async () => {
    const draft = { display_name: 'New part' } as unknown as Parameters<
      typeof commands.createPart
    >[0];
    const created = { id: 'p2', display_name: 'New part' } as unknown as PartRecord;
    vi.spyOn(commands, 'createPart').mockResolvedValue({ status: 'ok', data: created });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useCreatePart(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync(draft);
    });

    expect(commands.createPart).toHaveBeenCalledWith(draft);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allParts }),
    );
  });

  it('useUpdatePart invalidates the specific part and the parts list', async () => {
    const record = { id: 'p1', display_name: 'Updated' } as unknown as PartRecord;
    vi.spyOn(commands, 'updatePart').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useUpdatePart(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync(record);
    });

    expect(commands.updatePart).toHaveBeenCalledWith(record);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allParts }),
    );
  });

  it('useSetArchived calls commands.setPartArchived and invalidates the part and search', async () => {
    vi.spyOn(commands, 'setPartArchived').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetArchived(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ partId: 'p1', archived: true });
    });

    expect(commands.setPartArchived).toHaveBeenCalledWith('p1', true);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allParts }),
    );
    // The backend's default search excludes archived parts (and `is:archived`
    // returns only them), so archiving/restoring a part must invalidate
    // cached search/Ctrl+K results too, or they'd keep showing/hiding it.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
  });

  it('useApplyLedgerOp invalidates the affected part’s stock and transactions', async () => {
    const op = {
      type: 'receive',
      part_id: 'p1',
      quantity: 1000,
      note: '',
    } as unknown as Parameters<typeof commands.applyLedgerOp>[0];
    const txn = { id: 't1', part_id: 'p1' } as unknown as TransactionRecord;
    vi.spyOn(commands, 'applyLedgerOp').mockResolvedValue({ status: 'ok', data: txn });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useApplyLedgerOp(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync(op);
    });

    expect(commands.applyLedgerOp).toHaveBeenCalledWith(op);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.stock('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.transactions('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allParts }),
    );
    // Stock changes affect `available:`/`reserved:`/`checked_out:`/`stock:`
    // range filters and the low-stock flag, both evaluated live against
    // cached search results — those must be invalidated too.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
  });

  it('useReverseTransaction invalidates using the reversed transaction’s part id', async () => {
    const txn = { id: 't2', part_id: 'p9', reversed_txn_id: 't1' } as unknown as TransactionRecord;
    vi.spyOn(commands, 'reverseTransaction').mockResolvedValue({ status: 'ok', data: txn });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useReverseTransaction(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ txnId: 't1', note: 'oops' });
    });

    expect(commands.reverseTransaction).toHaveBeenCalledWith('t1', 'oops');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.stock('p9') }),
    );
  });

  it('useReverseGroup invalidates stock/transactions once per distinct affected part', async () => {
    const group = {
      id: 'g1',
      kind: 'receive_group',
      note: '',
      reversed_group_id: null,
      created_at: '',
      transactions: [
        { id: 't1', part_id: 'p1' },
        { id: 't2', part_id: 'p2' },
        { id: 't3', part_id: 'p1' },
      ],
    } as unknown as GroupRecord;
    vi.spyOn(commands, 'reverseGroup').mockResolvedValue({ status: 'ok', data: group });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useReverseGroup(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ groupId: 'g1', note: 'oops' });
    });

    expect(commands.reverseGroup).toHaveBeenCalledWith('g1', 'oops');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.stock('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.stock('p2') }),
    );
    expect(
      invalidateSpy.mock.calls.filter(
        ([arg]) => JSON.stringify(arg?.queryKey) === JSON.stringify(keys.stock('p1')),
      ),
    ).toHaveLength(1);
  });

  it('useSetTags calls commands.setTags, invalidates tags and search, and reports onDone', async () => {
    vi.spyOn(commands, 'setTags').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');
    const onDone = vi.fn();

    const { result } = renderHook(() => useSetTags({ onDone }), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ partId: 'p1', tags: ['smd', 'passive'] });
    });

    expect(commands.setTags).toHaveBeenCalledWith('p1', ['smd', 'passive']);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.tags('p1') }),
    );
    // Tags are part of the backend's search text, so a stale cached search
    // could keep showing/hiding this part by a tag it no longer has.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
    expect(onDone).toHaveBeenCalledWith(null, null);
  });

  it('useSetAttribute invalidates attributes, the part, and search', async () => {
    vi.spyOn(commands, 'setAttribute').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetAttribute(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ partId: 'p1', key: 'resistance', raw: '10k' });
    });

    expect(commands.setAttribute).toHaveBeenCalledWith('p1', 'resistance', '10k');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.attributes('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p1') }),
    );
    // The backend re-indexes the part's search text on every attribute
    // write, so cached search results keyed on that attribute can go stale.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
  });

  it('useAddVariant invalidates variants and search', async () => {
    const draft = { manufacturer: 'TI', mpn: 'LM358' } as unknown as Parameters<
      typeof commands.addVariant
    >[1];
    const variant = { id: 'v1', manufacturer: 'TI', mpn: 'LM358' } as unknown as ReturnType<
      typeof commands.addVariant
    > extends { data: infer R }
      ? R
      : never;
    vi.spyOn(commands, 'addVariant').mockResolvedValue({ status: 'ok', data: variant });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useAddVariant(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ partId: 'p1', draft });
    });

    expect(commands.addVariant).toHaveBeenCalledWith('p1', draft);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.variants('p1') }),
    );
    // Variant manufacturer/MPN are indexed into search_text, so cached search
    // results can go stale after adding a variant.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
  });

  it('useAddSupplierListing invalidates listings and search', async () => {
    const draft = { supplier: 'Mouser', sku: 'MOA123' } as unknown as Parameters<
      typeof commands.addSupplierListing
    >[1];
    const listing = { id: 'l1', supplier: 'Mouser', sku: 'MOA123' } as unknown as ReturnType<
      typeof commands.addSupplierListing
    > extends { data: infer R }
      ? R
      : never;
    vi.spyOn(commands, 'addSupplierListing').mockResolvedValue({ status: 'ok', data: listing });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useAddSupplierListing(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ variantId: 'v1', draft });
    });

    expect(commands.addSupplierListing).toHaveBeenCalledWith('v1', draft);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.supplierListings('v1') }),
    );
    // Supplier SKU is indexed into search_text, so cached search results
    // can go stale after adding a listing.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
  });

  it('useAddDimension invalidates dimensions and search', async () => {
    const draft = { name: 'Length', value: 10.5, unit: 'mm' } as unknown as Parameters<
      typeof commands.addDimension
    >[1];
    const dimension = {
      id: 'd1',
      name: 'Length',
      value: 10.5,
      unit: 'mm',
    } as unknown as ReturnType<typeof commands.addDimension> extends { data: infer R } ? R : never;
    vi.spyOn(commands, 'addDimension').mockResolvedValue({ status: 'ok', data: dimension });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useAddDimension(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ partId: 'p1', draft });
    });

    expect(commands.addDimension).toHaveBeenCalledWith('p1', draft);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dimensions('p1') }),
    );
    // Dimension names are indexed into search_text, so cached search results
    // can go stale after adding a dimension.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
  });

  it('useRecordEquivalence invalidates both parts but not search (no searchable content changes)', async () => {
    vi.spyOn(commands, 'recordEquivalence').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useRecordEquivalence(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ a: 'p1', b: 'p2', decision: 'approved', note: '' });
    });

    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p2') }),
    );
    expect(invalidateSpy).not.toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
  });

  it('useAddAlias invalidates the part but not search (aliases are not searchable content)', async () => {
    vi.spyOn(commands, 'addAlias').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useAddAlias(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({
        kind: 'mpn',
        value: 'TLV9002IDDFR',
        partId: 'p1',
        source: 'import',
      });
    });

    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p1') }),
    );
    expect(invalidateSpy).not.toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
  });

  it('reports a CommandError through onDone and does not invalidate on failure', async () => {
    const error: CommandError = { code: 'insufficient_stock', message: 'only 2 available' };
    vi.spyOn(commands, 'applyLedgerOp').mockResolvedValue({ status: 'error', error });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');
    const onDone = vi.fn();

    const op = {
      type: 'consume_available',
      part_id: 'p1',
      quantity: 3000,
      project_id: null,
      note: '',
    } as unknown as Parameters<typeof commands.applyLedgerOp>[0];
    const { result } = renderHook(() => useApplyLedgerOp({ onDone }), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync(op).catch(() => {});
    });

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(invalidateSpy).not.toHaveBeenCalled();
    expect(onDone).toHaveBeenCalledWith(error, undefined);
  });
});
