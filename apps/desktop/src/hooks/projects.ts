/**
 * The TanStack Query layer over the Phase 4 project/BOM/build command
 * surface (`create_project_full`, `list_projects_full`, `get_project`,
 * `update_project`, `set_project_status`, `duplicate_project`,
 * `archive_project`, the `bom_items` CRUD, `reserve_bom`/
 * `release_bom_reservations`/`plan_build`/`build_from_bom`/
 * `associate_checkout`). Same shape as `hooks/inventory.ts`: a query hook
 * per read, a mutation hook per write, one `keys` object, `unwrap` around
 * every command call. Kept in its own file (rather than folded into
 * `inventory.ts`) because it's a distinct, sizeable feature surface, but it
 * extends `inventory.ts`'s `keys` (spread, not duplicated) so a project/BOM
 * mutation can still invalidate the shared stock/search/dashboard/history
 * keys through the same object.
 *
 * The Phase 3 `useProjects`/`keys.projects` stub (`ProjectRef` — id + name,
 * for the Reserve/Check-out/Return quick-action pickers) stays in
 * `inventory.ts` untouched; `keys.projectsFull`/`useProjectsFull` here are
 * the rich, status-filterable list the Projects screen (Task 6/7) uses —
 * deliberately distinct names so the two never collide or get confused.
 */

import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';

import type {
  BomItemDraft,
  BomItemId,
  BomItemRecord,
  BuildPlan,
  CommandError,
  GroupRecord,
  PartId,
  ProjectDraft,
  ProjectId,
  ProjectRecord,
  ProjectStatus,
  Quantity,
  TransactionRecord,
} from '../bindings.gen';
import { commands, unwrap } from '../lib/commands';
import { keys as inventoryKeys, type MutationCallbacks } from './inventory';

/** Extends `inventory.ts`'s `keys` with the project/BOM entries this file's
 * hooks use — see the module doc comment for why this lives in its own
 * object instead of being merged back into `inventory.ts`. */
export const keys = {
  ...inventoryKeys,
  allProjectsFull: ['projectsFull'] as const,
  projectsFull: (statusFilter: ProjectStatus | null) => ['projectsFull', statusFilter] as const,
  project: (id: ProjectId) => ['project', id] as const,
  allBom: ['bom'] as const,
  bom: (projectId: ProjectId) => ['bom', projectId] as const,
  allPlanBuild: ['planBuild'] as const,
  planBuild: (projectId: ProjectId) => ['planBuild', projectId] as const,
};

// ---------------------------------------------------------------------
// Query hooks
// ---------------------------------------------------------------------

/** Every project (the rich record — status/description/build_quantity/
 * repo_link/notes/completed_at), optionally narrowed to one status —
 * `statusFilter` omitted or `undefined` means every status. The Projects
 * list screen's data source. */
export function useProjectsFull(
  statusFilter?: ProjectStatus,
): UseQueryResult<ProjectRecord[], CommandError> {
  const filter = statusFilter ?? null;
  return useQuery({
    queryKey: keys.projectsFull(filter),
    queryFn: () => unwrap(commands.listProjectsFull(filter)),
  });
}

export function useProject(
  id: ProjectId | undefined,
): UseQueryResult<ProjectRecord | null, CommandError> {
  return useQuery({
    queryKey: keys.project(id ?? ''),
    queryFn: () => unwrap(commands.getProject(id as ProjectId)),
    enabled: id !== undefined,
  });
}

/** A project's BOM lines, each already carrying the spec's per-build/
 * needed/available/reserved/consumed/missing columns computed server-side
 * (`list_bom` — ledger-derived, never stored; see `bom.rs`'s
 * `derive_reserved_consumed`). */
export function useBom(
  projectId: ProjectId | undefined,
): UseQueryResult<BomItemRecord[], CommandError> {
  return useQuery({
    queryKey: keys.bom(projectId ?? ''),
    queryFn: () => unwrap(commands.listBom(projectId as ProjectId)),
    enabled: projectId !== undefined,
  });
}

