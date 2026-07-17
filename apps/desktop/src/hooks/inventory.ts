/**
 * The TanStack Query layer over the typed `commands.*` surface
 * (`lib/commands.ts`). Every screen reads/writes inventory data through
 * these hooks — never through `commands` or `invoke` directly — so query
 * keys and cache invalidation stay centralized in one place.
 *
 * Query hooks that key off an id accept `id: X | undefined` and disable
 * themselves (`enabled: false`, no command call) when it is `undefined` —
 * the common case of "nothing selected yet" (e.g. the part-detail inspector
 * before a row is chosen) without callers having to special-case the hook
 * call itself, which would violate the rules of hooks.
 */

import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';

import type {
  AttributeDefRow,
  BinSummary,
  CategoryId,
  CommandError,
  DashboardSummary,
  DimensionDraft,
  DimensionRecord,
  GroupId,
  GroupRecord,
  HistoryFilter,
  HistoryPage,
  LedgerOp,
  ListingDraft,
  ListingRecord,
  MatchCandidate,
  MatchResult,
  PartDraft,
  PartId,
  PartRecord,
  ProjectId,
  ProjectRef,
  RecentTxn,
  TransactionId,
  TransactionRecord,
  VariantDraft,
  VariantId,
  VariantRecord,
} from '../bindings.gen';
import { commands, unwrap } from '../lib/commands';

/** Stable, centralized query keys, one builder per entity. Every id-scoped
 * "list" builder (e.g. `parts`) shares its first element with a bare "all"
 * key (e.g. `allParts`) so `invalidateQueries({ queryKey: keys.allParts })`
 * fuzzy-matches every variant of that query (TanStack Query's default
 * partial-key matching), regardless of the other key segments. */
export const keys = {
  allParts: ['parts'] as const,
  parts: (includeArchived: boolean) => ['parts', includeArchived] as const,
  part: (id: PartId) => ['part', id] as const,
  stock: (id: PartId) => ['stock', id] as const,
  allSearch: ['search'] as const,
  search: (query: string) => ['search', query] as const,
  setting: (key: string) => ['setting', key] as const,
  transactions: (id: PartId) => ['transactions', id] as const,
  categories: () => ['categories'] as const,
  categoryAttributes: (categoryId: CategoryId) => ['categoryAttributes', categoryId] as const,
  categoryAttributeDefs: (categoryId: CategoryId) => ['categoryAttributeDefs', categoryId] as const,
  previewUnitValue: (unitKind: string, raw: string) => ['previewUnitValue', unitKind, raw] as const,
  // `candidate` is serialized into the key (rather than kept as an object)
  // so two structurally-equal candidates — the normal case when a debounced
  // rebuild produces the same fields the previous one did — share one cache
  // entry instead of re-fetching. `buildMatchCandidate` (features/part) is
  // the one place that constructs a `MatchCandidate`, and always emits its
  // fields in the same order, so this is a stable, deterministic key.
  findMatches: (candidate: MatchCandidate | null) =>
    ['findMatches', candidate ? JSON.stringify(candidate) : null] as const,
  variants: (partId: PartId) => ['variants', partId] as const,
  supplierListings: (variantId: VariantId) => ['supplierListings', variantId] as const,
  dimensions: (partId: PartId) => ['dimensions', partId] as const,
  attributes: (partId: PartId) => ['attributes', partId] as const,
  tags: (partId: PartId) => ['tags', partId] as const,
  dashboardSummary: ['dashboardSummary'] as const,
  allRecentTransactions: ['recentTransactions'] as const,
  recentTransactions: (limit: number) => ['recentTransactions', limit] as const,
  projects: ['projects'] as const,
  bins: ['bins'] as const,
  // `filter` is serialized into the key (rather than kept as an object) so
  // two structurally-equal filters — the common case when a filter-bar
  // control re-renders with the same value — share one cache entry instead
  // of re-fetching; see `findMatches`'s key above for the same pattern.
  allHistory: ['history'] as const,
  history: (filter: HistoryFilter) => ['history', JSON.stringify(filter)] as const,
  group: (id: GroupId) => ['group', id] as const,
};

// ---------------------------------------------------------------------
// Query hooks
// ---------------------------------------------------------------------

/** `enabled` (default `true`) lets a caller defer the fetch until it's
 * actually needed — e.g. `GroupRow`'s reverse-group confirmation only wants
 * the full parts list (to resolve part names/units for group members not on
 * the current History page) once the dialog is actually open, not on every
 * render of a group header. */
