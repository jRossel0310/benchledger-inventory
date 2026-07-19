import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { PublishOutcomeDto, PublishStatus } from '../../bindings.gen';
import { commands, type Result } from '../../lib/commands';

// Controllable tauri event layer: capture registered handlers per event
// name so tests can fire `close-publish-requested` on demand. The returned
// unlisten fn removes the handler again, mirroring the real API's shape.
const listeners = new Map<string, Array<() => void>>();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: () => void) => {
    const current = listeners.get(event) ?? [];
    current.push(handler);
    listeners.set(event, current);
    return Promise.resolve(() => {
      listeners.set(
        event,
        (listeners.get(event) ?? []).filter((h) => h !== handler),
      );
    });
  }),
}));

import { CLOSE_PUBLISH_EVENT, ClosePublishDialog, PUBLISH_TIMEOUT_MS } from './ClosePublishDialog';

afterEach(() => {
  cleanup();
  listeners.clear();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

function statusOf(configured: boolean): PublishStatus {
  return {
    configured,
    repo: configured ? 'jacob/bench-ledger-public' : null,
    last_published_at: null,
    pending: false,
    vercel_url: null,
  };
}

const publishedOutcome: Result<PublishOutcomeDto> = {
  status: 'ok',
  data: { status: 'published', digest: 'abc123' },
};

const authError: Result<PublishOutcomeDto> = {
  status: 'error',
  error: { code: 'github_auth', message: 'GitHub rejected the stored token.' },
};

function renderDialog() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ClosePublishDialog />
    </QueryClientProvider>,
  );
}

/** Fire the Rust-side close event at every registered listener. `listen()`
 * resolves in a microtask, so one flush pass first guarantees registration
 * completed (works under fake timers too — promises are microtasks, which
 * `vi.useFakeTimers` does not fake). */
async function emitClose() {
  await act(async () => {});
  const handlers = listeners.get(CLOSE_PUBLISH_EVENT) ?? [];
  expect(handlers.length).toBeGreaterThan(0);
  await act(async () => {
    for (const handler of [...handlers]) handler();
  });
}