/** A dry-run of exactly what `build_from_bom` would do right now (`plan_build`
 * — no mutation). Goes through the query cache like any other read, but
 * because it reflects live, fast-changing state (stock/reservations), a
 * caller presenting it for confirmation (the Task 7 BuildReview screen)
 * should trigger an explicit `refetch()` right before showing it rather than
 * trusting a cache entry that might be a moment stale. */
export function usePlanBuild(
  projectId: ProjectId | undefined,
): UseQueryResult<BuildPlan, CommandError> {
  return useQuery({
    queryKey: keys.planBuild(projectId ?? ''),
    queryFn: () => unwrap(commands.planBuild(projectId as ProjectId)),
    enabled: projectId !== undefined,
  });
}

// ---------------------------------------------------------------------
// Mutation hooks: project lifecycle
// ---------------------------------------------------------------------

function useUnwrapMutation<TVariables, TData>(
  mutationFn: (variables: TVariables) => Promise<TData>,
  invalidate: (data: TData, variables: TVariables, queryClient: QueryClient) => void,
  callbacks?: MutationCallbacks<TData>,
): UseMutationResult<TData, CommandError, TVariables> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn,
    onSuccess: (data, variables) => invalidate(data, variables, queryClient),
    onSettled: (data, error) => {
      callbacks?.onDone?.((error as CommandError | undefined) ?? null, data);
    },
  });
}

/** A project mutation invalidates the full-project list (any status filter
 * variant), the specific project (once it has an id), and the dashboard
 * summary (`active_project_count`). */
function invalidateProject(projectId: ProjectId, queryClient: QueryClient): void {
  queryClient.invalidateQueries({ queryKey: keys.project(projectId) });
  queryClient.invalidateQueries({ queryKey: keys.allProjectsFull });
  queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
}

export function useCreateProjectFull(callbacks?: MutationCallbacks<ProjectRecord>) {
  return useUnwrapMutation<ProjectDraft, ProjectRecord>(
    (draft) => unwrap(commands.createProjectFull(draft)),
    (data, _variables, queryClient) => {
      invalidateProject(data.id, queryClient);
    },
    callbacks,
  );
}

/** Unlike the other project-lifecycle mutations, this one can change
 * `build_quantity` — which the BOM's `total_required`/`missing` columns are
 * computed server-side from (see `ProjectDetail.tsx`'s doc comment) — so on
 * top of the usual `invalidateProject`, it also invalidates `keys.bom` for
 * this project. That invalidation lives here rather than inside
 * `invalidateProject` itself because the other callers of that helper
 * (create/set-status/duplicate/archive) don't change build_quantity and
 * shouldn't pay for a BOM refetch. */
export function useUpdateProject(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<ProjectRecord, null>(
    (record) => unwrap(commands.updateProject(record)),
    (_data, variables, queryClient) => {
      invalidateProject(variables.id, queryClient);
      invalidateBom(variables.id, queryClient);
    },
    callbacks,
  );
}

export interface SetProjectStatusVariables {
  id: ProjectId;
  status: ProjectStatus;
}

export function useSetProjectStatus(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetProjectStatusVariables, null>(
    ({ id, status }) => unwrap(commands.setProjectStatus(id, status)),
    (_data, variables, queryClient) => {
      invalidateProject(variables.id, queryClient);
    },
    callbacks,
  );
}

export interface DuplicateProjectVariables {
  id: ProjectId;
  newName: string;
}

/** Copies a project's fields and its entire BOM structure (new ids, status
 * reset to `planned`) — no ledger rows or stock. Invalidates the project
 * list/dashboard the same as any other project mutation; the duplicate's own
 * `keys.bom`/`keys.project` entries don't need invalidating since they can't
 * already be cached (the id is new). */
export function useDuplicateProject(callbacks?: MutationCallbacks<ProjectRecord>) {
  return useUnwrapMutation<DuplicateProjectVariables, ProjectRecord>(
    ({ id, newName }) => unwrap(commands.duplicateProject(id, newName)),
    (data, _variables, queryClient) => {
      invalidateProject(data.id, queryClient);
    },
    callbacks,
  );
}

export function useArchiveProject(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<ProjectId, null>(
    (id) => unwrap(commands.archiveProject(id)),
    (_data, id, queryClient) => {
      invalidateProject(id, queryClient);
    },
    callbacks,
  );
}