export function useParts(
  includeArchived: boolean,
  enabled = true,
): UseQueryResult<PartRecord[], CommandError> {
  return useQuery({
    queryKey: keys.parts(includeArchived),
    queryFn: () => unwrap(commands.listParts(includeArchived)),
    enabled,
  });
}

export function usePart(id: PartId | undefined) {
  return useQuery({
    queryKey: keys.part(id ?? ''),
    queryFn: () => unwrap(commands.getPart(id as PartId)),
    enabled: id !== undefined,
  });
}

export function useStock(id: PartId | undefined) {
  return useQuery({
    queryKey: keys.stock(id ?? ''),
    queryFn: () => unwrap(commands.getStock(id as PartId)),
    enabled: id !== undefined,
  });
}

/** Disabled for a blank/whitespace-only query — the command surface expects
 * a real search term, and an empty term would otherwise re-fetch on every
 * keystroke of clearing the box. */
export function useSearch(query: string) {
  return useQuery({
    queryKey: keys.search(query),
    queryFn: () => unwrap(commands.search(query)),
    enabled: query.trim().length > 0,
  });
}

/** Same underlying `search` command and cache entry as `useSearch` (shares
 * `keys.search`, so results stay consistent with the Ctrl+K palette for the
 * same query string), but always enabled — including for a blank query,
 * which the backend treats as "every non-archived part" rather than "no
 * free text to narrow by". `useSearch`'s blank-query guard exists so the
 * palette shows nothing until you type; the Inventory browser (Phase 3 Task
 * 4) needs the opposite default — its unfiltered view lists the whole
 * library, not nothing.
 *
 * `placeholderData: keepPreviousData` keeps the previous query's rows (and
 * `isSuccess`/non-`isPending` status) on screen while a new `query` string's
 * result loads — every keystroke mints a new `keys.search(query)` cache key,
 * so without this the table would flip to "no data yet" and unmount on each
 * character. `isPlaceholderData`/`isFetching` distinguish "showing stale
 * rows while a refetch is in flight" from a real, fresh result for callers
 * (e.g. `InventoryTable`) that want to show a subtle pending affordance
 * instead of losing the table entirely. */
export function useInventorySearch(query: string) {
  return useQuery({
    queryKey: keys.search(query),
    queryFn: () => unwrap(commands.search(query)),
    placeholderData: keepPreviousData,
  });
}

/** A setting's raw string value, or `null` if the key has never been set
 * (see `set_setting`/`get_setting` — a tiny key-value store over the
 * `settings` table, used by the Inventory browser's saved search views). */
export function useSetting(key: string) {
  return useQuery({
    queryKey: keys.setting(key),
    queryFn: () => unwrap(commands.getSetting(key)),
  });
}

export function useTransactions(id: PartId | undefined) {
  return useQuery({
    queryKey: keys.transactions(id ?? ''),
    queryFn: () => unwrap(commands.listTransactions(id as PartId)),
    enabled: id !== undefined,
  });
}

export function useCategories() {
  return useQuery({
    queryKey: keys.categories(),
    queryFn: () => unwrap(commands.listCategories()),
  });
}

export function useCategoryAttributes(categoryId: CategoryId | undefined) {
  return useQuery({
    queryKey: keys.categoryAttributes(categoryId ?? ''),
    queryFn: () => unwrap(commands.categoryAttributes(categoryId as CategoryId)),
    enabled: categoryId !== undefined,
  });
}

/** The richer per-attribute shape (`AttributeDefRow`: data_type, unit_kind,
 * identity, choices) the part create/edit form (Phase 3 Task 6) renders one
 * typed field per — see `category_attribute_defs`'s doc comment in
 * `commands.rs` for why this exists alongside the thinner
 * `useCategoryAttributes`. */
export function useCategoryAttributeDefs(
  categoryId: CategoryId | undefined,
): UseQueryResult<AttributeDefRow[], CommandError> {
  return useQuery({
    queryKey: keys.categoryAttributeDefs(categoryId ?? ''),
    queryFn: () => unwrap(commands.categoryAttributeDefs(categoryId as CategoryId)),
    enabled: categoryId !== undefined,
  });
}

/** The part form's `number_unit`/`range` field live-preview: formats `raw`
 * under `unitKind`'s parsing rules into its canonical display form (e.g.
 * `"10k"` -> `"10 kΩ"`) via the stateless `preview_unit_value` command.
 * Disabled for an empty/whitespace `raw` or a `null` `unitKind` (a `text`/
 * `number`/etc. field has none) rather than firing a doomed call; `retry:
 * false` so an invalid in-progress value (the common case while typing,
 * e.g. `"10"` before the unit letter lands) fails fast instead of retrying
 * a call the backend will keep rejecting the same way. Callers debounce the
 * `raw` they pass in — see `AttributeFields.tsx`'s `NumberUnitField`. */
