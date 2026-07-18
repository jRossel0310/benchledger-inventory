import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type {
  BomItemRecord,
  BuildPlan,
  GroupRecord,
  ProjectRecord,
  TransactionRecord,
} from '../bindings.gen';
import { commands } from '../lib/commands';
import {
  keys,
  useAddBomItem,
  useArchiveProject,
  useAssociateCheckout,
  useBom,
  useBuildFromBom,
  useCreateProjectFull,
  useDuplicateProject,
  useImportBom,
  usePlanBuild,
  useProject,
  useProjectsFull,
  useReleaseBom,
  useRemoveBomItem,
  useReserveBom,
  useSetBomSubstitutes,
  useSetProjectStatus,
  useUpdateBomItem,
  useUpdateProject,
} from './projects';

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
  it('extend inventory.ts’s keys with project/BOM entries, distinct from the Phase 3 ProjectRef stub', () => {
    // The shared entity keys (spread from inventory.ts) are still present...
    expect(keys.allParts).toEqual(['parts']);
    expect(keys.dashboardSummary).toEqual(['dashboardSummary']);
    // ...alongside the Phase 3 ProjectRef quick-action-picker key, untouched...
    expect(keys.projects).toEqual(['projects']);
    // ...and the new rich-project/BOM keys, under distinct names.
    expect(keys.projectsFull(null)).toEqual(['projectsFull', null]);
    expect(keys.projectsFull('active')).toEqual(['projectsFull', 'active']);
    expect(keys.projectsFull(null)[0]).toBe(keys.allProjectsFull[0]);
    expect(keys.project('proj1')).toEqual(['project', 'proj1']);
    expect(keys.bom('proj1')).toEqual(['bom', 'proj1']);
    expect(keys.bom('proj1')[0]).toBe(keys.allBom[0]);
    expect(keys.planBuild('proj1')).toEqual(['planBuild', 'proj1']);
    expect(keys.planBuild('proj1')[0]).toBe(keys.allPlanBuild[0]);
  });
});

