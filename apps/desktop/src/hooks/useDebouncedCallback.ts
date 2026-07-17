/**
 * Debounces a callback so rapid-fire calls (e.g. every keystroke in a search
 * box) only run it once, `delayMs` after the *last* call — "latest value
 * wins." Built for the search-box → route-`?q=` write (`AppShell.tsx`,
 * `InventoryPage.tsx`): the input itself stays controlled/immediate, but the
 * route-param write (which mints a new `keys.search(query)` cache key and
 * re-triggers the table query) is debounced so typing doesn't spam browser
 * history or thrash the cache.
 *
 * `flush` bypasses the delay and runs `callback` immediately, cancelling any
 * pending debounced call first — used for actions that must land right away
 * (e.g. clearing the search box via its native × button: a delayed write
 * there would leave a stale, already-cleared-looking box with the old query
 * still live for `delayMs`).
 */

import { useCallback, useEffect, useRef } from 'react';

export interface DebouncedCallback<A extends unknown[]> {
  /** Schedule `callback(...args)` to run after `delayMs`, resetting the
   * timer if called again before it fires. */
  run: (...args: A) => void;
  /** Run `callback(...args)` immediately, discarding any pending `run`. */
  flush: (...args: A) => void;
  /** Discard any pending `run` without invoking `callback`. */
  cancel: () => void;
}

export function useDebouncedCallback<A extends unknown[]>(
  callback: (...args: A) => void,
  delayMs: number,
): DebouncedCallback<A> {
  // Always call the latest `callback` (not the one captured when `run`/
  // `flush` were created) without those needing `callback` in their own
  // dependency arrays — avoids re-debouncing (dropping a pending call) just
  // because the caller passed a new function identity on every render.
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancel = useCallback(() => {
    if (timeoutRef.current !== null) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
  }, []);

  useEffect(() => cancel, [cancel]);

  const run = useCallback(
    (...args: A) => {
      cancel();
      timeoutRef.current = setTimeout(() => {
        timeoutRef.current = null;
        callbackRef.current(...args);
      }, delayMs);
    },
    [cancel, delayMs],
  );

  const flush = useCallback(
    (...args: A) => {
      cancel();
      callbackRef.current(...args);
    },
    [cancel],
  );

  return { run, flush, cancel };
}