describe('ClosePublishDialog', () => {
  it('finalizes immediately without any dialog when publishing is unconfigured', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(false),
    });
    const publishSpy = vi.spyOn(commands, 'publishNow');
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();

    expect(finalizeSpy).toHaveBeenCalledTimes(1);
    expect(publishSpy).not.toHaveBeenCalled();
    expect(screen.queryByText('Publishing before close…')).toBeNull();
  });

  it('finalizes when the fresh status read itself fails (closing must always work)', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'error',
      error: { code: 'internal', message: 'db lock poisoned' },
    });
    const publishSpy = vi.spyOn(commands, 'publishNow');
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();

    expect(finalizeSpy).toHaveBeenCalledTimes(1);
    expect(publishSpy).not.toHaveBeenCalled();
  });

  it('shows "Publishing before close…" while the publish runs, then finalizes on success', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    let resolvePublish!: (value: Result<PublishOutcomeDto>) => void;
    vi.spyOn(commands, 'publishNow').mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvePublish = resolve;
        }),
    );
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();

    expect(screen.getByText('Publishing before close…')).toBeTruthy();
    expect(finalizeSpy).not.toHaveBeenCalled();

    await act(async () => {
      resolvePublish(publishedOutcome);
    });

    expect(finalizeSpy).toHaveBeenCalledTimes(1);
  });

  it('treats an Unchanged outcome exactly like a publish: finalize', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    vi.spyOn(commands, 'publishNow').mockResolvedValue({
      status: 'ok',
      data: { status: 'unchanged', digest: null },
    });
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();

    expect(finalizeSpy).toHaveBeenCalledTimes(1);
  });

  it('shows the failure state with Retry and Close anyway on a publish error', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    vi.spyOn(commands, 'publishNow').mockResolvedValue(authError);
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();

    expect(screen.getByText('Publish failed')).toBeTruthy();
    expect(
      screen.getByText('Publish failed — it will retry next launch. Your local data is safe.'),
    ).toBeTruthy();
    expect(screen.getByText('GitHub rejected the stored token.')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Close anyway' })).toBeTruthy();
    expect(finalizeSpy).not.toHaveBeenCalled();
  });

  it('Retry re-fires the publish and finalizes when the retry succeeds', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    const publishSpy = vi
      .spyOn(commands, 'publishNow')
      .mockResolvedValueOnce(authError)
      .mockResolvedValueOnce(publishedOutcome);
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();
    expect(publishSpy).toHaveBeenCalledTimes(1);

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    });

    expect(publishSpy).toHaveBeenCalledTimes(2);
    expect(finalizeSpy).toHaveBeenCalledTimes(1);
  });

  it('Close anyway finalizes without another publish attempt', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    const publishSpy = vi.spyOn(commands, 'publishNow').mockResolvedValue(authError);
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Close anyway' }));
    });

    expect(finalizeSpy).toHaveBeenCalledTimes(1);
    expect(publishSpy).toHaveBeenCalledTimes(1);
  });

  it('flips to the failure state after the 20s timeout and IGNORES the late settle', async () => {
    vi.useFakeTimers();
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    let resolvePublish!: (value: Result<PublishOutcomeDto>) => void;
    vi.spyOn(commands, 'publishNow').mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvePublish = resolve;
        }),
    );
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();
    expect(screen.getByText('Publishing before close…')).toBeTruthy();

    act(() => {
      vi.advanceTimersByTime(PUBLISH_TIMEOUT_MS);
    });

    expect(screen.getByText('Publish failed')).toBeTruthy();
    expect(screen.getByText('The publish attempt timed out after 20 seconds.')).toBeTruthy();

    // The in-flight publish settling AFTER the timeout must not yank the
    // window shut while the user reads the failure dialog.
    await act(async () => {
      resolvePublish(publishedOutcome);
    });

    expect(finalizeSpy).not.toHaveBeenCalled();
    expect(screen.getByText('Publish failed')).toBeTruthy();
  });

  it('cannot be dismissed with Escape or an outside click', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    vi.spyOn(commands, 'publishNow').mockResolvedValue(authError);
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();
    expect(screen.getByText('Publish failed')).toBeTruthy();

    await act(async () => {
      fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    });
    expect(screen.getByText('Publish failed')).toBeTruthy();

    await act(async () => {
      fireEvent.pointerDown(document.body);
      fireEvent.mouseDown(document.body);
      fireEvent.click(document.body);
    });
    expect(screen.getByText('Publish failed')).toBeTruthy();
    expect(finalizeSpy).not.toHaveBeenCalled();
  });

  it('ignores a duplicate close event while the flow is already active', async () => {
    const statusSpy = vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    const publishSpy = vi
      .spyOn(commands, 'publishNow')
      .mockImplementation(() => new Promise(() => {}));
    vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();
    await emitClose();

    expect(statusSpy).toHaveBeenCalledTimes(1);
    expect(publishSpy).toHaveBeenCalledTimes(1);
  });

  it('ignores a re-emitted close event while the failure dialog is showing', async () => {
    // Rust re-emits `close-publish-requested` on EVERY close request (so a
    // swallowed first event cannot wedge the window). A user clicking X at
    // the failure dialog therefore produces another event — which must not
    // restart the flow or dismiss the dialog.
    const statusSpy = vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: statusOf(true),
    });
    const publishSpy = vi.spyOn(commands, 'publishNow').mockResolvedValue(authError);
    const finalizeSpy = vi.spyOn(commands, 'finalizeClose').mockResolvedValue(undefined);
    renderDialog();

    await emitClose();
    expect(screen.getByText('Publish failed')).toBeTruthy();

    await emitClose();

    expect(screen.getByText('Publish failed')).toBeTruthy();
    expect(statusSpy).toHaveBeenCalledTimes(1);
    expect(publishSpy).toHaveBeenCalledTimes(1);
    expect(finalizeSpy).not.toHaveBeenCalled();
  });
});