describe('query hooks', () => {
  it('useProjectsFull calls commands.listProjectsFull with null when no filter is given', async () => {
    vi.spyOn(commands, 'listProjectsFull').mockResolvedValue({ status: 'ok', data: [] });
    const queryClient = makeClient();

    const { result } = renderHook(() => useProjectsFull(), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.listProjectsFull).toHaveBeenCalledWith(null);
  });

  it('useProjectsFull passes a given status filter through', async () => {
    vi.spyOn(commands, 'listProjectsFull').mockResolvedValue({ status: 'ok', data: [] });
    const queryClient = makeClient();

    renderHook(() => useProjectsFull('active'), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(commands.listProjectsFull).toHaveBeenCalledWith('active'));
  });

  it('useProject calls commands.getProject with the given id and is disabled without one', async () => {
    const project = { id: 'proj1', name: 'Blinky Board' } as unknown as ProjectRecord;
    const spy = vi.spyOn(commands, 'getProject').mockResolvedValue({ status: 'ok', data: project });
    const queryClient = makeClient();

    renderHook(() => useProject(undefined), { wrapper: wrapperFor(queryClient) });
    expect(spy).not.toHaveBeenCalled();

    const { result } = renderHook(() => useProject('proj1'), { wrapper: wrapperFor(queryClient) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.getProject).toHaveBeenCalledWith('proj1');
    expect(result.current.data).toBe(project);
  });

  it('useBom calls commands.listBom with the given project id and is disabled without one', async () => {
    const items = [{ id: 'b1', project_id: 'proj1' }] as unknown as BomItemRecord[];
    const spy = vi.spyOn(commands, 'listBom').mockResolvedValue({ status: 'ok', data: items });
    const queryClient = makeClient();

    renderHook(() => useBom(undefined), { wrapper: wrapperFor(queryClient) });
    expect(spy).not.toHaveBeenCalled();

    const { result } = renderHook(() => useBom('proj1'), { wrapper: wrapperFor(queryClient) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.listBom).toHaveBeenCalledWith('proj1');
    expect(result.current.data).toBe(items);
  });

  it('usePlanBuild calls commands.planBuild with the given project id and is disabled without one', async () => {
    const plan = { lines: [] } as unknown as BuildPlan;
    const spy = vi.spyOn(commands, 'planBuild').mockResolvedValue({ status: 'ok', data: plan });
    const queryClient = makeClient();

    renderHook(() => usePlanBuild(undefined), { wrapper: wrapperFor(queryClient) });
    expect(spy).not.toHaveBeenCalled();

    const { result } = renderHook(() => usePlanBuild('proj1'), {
      wrapper: wrapperFor(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.planBuild).toHaveBeenCalledWith('proj1');
    expect(result.current.data).toBe(plan);
  });
});

describe('project lifecycle mutation hooks', () => {
  it('useCreateProjectFull invalidates the full project list, the new project, and the dashboard', async () => {
    const project = { id: 'proj1', name: 'Blinky Board' } as unknown as ProjectRecord;
    vi.spyOn(commands, 'createProjectFull').mockResolvedValue({ status: 'ok', data: project });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useCreateProjectFull(), {
      wrapper: wrapperFor(queryClient),
    });
    const draft = { name: 'Blinky Board' } as unknown as Parameters<
      typeof commands.createProjectFull
    >[0];
    await act(async () => {
      await result.current.mutateAsync(draft);
    });

    expect(commands.createProjectFull).toHaveBeenCalledWith(draft);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.project('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allProjectsFull }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dashboardSummary }),
    );
  });

  it('useUpdateProject invalidates that project, the full list, and the dashboard', async () => {
    vi.spyOn(commands, 'updateProject').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useUpdateProject(), { wrapper: wrapperFor(queryClient) });
    const record = { id: 'proj1', name: 'Blinky Board v2' } as unknown as ProjectRecord;
    await act(async () => {
      await result.current.mutateAsync(record);
    });

    expect(commands.updateProject).toHaveBeenCalledWith(record);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.project('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allProjectsFull }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dashboardSummary }),
    );
  });

  it('useSetProjectStatus calls the command and invalidates the project/list/dashboard', async () => {
    vi.spyOn(commands, 'setProjectStatus').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetProjectStatus(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ id: 'proj1', status: 'active' });
    });

    expect(commands.setProjectStatus).toHaveBeenCalledWith('proj1', 'active');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.project('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allProjectsFull }),
    );
  });

  it('useDuplicateProject calls the command and invalidates the new project/list/dashboard', async () => {
    const dup = { id: 'proj2', name: 'Blinky Board copy' } as unknown as ProjectRecord;
    vi.spyOn(commands, 'duplicateProject').mockResolvedValue({ status: 'ok', data: dup });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useDuplicateProject(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ id: 'proj1', newName: 'Blinky Board copy' });
    });

    expect(commands.duplicateProject).toHaveBeenCalledWith('proj1', 'Blinky Board copy');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.project('proj2') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allProjectsFull }),
    );
  });

  it('useArchiveProject calls the command and invalidates the project/list/dashboard', async () => {
    vi.spyOn(commands, 'archiveProject').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useArchiveProject(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync('proj1');
    });

    expect(commands.archiveProject).toHaveBeenCalledWith('proj1');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.project('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allProjectsFull }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dashboardSummary }),
    );
  });
});

