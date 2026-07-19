/**
 * The close-time publish flow (Phase 6 Task 6). The Rust side
 * (`src-tauri/src/close_flow.rs` + `main.rs`) intercepts the FIRST window
 * close request, prevents it, and emits `close-publish-requested` exactly
 * once; from there this component owns the whole state machine, and the
 * only way the app actually exits is this component calling the
 * `finalize_close` command (`AppHandle::exit(0)`, which bypasses the close
 * guard entirely).
 *
 * State machine (per the Phase 6 plan):
 *
 * - `idle` — mounted (in `AppShell`, so it exists on every route), no
 *   dialog rendered, listening for the close event.
 * - On `close-publish-requested`: fetch a FRESH publish status (a direct
 *   `get_publish_status` call, not the query cache — the cached status may
 *   be stale, and closing on stale config would either publish when the
 *   user just unconfigured or skip when they just configured). Publishing
 *   unconfigured → `finalize_close` immediately: no dialog flashes for
 *   users who never set publishing up. A status read failure also
 *   finalizes — closing must always work, and any pending marker retries
 *   next launch anyway.
 * - `publishing` — non-dismissable dialog "Publishing before close…" while
 *   `publish_now` runs. Success — `published` OR `unchanged` (the remote
 *   is already current) — finalizes straight away, no toast (the window is
 *   about to disappear; nobody would see it).
 * - `failed` — a typed publish error OR a 20s timeout (cleared when the
 *   attempt settles). Shows the error plus the honest reassurance copy and
 *   two buttons: **Retry** (re-fires `publish_now`, fresh timeout) and
 *   **Close anyway** (finalizes; the failed publish already set the
 *   pending-publish marker server-side, so the next launch's quiet startup
 *   retry — `useStartupPublishRetry` — picks it up).
 *
 * A timed-out attempt is INVALIDATED, not just abandoned: if the in-flight
 * `publish_now` settles after the timeout already flipped the dialog to
 * `failed`, that late result is ignored — a late success yanking the
 * window shut while the user reads the failure dialog would be worse than
 * one redundant retry (which would come back `unchanged` and finalize).
 *
 * Non-dismissable by design: no ESC, no backdrop click, no × — closing the
 * app is the only way out of the dialog, and both buttons do exactly that
 * (after a retry succeeds, or immediately). `Dialog.Root` gets no
 * `onOpenChange`, and the content swallows escape/outside-interaction
 * events.
 */

import * as Dialog from '@radix-ui/react-dialog';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef, useState } from 'react';

import { usePublishNow } from '../../hooks/publish';
import { commands, unwrap } from '../../lib/commands';
import { errorMessage } from '../../lib/format';
import './ClosePublishDialog.css';

/** The event `main.rs`'s close guard emits (exactly once per app run). */
export const CLOSE_PUBLISH_EVENT = 'close-publish-requested';

/** How long a close-time publish attempt may run before the dialog stops
 * waiting and offers Retry / Close anyway. */
export const PUBLISH_TIMEOUT_MS = 20_000;

type CloseFlowState =
  { phase: 'idle' } | { phase: 'publishing' } | { phase: 'failed'; message: string };

export function ClosePublishDialog() {
  const [flow, setFlow] = useState<CloseFlowState>({ phase: 'idle' });
  const publishNow = usePublishNow();

  // Belt-and-braces re-entry guard (the Rust close guard already emits at
  // most once per run, but a briefly-overlapping StrictMode listener pair
  // must not double-drive the flow).
  const flowActiveRef = useRef(false);
  // Monotonic attempt counter: a settle (success/error) or timeout only
  // acts if its attempt is still the current one. The timeout handler
  // bumps the counter, invalidating the in-flight attempt outright.
  const attemptRef = useRef(0);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  function clearAttemptTimeout() {
    if (timeoutRef.current !== null) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
  }

  /** The one true exit: `finalize_close` → `AppHandle::exit(0)`. Fire and
   * forget — there is no meaningful recovery from a failed exit call, and
   * the command itself cannot error. */
  function finalize() {
    void commands.finalizeClose();
  }

  function startPublish() {
    const attempt = ++attemptRef.current;
    setFlow({ phase: 'publishing' });
    clearAttemptTimeout();
    timeoutRef.current = setTimeout(() => {
      if (attempt !== attemptRef.current) return;
      // Invalidate the in-flight attempt: its late settle (either way) is
      // ignored — see the module doc.
      attemptRef.current += 1;
      timeoutRef.current = null;
      setFlow({
        phase: 'failed',
        message: 'The publish attempt timed out after 20 seconds.',
      });
    }, PUBLISH_TIMEOUT_MS);
    publishNow.mutate(undefined, {
      onSuccess: () => {
        if (attempt !== attemptRef.current) return;
        clearAttemptTimeout();
        // `published` and `unchanged` both mean the remote is current.
        finalize();
      },
      onError: (error) => {
        if (attempt !== attemptRef.current) return;
        clearAttemptTimeout();
        // The failed publish already set the pending-publish marker
        // server-side (`inventory_sync::publish`).
        setFlow({ phase: 'failed', message: errorMessage(error) });
      },
    });
  }

  async function handleCloseRequested() {
    if (flowActiveRef.current) return;
    flowActiveRef.current = true;
    let configured = false;
    try {
      // Fresh read, deliberately NOT the query cache — see the module doc.
      configured = (await unwrap(commands.getPublishStatus())).configured;
    } catch {
      // Status unreadable — treat as unconfigured: closing must always
      // work, and a pending marker (if any) retries next launch.
    }
    if (!configured) {
      finalize();
      return;
    }
    startPublish();
  }

  // Keep the listener registered once while always invoking the latest
  // handler (the handler closes over hook state that changes per render).
  const handlerRef = useRef(handleCloseRequested);
  handlerRef.current = handleCloseRequested;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen(CLOSE_PUBLISH_EVENT, () => {
      void handlerRef.current();
    })
      .then((fn) => {
        // StrictMode's simulated unmount can win the race against listen()
        // resolving; a listener registered after its effect was cleaned up
        // must be released immediately.
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // Not running under tauri (vitest/jsdom) — no close events exist.
      });
    return () => {
      disposed = true;
      unlisten?.();
      clearAttemptTimeout();
    };
  }, []);

  if (flow.phase === 'idle') return null;

  return (
    <Dialog.Root open>
      <Dialog.Portal>
        <Dialog.Overlay className="close-publish-overlay" />
        <Dialog.Content
          className="close-publish-content"
          onEscapeKeyDown={(event) => event.preventDefault()}
          onPointerDownOutside={(event) => event.preventDefault()}
          onInteractOutside={(event) => event.preventDefault()}
        >
          {flow.phase === 'failed' ? (
            <>
              <Dialog.Title className="close-publish-title">Publish failed</Dialog.Title>
              <Dialog.Description className="close-publish-description">
                Publish failed — it will retry next launch. Your local data is safe.
              </Dialog.Description>
              <p className="close-publish-error">{flow.message}</p>
              <div className="close-publish-buttons">
                <button
                  type="button"
                  className="close-publish-close-anyway"
                  onClick={() => finalize()}
                >
                  Close anyway
                </button>
                <button
                  type="button"
                  className="close-publish-retry"
                  onClick={() => startPublish()}
                >
                  Retry
                </button>
              </div>
            </>
          ) : (
            <>
              <Dialog.Title className="close-publish-title">Publishing before close…</Dialog.Title>
              <Dialog.Description className="close-publish-description">
                Uploading the latest snapshot to GitHub. The app closes as soon as it finishes.
              </Dialog.Description>
            </>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
