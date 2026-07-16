/**
 * Typed command surface. Re-exports the generated `commands.*` (never call
 * `@tauri-apps/api/core` `invoke` directly — see the Phase 3 plan's global
 * constraint) plus a small `unwrap` helper that turns tauri-specta's
 * `{status: "ok" | "error"}` result shape into a plain rejected/resolved
 * Promise, so TanStack Query's error handling (retry, `isError`,
 * `error`/`onError`) works the normal way instead of every call site having
 * to branch on `.status` itself.
 */

import { commands } from '../bindings.gen';
import type { CommandError } from '../bindings.gen';

export { commands };
export type { CommandError };

/** The shape every generated command resolves to: tauri-specta's
 * `typedError` wraps a successful call as `{status: "ok", data}` and a
 * thrown `CommandError` as `{status: "error", error}` (see the
 * `typedError` runtime helper at the bottom of `bindings.gen.ts`). */
export type Result<T, E = CommandError> = { status: 'ok'; data: T } | { status: 'error'; error: E };

/**
 * Unwrap a command result, throwing the `CommandError` when it's an error
 * variant so callers (especially TanStack Query `queryFn`/`mutationFn`) see
 * a normal resolve/reject Promise instead of a tagged union. Accepts either
 * an already-resolved `Result` or the `Promise<Result<...>>` a `commands.*`
 * call returns directly, so it can wrap a call inline:
 * `queryFn: () => unwrap(commands.listParts(false))`.
 */
export function unwrap<T>(
  result: Result<T, CommandError> | Promise<Result<T, CommandError>>,
): Promise<T> {
  return Promise.resolve(result).then((r) => {
    if (r.status === 'error') throw r.error;
    return r.data;
  });
}