export function usePreviewUnitValue(
  unitKind: string | null,
  raw: string,
): UseQueryResult<string, CommandError> {
  const trimmed = raw.trim();
  return useQuery({
    queryKey: keys.previewUnitValue(unitKind ?? '', trimmed),
    queryFn: () => unwrap(commands.previewUnitValue(unitKind as string, trimmed)),
    enabled: unitKind !== null && trimmed.length > 0,
    retry: false,
  });
}

/** Scores a duplicate-candidate part (built by `buildMatchCandidate`,
 * `features/part/buildMatchCandidate.ts`) against parts already on file —
 * the part form's live `DuplicatePanel`. `candidate: null` (not enough
 * identity signal entered yet, or category not chosen) disables the query
 * entirely rather than firing a call the backend would just return `[]`
 * for. Callers debounce the `candidate` they pass in — see
 * `DuplicatePanel.tsx`. */
export function useFindMatches(
  candidate: MatchCandidate | null,
): UseQueryResult<MatchResult[], CommandError> {
  return useQuery({
    queryKey: keys.findMatches(candidate),
    queryFn: () => unwrap(commands.findMatches(candidate as MatchCandidate)),
    enabled: candidate !== null,
  });
}

export function useVariants(partId: PartId | undefined) {
  return useQuery({
    queryKey: keys.variants(partId ?? ''),
    queryFn: () => unwrap(commands.listVariants(partId as PartId)),
    enabled: partId !== undefined,
  });
}

export function useSupplierListings(variantId: VariantId | undefined) {
  return useQuery({
    queryKey: keys.supplierListings(variantId ?? ''),
    queryFn: () => unwrap(commands.listSupplierListings(variantId as VariantId)),
    enabled: variantId !== undefined,
  });
}

export function useDimensions(partId: PartId | undefined) {
  return useQuery({
    queryKey: keys.dimensions(partId ?? ''),
    queryFn: () => unwrap(commands.listDimensions(partId as PartId)),
    enabled: partId !== undefined,
  });
}

export function useAttributes(partId: PartId | undefined) {
  return useQuery({
    queryKey: keys.attributes(partId ?? ''),
    queryFn: () => unwrap(commands.getAttributes(partId as PartId)),
    enabled: partId !== undefined,
  });
}

export function useTags(partId: PartId | undefined) {
  return useQuery({
    queryKey: keys.tags(partId ?? ''),
    queryFn: () => unwrap(commands.getTags(partId as PartId)),
    enabled: partId !== undefined,
  });
}

/** The dashboard's summary-card counts (Phase 3 Task 3): available/reserved/
 * checked-out unit totals, part count, and the low-stock/metadata-review/
 * unbinned counts, computed server-side in two cheap aggregate queries
 * rather than one client-side query per part. */
export function useDashboardSummary(): UseQueryResult<DashboardSummary, CommandError> {
  return useQuery({
    queryKey: keys.dashboardSummary,
    queryFn: () => unwrap(commands.dashboardSummary()),
  });
}

/** The dashboard's recent-activity feed: the `limit` most recent ledger rows
 * across every part, newest first, each already carrying a `reversible`
 * flag computed server-side. */
export function useRecentTransactions(limit: number): UseQueryResult<RecentTxn[], CommandError> {
  return useQuery({
    queryKey: keys.recentTransactions(limit),
    queryFn: () => unwrap(commands.recentTransactions(limit)),
  });
}

/** The Bin browser's tile grid (Phase 3 Task 8): every distinct `bin_label`
 * among non-archived parts, each with its part count, plus a distinct
 * "Unassigned" row (`bin_label: null`) whenever at least one non-archived
 * part has no bin — computed server-side in one aggregate query rather than
 * grouping `list_parts`/`search` results client-side. */
export function useBins(): UseQueryResult<BinSummary[], CommandError> {
  return useQuery({
    queryKey: keys.bins,
    queryFn: () => unwrap(commands.listBins()),
  });
}

/** Every project (id + name), alphabetical — feeds the Reserve/Check-out/
 * Return project pickers in the Ctrl+K quick-action flows (Phase 3 Task 5).
 * Projects themselves are a Phase 4 stub (`inventory_db::ledger::
 * ProjectRef`); this hook exists now so those flows aren't blocked on that
 * screen shipping first. */
