/**
 * The TanStack Query layer over the Phase 5c enrichment command surface
 * (`enrich_part_preview`, `apply_enrichment`, `get_digikey_status`,
 * `set_digikey_environment` — `inventory_db::enrichment` + the DigiKey
 * sandbox/production toggle). Same shape as `hooks/inventory.ts`/
 * `hooks/projects.ts`/`hooks/imports.ts`: a query hook per read, a mutation
 * hook per write, one `keys` object (extending `inventory.ts`'s, the same
 * "spread, don't duplicate" pattern `hooks/projects.ts`/`hooks/imports.ts`
 * use), `unwrap` around every command call.
 *
 * `useEnrichmentPreview` is deliberately LAZY (`enabled` defaults to
 * `false`) — enrichment does real network I/O (the DigiKey provider), so it
 * must never auto-fire just because a part-detail screen mounted; a caller
 * only flips `enabled: true` once the user actually opens the enrichment
 * diff.
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
  AppliedField,
  CommandError,
  DigiKeyStatus,
  DigiKeyTestResult,
  EnrichmentDiff,
  PartId,
} from '../bindings.gen';
import { commands, unwrap } from '../lib/commands';
import { keys as inventoryKeys, type MutationCallbacks } from './inventory';

/** Extends `inventory.ts`'s `keys` with the enrichment entries this file's
 * hooks use — same "spread, don't duplicate" pattern `hooks/projects.ts`/
 * `hooks/imports.ts` follow. */
export const keys = {
  ...inventoryKeys,
  enrichmentPreview: (partId: PartId) => ['enrichmentPreview', partId] as const,
  digikeyStatus: ['digikeyStatus'] as const,
};

// ---------------------------------------------------------------------
// Query hooks
// ---------------------------------------------------------------------

/** The provenance-aware enrichment diff for one part (`enrich_part_preview`):
 * runs the provider chain (DigiKey, if configured, then the always-available
 * offline description parser) and compares the resulting candidates against
 * the part's current values — writes nothing.
 *
 * LAZY by design: `enabled` defaults to `false`, so this never fires on its
 * own just because a component using it rendered (e.g. a part-detail screen
 * mounting) — enrichment does real network I/O via the DigiKey provider, so
 * a caller must explicitly pass `enabled: true` once the user opens the
 * diff screen (and `partId` is known), the same on-demand pattern
 * `useAttachmentBytes` uses for a potentially large fetch. */
export function useEnrichmentPreview(
  partId: PartId | undefined,
  options?: { enabled?: boolean },
): UseQueryResult<EnrichmentDiff, CommandError> {
  return useQuery({
    queryKey: keys.enrichmentPreview(partId ?? ''),
    queryFn: () => unwrap(commands.enrichPartPreview(partId as PartId)),
    enabled: partId !== undefined && (options?.enabled ?? false),
  });
}

/** Whether DigiKey credentials are configured, plus the sandbox/production
 * environment currently selected (`get_digikey_status`) — never the
 * credentials themselves. Lets the enrichment UI explain why the DigiKey
 * provider silently contributed nothing to a preview. */
