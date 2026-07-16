import { QueryClient } from '@tanstack/react-query';

/**
 * The app's single `QueryClient`. This wraps a local SQLite database behind
 * Tauri IPC, not a flaky network — so defaults lean away from aggressive
 * background retries/refetches:
 *  - `queries.retry: 1` — one retry covers a transient issue (e.g. a
 *    momentarily poisoned db-mutex lock); anything else (a `CommandError`
 *    like `part_not_found`) won't be fixed by retrying, so more than one
 *    just delays the error reaching the UI.
 *  - `mutations.retry: false` — mutations apply ledger operations
 *    (receive/reserve/check out/…). They are not idempotent, so an
 *    automatic retry after a failed-but-possibly-applied write could double
 *    a real stock movement. A failed mutation must surface to the user, who
 *    can retry deliberately.
 *  - `refetchOnWindowFocus: false` — a desktop app's window regaining focus
 *    isn't a signal that server data changed underneath it the way it is
 *    for a web app; explicit invalidation after mutations keeps data fresh.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      gcTime: 5 * 60_000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
    mutations: {
      retry: false,
    },
  },
});