export function useProjects(): UseQueryResult<ProjectRef[], CommandError> {
  return useQuery({
    queryKey: keys.projects,
    queryFn: () => unwrap(commands.listProjects()),
  });
}

/** The History screen's paged, filtered ledger view (Phase 3 Task 9):
 * `filter`'s optional fields narrow (date range/type/part/project/group),
 * `limit`/`offset` page the result, and the backend reports `total` so a
 * pagination control can render against the unpaged count. `keepPreviousData`
 * keeps the prior page/filter's rows on screen while a new filter or page
 * loads — the same "don't flash to empty on every keystroke" rationale
 * `useInventorySearch` documents — so paging/filtering reads as a smooth
 * narrowing rather than a flicker to a loading state on every change. */
export function useHistory(filter: HistoryFilter): UseQueryResult<HistoryPage, CommandError> {
  return useQuery({
    queryKey: keys.history(filter),
    queryFn: () => unwrap(commands.listHistory(filter)),
    placeholderData: keepPreviousData,
  });
}

/** The TRUE, unfiltered, unpaged member list of one transaction group
 * (`get_group`) — the reverse-group confirmation's source of truth
 * (`GroupRow.tsx`). The History screen clusters a group header from whatever
 * members of that group happen to be on the current filtered/paged
 * `list_history` result (`groupHistoryRows.ts`), which can be a strict
 * subset of the group's real membership; confirming a reversal against that
 * partial view would understate what "Reverse group" is actually about to
 * undo (the backend's `reverse_group` always reverses the whole group).
 * `groupId: undefined` disables the query — the same "enable on demand"
 * pattern `usePart`/`useStock` use — so a caller (the confirmation dialog)
 * can defer the fetch until it actually opens rather than firing one for
 * every group header rendered in the list. */
export function useGroup(
  groupId: GroupId | undefined,
): UseQueryResult<GroupRecord | null, CommandError> {
  return useQuery({
    queryKey: keys.group(groupId ?? ''),
    queryFn: () => unwrap(commands.getGroup(groupId as GroupId)),
    enabled: groupId !== undefined,
  });
}

// ---------------------------------------------------------------------
// Mutation hooks
// ---------------------------------------------------------------------

export interface MutationCallbacks<TData> {
  /** Called once the mutation settles, success or failure — the hook point
   * later screens use for toast messaging (e.g. "Received 10" / a
   * `CommandError` message + hint). `error` is `null` on success. */
  onDone?: (error: CommandError | null, data: TData | undefined) => void;
}

/** Shared plumbing for every mutation hook below: run `mutationFn` through
 * `unwrap` already applied by the caller, invalidate whatever query keys
 * `invalidate` decides based on the result, and always report the outcome
 * through `onDone`. Centralizing this keeps each hook to just "what command,
 * what shape, what keys" instead of repeating the settle/report wiring. */
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

export function useApplyLedgerOp(callbacks?: MutationCallbacks<TransactionRecord>) {
  return useUnwrapMutation<LedgerOp, TransactionRecord>(
    (op) => unwrap(commands.applyLedgerOp(op)),
    (data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.stock(data.part_id) });
      queryClient.invalidateQueries({ queryKey: keys.transactions(data.part_id) });
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      // A ledger op moves stock between states, which the dashboard's
      // summary totals and recent-activity feed both surface.
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
      queryClient.invalidateQueries({ queryKey: keys.allRecentTransactions });
      // ...and it's a new row in the History screen's ledger view.
      queryClient.invalidateQueries({ queryKey: keys.allHistory });
    },
    callbacks,
  );
}

export function useCreatePart(callbacks?: MutationCallbacks<PartRecord>) {
  return useUnwrapMutation<PartDraft, PartRecord>(
    (draft) => unwrap(commands.createPart(draft)),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      // A new part changes the dashboard's part count and (until it's binned
      // and its metadata reviewed) its unbinned/metadata-incomplete counts.
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
    },
    callbacks,
  );
}

export function useUpdatePart(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<PartRecord, null>(
    (record) => unwrap(commands.updatePart(record)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.part(variables.id) });
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      // A part edit can change its bin_label, low_stock_threshold, or
      // metadata_complete — every one of them a dashboard summary input.
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
      // A part edit is also how a part's bin is assigned/reassigned/cleared
      // (the Bin browser's `AssignBinDialog`, Phase 3 Task 8, has no
      // dedicated command — it loads the full record and saves it back
      // through this same mutation) — the bin tile grid's per-label counts
      // depend on `bin_label` too, so a stale `keys.bins` cache would keep
      // showing the pre-move counts on both the old and new bin's tiles.
      queryClient.invalidateQueries({ queryKey: keys.bins });
    },
    callbacks,
  );
}