export function useDigiKeyStatus(): UseQueryResult<DigiKeyStatus, CommandError> {
  return useQuery({
    queryKey: keys.digikeyStatus,
    queryFn: () => unwrap(commands.getDigikeyStatus()),
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

export interface ApplyEnrichmentVariables {
  partId: PartId;
  applied: AppliedField[];
}

/** Apply (`apply_enrichment`): writes the caller-approved subset of a
 * preview's diffs — each `AppliedField` carries the value+source the caller
 * already saw and approved in the `EnrichmentDiff`, so apply never re-runs
 * the provider chain — in ONE all-or-nothing transaction, upserting
 * `field_provenance` for each approved field.
 *
 * Invalidates every query an approved field can affect: this part's record
 * and attributes (a `variant.*`/`description`/`category`/`attr.*` write can
 * change `metadata_complete`, the display fields, or an attribute value),
 * this part's variants (`variant.*` fields live on the preferred
 * manufacturer variant), search (every one of those fields is indexed into
 * search_text), the dashboard summary (`metadata_complete`/attribute
 * completeness feed its counts), and this part's own enrichment preview
 * (the diff just applied is now stale — a re-open should show it against
 * the fresh current values). Mirrors the invalidation breadth
 * `useUpdatePart`/`useSetAttribute` (`hooks/inventory.ts`) use for the same
 * underlying fields, since `apply_enrichment` writes through the exact same
 * paths a manual edit would. */
export function useApplyEnrichment(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<ApplyEnrichmentVariables, null>(
    ({ partId, applied }) => unwrap(commands.applyEnrichment(partId, applied)),
    (_data, variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.part(variables.partId) });
      queryClient.invalidateQueries({ queryKey: keys.attributes(variables.partId) });
      queryClient.invalidateQueries({ queryKey: keys.variants(variables.partId) });
      queryClient.invalidateQueries({ queryKey: keys.allParts });
      queryClient.invalidateQueries({ queryKey: keys.allSearch });
      queryClient.invalidateQueries({ queryKey: keys.dashboardSummary });
      queryClient.invalidateQueries({ queryKey: keys.enrichmentPreview(variables.partId) });
    },
    callbacks,
  );
}

/** The sandbox/production toggle (`set_digikey_environment`): only
 * `"sandbox"`/`"production"` are accepted server-side — anything else comes
 * back as a typed `invalid_digikey_environment` `CommandError`. Invalidates
 * `keys.digikeyStatus` so the status readout reflects the new environment
 * immediately. */
export function useSetDigiKeyEnvironment(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<string, null>(
    (environment) => unwrap(commands.setDigikeyEnvironment(environment)),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.digikeyStatus });
    },
    callbacks,
  );
}

export interface SetDigiKeyCredentialsVariables {
  clientId: string;
  clientSecret: string;
}

/** Store the DigiKey OAuth2 client-credentials pair (`set_digikey_credentials`):
 * `clientId`/`clientSecret` cross the IPC boundary exactly ONCE, write-only —
 * the command returns nothing, so there is no response value that could ever
 * echo a credential back. This hook passes the two values through to the
 * command verbatim and does not itself retain, cache, or invalidate anything
 * with them — the ONLY thing it stores in TanStack Query's cache is the
 * invalidation of `keys.digikeyStatus`, so the Settings screen's
 * `configured`/environment readout reflects the new credentials immediately.
 * The caller (the Settings form) owns clearing its own input fields on
 * success. */
export function useSetDigiKeyCredentials(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetDigiKeyCredentialsVariables, null>(
    ({ clientId, clientSecret }) => unwrap(commands.setDigikeyCredentials(clientId, clientSecret)),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.digikeyStatus });
    },
    callbacks,
  );
}

/** Delete both stored DigiKey credential entries (`clear_digikey_credentials`)
 * — idempotent server-side (clearing an already-empty store is still `Ok`).
 * Invalidates `keys.digikeyStatus` so the Settings screen immediately shows
 * `configured: false`. */
export function useClearDigiKeyCredentials(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<void, null>(
    () => unwrap(commands.clearDigikeyCredentials()),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.digikeyStatus });
    },
    callbacks,
  );
}

/** Verify stored DigiKey credentials actually authenticate
 * (`test_digikey_connection`): attempts ONLY an OAuth2 token fetch (no
 * product-details call) against the currently-configured sandbox/production
 * environment, and returns a `DigiKeyTestResult` carrying a fixed status
 * string — never a response body or the credential itself. No invalidation:
 * this command reads credentials/environment but never writes anything, so
 * there is nothing in the query cache for a successful test to make
 * stale. */
export function useTestDigiKeyConnection(callbacks?: MutationCallbacks<DigiKeyTestResult>) {
  return useUnwrapMutation<void, DigiKeyTestResult>(
    () => unwrap(commands.testDigikeyConnection()),
    () => {
      // No invalidation: `test_digikey_connection` never writes state.
    },
    callbacks,
  );
}
