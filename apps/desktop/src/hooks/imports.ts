/**
 * The TanStack Query layer over the Phase 5b import command surface
 * (`parse_and_store_import`, `list_imports`, `list_import_lines`,
 * `get_import_review`, `commit_import`, `reverse_import` — spec §10's
 * Upload -> Extract -> Match -> Review -> Confirm pipeline). Same shape as
 * `hooks/inventory.ts`/`hooks/projects.ts`: a query hook per read, a
 * mutation hook per write, one `keys` object (extending `inventory.ts`'s,
 * the same pattern `hooks/projects.ts` uses), `unwrap` around every command
 * call.
 *
 * Only `commit_import`/`reverse_import` touch inventory — both resolve to a
 * `GroupRecord` (the same shape `reserve_bom`/`build_from_bom`/
 * `reverse_group` return), so their invalidation mirrors `useReverseGroup`'s
 * broad ledger surface (per-part stock/transactions, parts, search,
 * dashboard, recent activity, history) in `hooks/inventory.ts`, on top of
 * the imports-specific keys. `parse_and_store_import`/the read hooks never
 * create a part or a receive — see `import_commit.rs`'s module doc comment.
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
  CommandError,
  GroupRecord,
  ImportId,
  ImportLineId,
  ImportLineRecord,
  ImportRecord,
  ImportReview,
  LineDecision,
} from '../bindings.gen';
import { commands, unwrap } from '../lib/commands';
import { keys as inventoryKeys, type MutationCallbacks } from './inventory';

/** Extends `inventory.ts`'s `keys` with the import entries this file's hooks
 * use — same "spread, don't duplicate" pattern `hooks/projects.ts` follows. */
export const keys = {
  ...inventoryKeys,
  allImports: ['imports'] as const,
  importReview: (importId: ImportId) => ['importReview', importId] as const,
  importLines: (importId: ImportId) => ['importLines', importId] as const,
};

// ---------------------------------------------------------------------
// Query hooks
// ---------------------------------------------------------------------

/** Every persisted import, newest first (`list_imports`) — the Imports
 * screen's list source. */
export function useImports(): UseQueryResult<ImportRecord[], CommandError> {
  return useQuery({
    queryKey: keys.allImports,
    queryFn: () => unwrap(commands.listImports()),
  });
}

/** The full Match -> Review plan for one persisted import (`get_import_
 * review`): every line with its match candidates, default proposed action,
 * and shipped-not-ordered receive quantity, plus any duplicate-import
 * warning. Purely derived server-side — never a mutation. Disabled without
 * an `importId`, the same "nothing selected yet" pattern `usePart`/`useBom`
 * use. */
export function useImportReview(
  importId: ImportId | undefined,
): UseQueryResult<ImportReview, CommandError> {
  return useQuery({
    queryKey: keys.importReview(importId ?? ''),
    queryFn: () => unwrap(commands.getImportReview(importId as ImportId)),
    enabled: importId !== undefined,
  });
}

/** One import's raw persisted lines (`list_import_lines`), independent of
 * the derived review — useful wherever a screen wants the source rows
 * without the match/proposed-action overlay `useImportReview` adds. */
export function useImportLines(
  importId: ImportId | undefined,
): UseQueryResult<ImportLineRecord[], CommandError> {
  return useQuery({
    queryKey: keys.importLines(importId ?? ''),
    queryFn: () => unwrap(commands.listImportLines(importId as ImportId)),
    enabled: importId !== undefined,
  });
}

// ---------------------------------------------------------------------
// Mutation hooks
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

export interface ParseImportVariables {
  /** The file's raw bytes as a JSON number array — how `Vec<u8>` crosses the
   * IPC boundary (see `parse_and_store_import` in `commands.rs`, the same
   * convention `add_attachment` uses). */
  bytes: number[];
  filename: string;
}

/** Upload -> Extract (spec §10): detects the file's format and parses it
 * with the matching DigiKey parser, then persists the result
 * (`parse_and_store_import`). Purely additive against inventory — nothing
 * here creates a part or a receive; that's `useCommitImport` below.
 * Invalidates the import list so the freshly parsed import shows
 * immediately. */
export function useParseImport(callbacks?: MutationCallbacks<ImportRecord>) {
  return useUnwrapMutation<ParseImportVariables, ImportRecord>(
    ({ bytes, filename }) => unwrap(commands.parseAndStoreImport(bytes, filename)),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.allImports });
    },
    callbacks,
  );
}