export interface SetArchivedVariables {
  partId: PartId;
  archived: boolean;
}

export function useSetArchived(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetArchivedVariables, null>(
    ({ partId, archived }) => unwrap(commands.setPartArchived(partId, archived)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.part(variables.partId) });
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      // The backend excludes archived parts from search by default (and
      // `is:archived` only returns them), so archiving/restoring a part
      // flips which side of that filter it's on — stale cached search
      // results (including Ctrl+K) would otherwise keep showing/hiding it.
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      // The dashboard summary excludes archived parts from every count, so
      // archiving/restoring moves this part in or out of all of them.
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
      // History rows join in each part's current archived state
      // (`HistoryRow.part_archived`), which this flips.
      queryClient.invalidateQueries({ queryKey: keys.allHistory });
    },
    callbacks,
  );
}

export interface SetAttributeVariables {
  partId: PartId;
  key: string;
  raw: string;
}

export function useSetAttribute(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetAttributeVariables, null>(
    ({ partId, key, raw }) => unwrap(commands.setAttribute(partId, key, raw)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.attributes(variables.partId) });
      // Setting an identity attribute can flip metadata_complete on the part
      // record and change how it reads in the parts list.
      queryClient.invalidateQueries({ queryKey: keys.part(variables.partId) });
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      // The backend re-indexes the part's search text (key + value) on every
      // attribute write, so cached search/filter results (e.g. an
      // attribute-keyed filter) can go stale here too.
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
    },
    callbacks,
  );
}

export interface AddDimensionVariables {
  partId: PartId;
  draft: DimensionDraft;
}

export function useAddDimension(callbacks?: MutationCallbacks<DimensionRecord>) {
  return useUnwrapMutation<AddDimensionVariables, DimensionRecord>(
    ({ partId, draft }) => unwrap(commands.addDimension(partId, draft)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.dimensions(variables.partId) });
      // Dimension names are indexed into search_text, so cached search results
      // can go stale after adding a dimension.
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
    },
    callbacks,
  );
}

export interface AddVariantVariables {
  partId: PartId;
  draft: VariantDraft;
}

export function useAddVariant(callbacks?: MutationCallbacks<VariantRecord>) {
  return useUnwrapMutation<AddVariantVariables, VariantRecord>(
    ({ partId, draft }) => unwrap(commands.addVariant(partId, draft)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.variants(variables.partId) });
      // Variant manufacturer/MPN are indexed into search_text, so cached search
      // results can go stale after adding a variant.
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
    },
    callbacks,
  );
}

export interface SetPreferredVariantVariables {
  partId: PartId;
  variantId: VariantId;
}

export function useSetPreferredVariant(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetPreferredVariantVariables, null>(
    ({ partId, variantId }) => unwrap(commands.setPreferredVariant(partId, variantId)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.variants(variables.partId) });
    },
    callbacks,
  );
}

export interface AddSupplierListingVariables {
  variantId: VariantId;
  draft: ListingDraft;
}

export function useAddSupplierListing(callbacks?: MutationCallbacks<ListingRecord>) {
  return useUnwrapMutation<AddSupplierListingVariables, ListingRecord>(
    ({ variantId, draft }) => unwrap(commands.addSupplierListing(variantId, draft)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.supplierListings(variables.variantId) });
      // Supplier SKU is indexed into search_text, so cached search results
      // can go stale after adding a listing.
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
    },
    callbacks,
  );
}

export interface SetTagsVariables {
  partId: PartId;
  tags: string[];
}

export function useSetTags(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetTagsVariables, null>(
    ({ partId, tags }) => unwrap(commands.setTags(partId, tags)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.tags(variables.partId) });
      // Tags are part of the backend's search text, so a stale cached search
      // (or Ctrl+K result) can keep showing/hiding this part by a tag term
      // it no longer has.
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
    },
    callbacks,
  );
}

export interface ReverseTransactionVariables {
  txnId: TransactionId;
  note: string;
}