// ---------------------------------------------------------------------
// Mutation hooks: BOM editing
// ---------------------------------------------------------------------

/** A BOM-editing mutation invalidates that project's BOM listing and its
 * (now-stale) build plan — never the project list/dashboard, since editing
 * BOM lines doesn't change anything either of those surface. */
function invalidateBom(projectId: ProjectId, queryClient: QueryClient): void {
  queryClient.invalidateQueries({ queryKey: keys.bom(projectId) });
  queryClient.invalidateQueries({ queryKey: keys.planBuild(projectId) });
}

export interface AddBomItemVariables {
  projectId: ProjectId;
  draft: BomItemDraft;
}

export function useAddBomItem(callbacks?: MutationCallbacks<BomItemRecord>) {
  return useUnwrapMutation<AddBomItemVariables, BomItemRecord>(
    ({ projectId, draft }) => unwrap(commands.addBomItem(projectId, draft)),
    (_data, variables, queryClient) => {
      invalidateBom(variables.projectId, queryClient);
    },
    callbacks,
  );
}

export interface UpdateBomItemVariables {
  /** Not sent to the command (only `id`/`draft` are) — carried so the
   * mutation knows which project's `keys.bom`/`keys.planBuild` to
   * invalidate, since `update_bom_item`'s response doesn't require the
   * caller to already have the project id in scope. */
  projectId: ProjectId;
  id: BomItemId;
  draft: BomItemDraft;
}

export function useUpdateBomItem(callbacks?: MutationCallbacks<BomItemRecord>) {
  return useUnwrapMutation<UpdateBomItemVariables, BomItemRecord>(
    ({ id, draft }) => unwrap(commands.updateBomItem(id, draft)),
    (_data, variables, queryClient) => {
      invalidateBom(variables.projectId, queryClient);
    },
    callbacks,
  );
}

export interface RemoveBomItemVariables {
  projectId: ProjectId;
  id: BomItemId;
}

export function useRemoveBomItem(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<RemoveBomItemVariables, null>(
    ({ id }) => unwrap(commands.removeBomItem(id)),
    (_data, variables, queryClient) => {
      invalidateBom(variables.projectId, queryClient);
    },
    callbacks,
  );
}

export interface SetBomSubstitutesVariables {
  projectId: ProjectId;
  bomItemId: BomItemId;
  partIds: PartId[];
}

export function useSetBomSubstitutes(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetBomSubstitutesVariables, null>(
    ({ bomItemId, partIds }) => unwrap(commands.setBomSubstitutes(bomItemId, partIds)),
    (_data, variables, queryClient) => {
      invalidateBom(variables.projectId, queryClient);
    },
    callbacks,
  );
}

export interface ImportBomVariables {
  projectId: ProjectId;
  rows: BomItemDraft[];
}

/** Bulk `add_bom_item` (`import_bom` — silently skips rows whose part
 * doesn't exist or that collide with an existing BOM line); same
 * invalidation as a single `useAddBomItem`. */
export function useImportBom(callbacks?: MutationCallbacks<BomItemRecord[]>) {
  return useUnwrapMutation<ImportBomVariables, BomItemRecord[]>(
    ({ projectId, rows }) => unwrap(commands.importBom(projectId, rows)),
    (_data, variables, queryClient) => {
      invalidateBom(variables.projectId, queryClient);
    },
    callbacks,
  );
}

// ---------------------------------------------------------------------
// Mutation hooks: reserve / release / build (ledger-mutating)
// ---------------------------------------------------------------------

/** Reserve/release/build/checkout all move real stock through the ledger
 * (via `apply_group`/`apply`), same as `useApplyLedgerOp`/`useReverseGroup`
 * in `inventory.ts` — so on top of the BOM/plan invalidation, they need the
 * same broad stock/search/dashboard/history invalidation those hooks do,
 * plus the affected parts' individual `keys.stock`/`keys.transactions`
 * entries. A build can also auto-activate a `planned` project (see
 * `build_from_bom`'s doc comment in `build.rs`), so the project itself and
 * the full-project list are invalidated too — reserve/release never change
 * status, but invalidating them unconditionally here is cheap and keeps this
 * one helper correct for all three callers instead of hand-tuning each. */