/** Every stock/search/dashboard/history query a `GroupRecord`-returning
 * import mutation can affect — the same broad surface `useReverseGroup`
 * (`hooks/inventory.ts`) invalidates for any ledger group, minus the
 * project-specific keys (an import commit has no project). Shared by
 * `useCommitImport`/`useReverseImport` since both apply/undo the same kind
 * of receive group.
 *
 * Also invalidates `keys.bins` (a gap `useCommitImport` had until Task 4): a
 * `create_new` decision's `PartDraft.bin_label` creates a brand-new part
 * with a bin, exactly the same "a new part with a bin_label adds it to the
 * bin tile grid's counts" case `useCreatePart` (`hooks/inventory.ts`) already
 * documents and invalidates for — a stale `keys.bins` cache would keep
 * showing the pre-commit count for that bin (or hide a newly-occupied one)
 * until something unrelated happened to invalidate it. Reversal never
 * changes a part's `bin_label` (only stock moves back to zero), so this is
 * over-invalidation on that path, not under — the safe direction.
 *
 * Also invalidates `keys.part`/`keys.variants` for every part the group's
 * transactions touch (final review Finding 1): `LineDecision.add_as_variant`
 * adds a new variant + listing to an EXISTING part, so without this a part
 * detail screen already open/cached for that part would silently miss the
 * new variant after commit. Deriving the affected part ids from the
 * `GroupRecord`'s own `transactions` (rather than from each hook's
 * `decisions` variable) is what makes ONE derivation cover `useCommitImport`
 * (whose variables carry per-line decisions) AND `useReverseImport` (whose
 * variables are just `{ importId, note }`, with no part ids in them) — and
 * it also covers `create_new`'s freshly-created part for free, the same way
 * `keys.stock`/`keys.transactions` above already do. */
function invalidateAfterImportGroup(group: GroupRecord, queryClient: QueryClient): void {
  const partIds = new Set(group.transactions.map((txn) => txn.part_id));
  for (const partId of partIds) {
    queryClient.invalidateQueries({ queryKey: keys.stock(partId) });
    queryClient.invalidateQueries({ queryKey: keys.transactions(partId) });
    queryClient.invalidateQueries({ queryKey: keys.part(partId) });
    queryClient.invalidateQueries({ queryKey: keys.variants(partId) });
  }
  queryClient.invalidateQueries({ queryKey: keys.allParts });
  queryClient.invalidateQueries({ queryKey: keys.allSearch });
  queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
  queryClient.invalidateQueries({ queryKey: keys.allRecentTransactions });
  queryClient.invalidateQueries({ queryKey: keys.allHistory });
  queryClient.invalidateQueries({ queryKey: keys.bins });
}

export interface CommitImportVariables {
  importId: ImportId;
  decisions: [ImportLineId, LineDecision][];
}

/** Confirm (spec §10): applies every resolved per-line decision as ONE
 * atomic group (`commit_import`) — new parts/variants/listings, each line's
 * shipped-quantity receive, price history, and remembered SKU/MPN aliases.
 * The only mutation in the Match -> Review -> Commit pipeline; a failure
 * rolls back the entire commit server-side and the import stays `parsed`.
 * Invalidates the import list, this import's review/lines (status flips to
 * `committed`), and the full ledger surface a receive touches. */
export function useCommitImport(callbacks?: MutationCallbacks<GroupRecord>) {
  return useUnwrapMutation<CommitImportVariables, GroupRecord>(
    ({ importId, decisions }) => unwrap(commands.commitImport(importId, decisions)),
    (data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.allImports });
      queryClient.invalidateQueries({ queryKey: keys.importReview(variables.importId) });
      queryClient.invalidateQueries({ queryKey: keys.importLines(variables.importId) });
      invalidateAfterImportGroup(data, queryClient);
    },
    callbacks,
  );
}

export interface ReverseImportVariables {
  importId: ImportId;
  note: string;
}

/** Undoes a committed import's receive group and flips its status back to
 * `reversed` (`reverse_import`). Parts the original commit created are NOT
 * deleted — they simply return to zero stock (history is never deleted; see
 * `import_commit.rs`'s module doc comment). Same invalidation shape as
 * `useCommitImport`. */
export function useReverseImport(callbacks?: MutationCallbacks<GroupRecord>) {
  return useUnwrapMutation<ReverseImportVariables, GroupRecord>(
    ({ importId, note }) => unwrap(commands.reverseImport(importId, note)),
    (data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.allImports });
      queryClient.invalidateQueries({ queryKey: keys.importReview(variables.importId) });
      queryClient.invalidateQueries({ queryKey: keys.importLines(variables.importId) });
      invalidateAfterImportGroup(data, queryClient);
    },
    callbacks,
  );
}