export function useReverseTransaction(callbacks?: MutationCallbacks<TransactionRecord>) {
  return useUnwrapMutation<ReverseTransactionVariables, TransactionRecord>(
    ({ txnId, note }) => unwrap(commands.reverseTransaction(txnId, note)),
    (data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.stock(data.part_id) });
      queryClient.invalidateQueries({ queryKey: keys.transactions(data.part_id) });
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      // A reversal moves stock back and adds a new ledger row, both of
      // which the dashboard summary/recent-activity feed reflect.
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
      queryClient.invalidateQueries({ queryKey: keys.allRecentTransactions });
      queryClient.invalidateQueries({ queryKey: keys.allHistory });
    },
    callbacks,
  );
}

export interface ReverseGroupVariables {
  groupId: GroupId;
  note: string;
}

export function useReverseGroup(callbacks?: MutationCallbacks<GroupRecord>) {
  return useUnwrapMutation<ReverseGroupVariables, GroupRecord>(
    ({ groupId, note }) => unwrap(commands.reverseGroup(groupId, note)),
    (data, _variables, queryClient) => {
      const partIds = new Set(data.transactions.map((txn) => txn.part_id));
      for (const partId of partIds) {
        queryClient.invalidateQueries({ queryKey: keys.stock(partId) });
        queryClient.invalidateQueries({ queryKey: keys.transactions(partId) });
      }
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
      queryClient.invalidateQueries({ queryKey: keys.allRecentTransactions });
      queryClient.invalidateQueries({ queryKey: keys.allHistory });
    },
    callbacks,
  );
}

export interface RecordEquivalenceVariables {
  a: PartId;
  b: PartId;
  decision: string;
  note: string;
}

export function useRecordEquivalence(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<RecordEquivalenceVariables, null>(
    ({ a, b, decision, note }) => unwrap(commands.recordEquivalence(a, b, decision, note)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.part(variables.a) });
      queryClient.invalidateQueries({ queryKey: keys.part(variables.b) });
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      // No allSearch here: recording an equivalence decision only writes
      // equivalence_decisions — it never touches search_text, part_stock,
      // or parts.archived, so cached search results can't have gone stale.
    },
    callbacks,
  );
}

export interface SetSettingVariables {
  key: string;
  value: string;
}

export function useSetSetting(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetSettingVariables, null>(
    ({ key, value }) => unwrap(commands.setSetting(key, value)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.setting(variables.key) });
    },
    callbacks,
  );
}

export interface RenameBinVariables {
  oldLabel: string;
  newLabel: string;
}

/** Bulk-moves every non-archived part in `oldLabel` to `newLabel` in one
 * atomic backend transaction (`rename_bin`) — not N sequential
 * `useUpdatePart` calls, which could leave a bin half-renamed if one of them
 * failed partway through. Resolves to how many parts moved. Invalidates
 * `keys.bins` (the tile grid's counts/labels changed), the `parts`/`search`
 * buckets (`bin_label` feeds both `SearchHit.bin_label` and search_text),
 * the dashboard summary, and every cached `keys.part(id)` entry (`['part']`
 * fuzzy-matches every part id variant, same partial-key trick `keys.allParts`
 * relies on) since any of the moved parts' detail views could be cached. */
export function useRenameBin(callbacks?: MutationCallbacks<number>) {
  return useUnwrapMutation<RenameBinVariables, number>(
    ({ oldLabel, newLabel }) => unwrap(commands.renameBin(oldLabel, newLabel)),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.bins });
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
      queryClient.invalidateQueries({ queryKey: ['part'] });
    },
    callbacks,
  );
}

/** Creates a project inline from a name string and returns its fresh
 * `ProjectId` — the "Create new project…" affordance in a Reserve/Check-out/
 * Return project picker (Phase 3 Task 5) so a project never has to be
 * created on a separate screen first. Invalidates `keys.projects` so the
 * next picker render (or this one, once its own `useProjects()` refetches)
 * includes it. */
export function useCreateProject(callbacks?: MutationCallbacks<ProjectId>) {
  return useUnwrapMutation<string, ProjectId>(
    (name) => unwrap(commands.createProject(name)),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.projects });
    },
    callbacks,
  );
}

export interface AddAliasVariables {
  kind: string;
  value: string;
  partId: PartId;
  source: string;
}

export function useAddAlias(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<AddAliasVariables, null>(
    ({ kind, value, partId, source }) => unwrap(commands.addAlias(kind, value, partId, source)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.part(variables.partId) });
      // No allSearch here: aliases (supplier SKU / MPN seen on import) live
      // in part_aliases, which the backend never folds into search_text —
      // they aren't searchable content, so there's nothing to invalidate.
    },
    callbacks,
  );
}