describe('BOM editing mutation hooks', () => {
  it('useAddBomItem invalidates that project’s bom and plan-build, not the project list', async () => {
    const item = { id: 'b1', project_id: 'proj1' } as unknown as BomItemRecord;
    vi.spyOn(commands, 'addBomItem').mockResolvedValue({ status: 'ok', data: item });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useAddBomItem(), { wrapper: wrapperFor(queryClient) });
    const draft = { part_id: 'p1' } as unknown as Parameters<typeof commands.addBomItem>[1];
    await act(async () => {
      await result.current.mutateAsync({ projectId: 'proj1', draft });
    });

    expect(commands.addBomItem).toHaveBeenCalledWith('proj1', draft);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.planBuild('proj1') }),
    );
    expect(invalidateSpy).not.toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allProjectsFull }),
    );
  });

  it('useUpdateBomItem calls updateBomItem with id/draft and invalidates via the given projectId', async () => {
    const item = { id: 'b1', project_id: 'proj1' } as unknown as BomItemRecord;
    vi.spyOn(commands, 'updateBomItem').mockResolvedValue({ status: 'ok', data: item });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useUpdateBomItem(), { wrapper: wrapperFor(queryClient) });
    const draft = { part_id: 'p1' } as unknown as Parameters<typeof commands.updateBomItem>[1];
    await act(async () => {
      await result.current.mutateAsync({ projectId: 'proj1', id: 'b1', draft });
    });

    expect(commands.updateBomItem).toHaveBeenCalledWith('b1', draft);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.planBuild('proj1') }),
    );
  });

  it('useRemoveBomItem calls removeBomItem with only the id and invalidates via the given projectId', async () => {
    vi.spyOn(commands, 'removeBomItem').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useRemoveBomItem(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: 'proj1', id: 'b1' });
    });

    expect(commands.removeBomItem).toHaveBeenCalledWith('b1');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.planBuild('proj1') }),
    );
  });

  it('useSetBomSubstitutes calls the command and invalidates via the given projectId', async () => {
    vi.spyOn(commands, 'setBomSubstitutes').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetBomSubstitutes(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ projectId: 'proj1', bomItemId: 'b1', partIds: ['p2'] });
    });

    expect(commands.setBomSubstitutes).toHaveBeenCalledWith('b1', ['p2']);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.planBuild('proj1') }),
    );
  });

  it('useImportBom calls the command with project id and rows, and invalidates bom/planBuild', async () => {
    vi.spyOn(commands, 'importBom').mockResolvedValue({ status: 'ok', data: [] });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useImportBom(), { wrapper: wrapperFor(queryClient) });
    const rows = [{ part_id: 'p1' }] as unknown as Parameters<typeof commands.importBom>[1];
    await act(async () => {
      await result.current.mutateAsync({ projectId: 'proj1', rows });
    });

    expect(commands.importBom).toHaveBeenCalledWith('proj1', rows);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.planBuild('proj1') }),
    );
  });
});

describe('reserve/release/build/checkout mutation hooks (ledger-mutating)', () => {
  const group = {
    id: 'g1',
    kind: 'reserve_bom',
    note: '',
    reversed_group_id: null,
    created_at: '',
    transactions: [
      { id: 't1', part_id: 'p1' },
      { id: 't2', part_id: 'p2' },
      { id: 't3', part_id: 'p1' },
    ],
  } as unknown as GroupRecord;

  it('useReserveBom invalidates per-part stock/transactions, bom/planBuild/project, and the shared surfaces', async () => {
    vi.spyOn(commands, 'reserveBom').mockResolvedValue({ status: 'ok', data: group });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useReserveBom(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync('proj1');
    });

    expect(commands.reserveBom).toHaveBeenCalledWith('proj1');
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
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.planBuild('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.project('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allProjectsFull }),
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
  });

  it('useReleaseBom calls releaseBomReservations and invalidates the same broad surface', async () => {
    vi.spyOn(commands, 'releaseBomReservations').mockResolvedValue({ status: 'ok', data: group });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useReleaseBom(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync('proj1');
    });

    expect(commands.releaseBomReservations).toHaveBeenCalledWith('proj1');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dashboardSummary }),
    );
  });

  it('useBuildFromBom calls buildFromBom with approved lines and invalidates the broad surface including project status', async () => {
    vi.spyOn(commands, 'buildFromBom').mockResolvedValue({ status: 'ok', data: group });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useBuildFromBom(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: 'proj1', approvedAvailableLines: ['b1'] });
    });

    expect(commands.buildFromBom).toHaveBeenCalledWith('proj1', ['b1']);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.planBuild('proj1') }),
    );
    // A build can auto-activate a `planned` project, so the project itself
    // and the full-project list must be invalidated too.
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.project('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allProjectsFull }),
    );
  });

  it('useAssociateCheckout calls the command and invalidates stock/bom/shared surfaces', async () => {
    const txn = { id: 't1', part_id: 'p1' } as unknown as TransactionRecord;
    vi.spyOn(commands, 'associateCheckout').mockResolvedValue({ status: 'ok', data: txn });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useAssociateCheckout(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ projectId: 'proj1', partId: 'p1', quantity: 1000 });
    });

    expect(commands.associateCheckout).toHaveBeenCalledWith('proj1', 'p1', 1000);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.stock('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.transactions('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.bom('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.planBuild('proj1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dashboardSummary }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allHistory }),
    );
  });
});
