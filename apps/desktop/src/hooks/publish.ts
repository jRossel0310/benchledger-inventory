/**
 * The TanStack Query layer over the Phase 6 Task 4 publish command surface
 * (`get_publish_status`, `set_publish_config`, `set_github_token`,
 * `clear_github_token`, `test_github_connection`, `publish_now`,
 * `retry_pending_publish` — `inventory_sync::publish` + the GitHub token
 * plumbing). Same shape as `hooks/enrichment.ts`: a query hook per read, a
 * mutation hook per write, one `keys` object (extending `inventory.ts`'s,
 * the same "spread, don't duplicate" pattern), `unwrap` around every
 * command call.
 *
 * Secrets discipline mirrors `useSetDigiKeyCredentials` exactly: the token
 * crosses `useSetGitHubToken` exactly once, write-only — the command
 * returns nothing, this hook caches nothing derived from the token, and
 * the caller (the Settings form) owns clearing its own input field.
 */

import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import { useEffect, useRef } from 'react';

import type {
  CommandError,
  GitHubTestResult,
  PublishOutcomeDto,
  PublishStatus,
} from '../bindings.gen';
import { commands, unwrap } from '../lib/commands';
import { keys as inventoryKeys, type MutationCallbacks } from './inventory';

/** Extends `inventory.ts`'s `keys` with the publish entry this file's hooks
 * use — same "spread, don't duplicate" pattern `hooks/enrichment.ts`/
 * `hooks/projects.ts` follow. */
export const keys = {
  ...inventoryKeys,
  publishStatus: ['publishStatus'] as const,
};

// ---------------------------------------------------------------------
// Query hooks
// ---------------------------------------------------------------------

/** Publish state for the Settings screen and the Dashboard card
 * (`get_publish_status`): configured/repo/last-published/pending/Vercel URL
 * — never the token (the response shape has no field that could carry
 * one). */
export function usePublishStatus(): UseQueryResult<PublishStatus, CommandError> {
  return useQuery({
    queryKey: keys.publishStatus,
    queryFn: () => unwrap(commands.getPublishStatus()),
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

export interface SetPublishConfigVariables {
  owner: string;
  repo: string;
  branch: string;
  path: string;
  vercelUrl: string | null;
}

/** Store the publish configuration (`set_publish_config`): owner/repo are
 * required (typed `invalid_publish_config` reject on blank), branch/path
 * fall back to their defaults server-side when blank, and a `null`/blank
 * Vercel URL clears the stored one. Invalidates `keys.publishStatus` so
 * the Settings/Dashboard readouts reflect the new target immediately. */
export function useSetPublishConfig(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<SetPublishConfigVariables, null>(
    ({ owner, repo, branch, path, vercelUrl }) =>
      unwrap(commands.setPublishConfig(owner, repo, branch, path, vercelUrl)),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.publishStatus });
    },
    callbacks,
  );
}

/** Store the GitHub publish token (`set_github_token`): the token crosses
 * the IPC boundary exactly ONCE, write-only — the command returns nothing,
 * so there is no response value that could ever echo it back, and this
 * hook neither retains nor caches it. Invalidates `keys.publishStatus`
 * (house rule: every publish-surface mutation does — the status shape
 * itself carries no token field either way). The caller (the Settings
 * form) owns clearing its own input field on success. */
export function useSetGitHubToken(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<string, null>(
    (token) => unwrap(commands.setGithubToken(token)),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.publishStatus });
    },
    callbacks,
  );
}

/** Delete the stored GitHub token (`clear_github_token`) — idempotent
 * server-side (clearing an already-empty store is still `Ok`). Invalidates
 * `keys.publishStatus`, same rule as every other publish mutation. */
export function useClearGitHubToken(callbacks?: MutationCallbacks<null>) {
  return useUnwrapMutation<void, null>(
    () => unwrap(commands.clearGithubToken()),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.publishStatus });
    },
    callbacks,
  );
}

/** Probe the configured repo (`test_github_connection`): a single
 * contents-GET against the configured path, returning a `GitHubTestResult`
 * with one of five fixed strings — never a response body or the token. No
 * invalidation: the probe reads config/token but writes nothing, so there
 * is nothing in the query cache for a test to make stale (same reasoning
 * as `useTestDigiKeyConnection`). */
export function useTestGitHubConnection(callbacks?: MutationCallbacks<GitHubTestResult>) {
  return useUnwrapMutation<void, GitHubTestResult>(
    () => unwrap(commands.testGithubConnection()),
    () => {
      // No invalidation: `test_github_connection` never writes state.
    },
    callbacks,
  );
}

/** Publish the snapshot now (`publish_now`): digest-checked server-side, so
 * the outcome is either `{status: "published", digest}` or
 * `{status: "unchanged"}` (no upload happened — the remote is already
 * current). Invalidates `keys.publishStatus`: a successful publish moves
 * `last_published_at` and clears `pending`, and a failed one (surfaced as
 * a typed `CommandError` here) has set the pending marker server-side —
 * TanStack Query runs `invalidateQueries` only on success, but the caller's
 * error path should refetch status itself if it surfaces the failure. */
export function usePublishNow(callbacks?: MutationCallbacks<PublishOutcomeDto>) {
  return useUnwrapMutation<void, PublishOutcomeDto>(
    () => unwrap(commands.publishNow()),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.publishStatus });
    },
    callbacks,
  );
}

/** The QUIET startup retry (`retry_pending_publish`): resolves `null` —
 * without erroring — when there is nothing to do (no pending marker,
 * publishing unconfigured, or no token stored), and otherwise re-attempts
 * the pending publish. Invalidates `keys.publishStatus`: a successful
 * retry clears the pending flag and stamps a fresh `last_published_at`. */
export function useRetryPendingPublish(callbacks?: MutationCallbacks<PublishOutcomeDto | null>) {
  return useUnwrapMutation<void, PublishOutcomeDto | null>(
    () => unwrap(commands.retryPendingPublish()),
    (_data, _variables, queryClient) => {
      queryClient.invalidateQueries({ queryKey: keys.publishStatus });
    },
    callbacks,
  );
}

/**
 * The QUIET startup retry (Phase 6 Task 6): fire `retry_pending_publish`
 * exactly once when the shell mounts. Deliberately silent in every
 * direction — no toast on failure (the Dashboard card still shows the
 * pending state) and none on success either (the card just updates via
 * this mutation's `keys.publishStatus` invalidation). The command itself
 * is already quiet about inapplicable states (`null` when there is no
 * pending marker, no config, or no token), so the only possible error here
 * is a genuinely attempted-and-failed publish — which re-set the pending
 * marker server-side and needs nothing from us.
 *
 * The ref guard makes "once" survive StrictMode's simulated double-mount
 * (refs persist across it); `onError` is swallowed so the rejected
 * mutation never surfaces anywhere.
 */
export function useStartupPublishRetry(): void {
  const retry = useRetryPendingPublish();
  const firedRef = useRef(false);
  const mutate = retry.mutate;
  useEffect(() => {
    if (firedRef.current) return;
    firedRef.current = true;
    mutate(undefined, {
      onError: () => {
        // Quiet by contract — the pending marker is already set server-side.
      },
    });
  }, [mutate]);
}