function invalidateAfterLedgerGroup(
  group: GroupRecord,
  projectId: ProjectId,
  queryClient: QueryClient,
): void {
  const partIds = new Set(group.transactions.map((txn) => txn.part_id));
  for (const partId of partIds) {
    queryClient.invalidateQueries({ queryKey: keys.stock(partId) });
    queryClient.invalidateQueries({ queryKey: keys.transactions(partId) });
  }
  invalidateBom(projectId, queryClient);
  invalidateProject(projectId, queryClient);
  queryClient.invalidateQueries({ queryKey: keys.allParts });
  queryClient.invalidateQueries({ queryKey: keys.allSearch });
  queryClient.invalidateQueries({ queryKey: keys.allRecentTransactions });
  queryClient.invalidateQueries({ queryKey: keys.allHistory });
}

/** Reserves `min(needed, available)` for every required BOM line as one
 * atomic group (`reserve_bom`) — partial reservation is the documented
 * happy path, never a failure. */
export function useReserveBom(callbacks?: MutationCallbacks<GroupRecord>) {
  return useUnwrapMutation<ProjectId, GroupRecord>(
    (projectId) => unwrap(commands.reserveBom(projectId)),
    (data, projectId, queryClient) => {
      invalidateAfterLedgerGroup(data, projectId, queryClient);
    },
    callbacks,
  );
}

/** Releases every reservation currently held for the project
 * (`release_bom_reservations`), as one atomic group. */
export function useReleaseBom(callbacks?: MutationCallbacks<GroupRecord>) {
  return useUnwrapMutation<ProjectId, GroupRecord>(
    (projectId) => unwrap(commands.releaseBomReservations(projectId)),
    (data, projectId, queryClient) => {
      invalidateAfterLedgerGroup(data, projectId, queryClient);
    },
    callbacks,
  );
}

export interface BuildFromBomVariables {
  projectId: ProjectId;
  /** BOM lines the caller approved drawing from free available stock,
   * beyond what's already reserved — see `plan_build`'s `BuildPlanLine.
   * available_needed` and `build_from_bom`'s doc comment in `build.rs`. */
  approvedAvailableLines: BomItemId[];
}

/** Executes `plan_build`, filtered by `approvedAvailableLines`, as one
 * atomic group (`build_from_bom`): consumes every line's current
 * reservation, consumes approved available-draw lines, checks out reusable
 * parts, and auto-activates a `planned` project. All-or-nothing — an
 * approved line that turns out to be short fails the whole group. */
export function useBuildFromBom(callbacks?: MutationCallbacks<GroupRecord>) {
  return useUnwrapMutation<BuildFromBomVariables, GroupRecord>(
    ({ projectId, approvedAvailableLines }) =>
      unwrap(commands.buildFromBom(projectId, approvedAvailableLines)),
    (data, variables, queryClient) => {
      invalidateAfterLedgerGroup(data, variables.projectId, queryClient);
    },
    callbacks,
  );
}

export interface AssociateCheckoutVariables {
  projectId: ProjectId;
  partId: PartId;
  quantity: Quantity;
}

/** Ad-hoc reusable-item checkout to a project, not tied to a BOM line
 * (`associate_checkout` — a single `CheckOut` ledger op, not a group).
 * Invalidates the same surface a single-op ledger mutation does
 * (`useApplyLedgerOp` in `inventory.ts`), plus this project's BOM/plan
 * (a checked-out reusable part can be a BOM line too). */
export function useAssociateCheckout(callbacks?: MutationCallbacks<TransactionRecord>) {
  return useUnwrapMutation<AssociateCheckoutVariables, TransactionRecord>(
    ({ projectId, partId, quantity }) =>
      unwrap(commands.associateCheckout(projectId, partId, quantity)),
    (data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.stock(data.part_id) });
      queryClient.invalidateQueries({ queryKey: keys.transactions(data.part_id) });
      invalidateBom(variables.projectId, queryClient);
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
      queryClient.invalidateQueries({ queryKey: keys.allRecentTransactions });
      queryClient.invalidateQueries({ queryKey: keys.allHistory });
    },
    callbacks,
  );
}
