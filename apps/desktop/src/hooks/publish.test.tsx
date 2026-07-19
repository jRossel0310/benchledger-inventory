import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { StrictMode, type ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { GitHubTestResult, PublishOutcomeDto, PublishStatus } from '../bindings.gen';
import { commands } from '../lib/commands';
import {
  keys,
  useClearGitHubToken,
  usePublishNow,
  usePublishStatus,
  useRetryPendingPublish,
  useSetGitHubToken,
  useSetPublishConfig,
  useStartupPublishRetry,
  useTestGitHubConnection,
} from './publish';

function makeClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
}

function wrapperFor(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

const unconfiguredStatus: PublishStatus = {
  configured: false,
  repo: null,
  last_published_at: null,
  pending: false,
  vercel_url: null,
};

describe('query keys', () => {
  it('extend inventory.ts’s keys with the publish entry', () => {
    // The shared entity keys (spread from inventory.ts) are still present...
    expect(keys.allParts).toEqual(['parts']);
    expect(keys.dashboardSummary).toEqual(['dashboardSummary']);
    // ...alongside the new publish key.
    expect(keys.publishStatus).toEqual(['publishStatus']);
  });
});

describe('usePublishStatus', () => {
  it('calls commands.getPublishStatus and returns the status', async () => {
    vi.spyOn(commands, 'getPublishStatus').mockResolvedValue({
      status: 'ok',
      data: unconfiguredStatus,
    });
    const queryClient = makeClient();

    const { result } = renderHook(() => usePublishStatus(), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.getPublishStatus).toHaveBeenCalled();
    expect(result.current.data).toEqual(unconfiguredStatus);
  });
});

describe('useSetPublishConfig', () => {
  it('passes the config through and invalidates publishStatus', async () => {
    const spy = vi
      .spyOn(commands, 'setPublishConfig')
      .mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetPublishConfig(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({
        owner: 'jacob',
        repo: 'bench-ledger-public',
        branch: '',
        path: '',
        vercelUrl: 'https://bench.vercel.app',
      });
    });

    expect(spy).toHaveBeenCalledWith(
      'jacob',
      'bench-ledger-public',
      '',
      '',
      'https://bench.vercel.app',
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.publishStatus }),
    );
  });

  it('surfaces a CommandError (e.g. a blank owner) as a rejected mutation without invalidating', async () => {
    const error = { code: 'invalid_publish_config', message: 'must not be empty' };
    vi.spyOn(commands, 'setPublishConfig').mockResolvedValue({ status: 'error', error });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetPublishConfig(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await expect(
        result.current.mutateAsync({ owner: '', repo: '', branch: '', path: '', vercelUrl: null }),
      ).rejects.toEqual(error);
    });

    expect(invalidateSpy).not.toHaveBeenCalled();
  });
});

describe('useSetGitHubToken', () => {
  it('passes the token through to setGithubToken exactly once and invalidates publishStatus', async () => {
    const spy = vi
      .spyOn(commands, 'setGithubToken')
      .mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetGitHubToken(), { wrapper: wrapperFor(queryClient) });
    let returned: null | undefined;
    await act(async () => {
      returned = await result.current.mutateAsync('fake-token-abc');
    });

    expect(spy).toHaveBeenCalledWith('fake-token-abc');
    expect(spy).toHaveBeenCalledTimes(1);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.publishStatus }),
    );

    // The hook's own return value never carries the token back out — the
    // command itself returns nothing (`null`), so there is nothing for a
    // caller to accidentally persist or log.
    expect(returned).toBeNull();

    // Nothing token-shaped ended up in the query cache either: the only
    // cache interaction was the publishStatus invalidation above.
    const cached = JSON.stringify(
      queryClient
        .getQueryCache()
        .getAll()
        .map((q) => q.state.data),
    );
    expect(cached).not.toContain('fake-token-abc');
  });

  it('surfaces a CommandError (e.g. a blank token) as a rejected mutation without invalidating', async () => {
    const error = { code: 'invalid_token', message: 'GitHub token must not be empty' };
    vi.spyOn(commands, 'setGithubToken').mockResolvedValue({ status: 'error', error });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetGitHubToken(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await expect(result.current.mutateAsync('')).rejects.toEqual(error);
    });

    expect(invalidateSpy).not.toHaveBeenCalled();
  });
});

describe('useClearGitHubToken', () => {
  it('calls clearGithubToken and invalidates publishStatus', async () => {
    vi.spyOn(commands, 'clearGithubToken').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useClearGitHubToken(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync();
    });

    expect(commands.clearGithubToken).toHaveBeenCalled();
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.publishStatus }),
    );
  });
});

describe('useTestGitHubConnection', () => {
  it('calls testGithubConnection and returns the result without invalidating any query', async () => {
    const testResult: GitHubTestResult = { ok: false, message: 'not configured' };
    vi.spyOn(commands, 'testGithubConnection').mockResolvedValue({
      status: 'ok',
      data: testResult,
    });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useTestGitHubConnection(), {
      wrapper: wrapperFor(queryClient),
    });
    let returned: GitHubTestResult | undefined;
    await act(async () => {
      returned = await result.current.mutateAsync();
    });

    expect(commands.testGithubConnection).toHaveBeenCalled();
    expect(returned).toEqual(testResult);
    expect(invalidateSpy).not.toHaveBeenCalled();
  });
});

describe('usePublishNow', () => {
  it('returns the published outcome and invalidates publishStatus', async () => {
    const outcome: PublishOutcomeDto = { status: 'published', digest: 'abc123' };
    vi.spyOn(commands, 'publishNow').mockResolvedValue({ status: 'ok', data: outcome });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => usePublishNow(), { wrapper: wrapperFor(queryClient) });
    let returned: PublishOutcomeDto | undefined;
    await act(async () => {
      returned = await result.current.mutateAsync();
    });

    expect(commands.publishNow).toHaveBeenCalled();
    expect(returned).toEqual(outcome);
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.publishStatus }),
    );
  });

  it('surfaces a typed publish failure as a rejected mutation', async () => {
    const error = { code: 'github_auth', message: 'GitHub rejected the token' };
    vi.spyOn(commands, 'publishNow').mockResolvedValue({ status: 'error', error });
    const queryClient = makeClient();

    const { result } = renderHook(() => usePublishNow(), { wrapper: wrapperFor(queryClient) });
    await act(async () => {
      await expect(result.current.mutateAsync()).rejects.toEqual(error);
    });
  });
});

describe('useRetryPendingPublish', () => {
  it('resolves null on the quiet no-op path and still invalidates publishStatus', async () => {
    vi.spyOn(commands, 'retryPendingPublish').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useRetryPendingPublish(), {
      wrapper: wrapperFor(queryClient),
    });
    let returned: PublishOutcomeDto | null | undefined;
    await act(async () => {
      returned = await result.current.mutateAsync();
    });

    expect(commands.retryPendingPublish).toHaveBeenCalled();
    expect(returned).toBeNull();
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.publishStatus }),
    );
  });

  it('returns the outcome when a pending publish was actually retried', async () => {
    const outcome: PublishOutcomeDto = { status: 'published', digest: 'def456' };
    vi.spyOn(commands, 'retryPendingPublish').mockResolvedValue({ status: 'ok', data: outcome });
    const queryClient = makeClient();

    const { result } = renderHook(() => useRetryPendingPublish(), {
      wrapper: wrapperFor(queryClient),
    });
    let returned: PublishOutcomeDto | null | undefined;
    await act(async () => {
      returned = await result.current.mutateAsync();
    });

    expect(returned).toEqual(outcome);
  });
});

describe('useStartupPublishRetry', () => {
  /** StrictMode wrapper: the production shell mounts under
   * `<React.StrictMode>` (main.tsx), whose simulated double-mount runs
   * every mount effect twice — exactly the environment the hook's
   * fire-once ref guard exists for, so these tests reproduce it. */
  function strictWrapperFor(queryClient: QueryClient) {
    return function Wrapper({ children }: { children: ReactNode }) {
      return (
        <StrictMode>
          <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
        </StrictMode>
      );
    };
  }

  it('fires retry_pending_publish exactly once despite the StrictMode double-mount', async () => {
    const spy = vi
      .spyOn(commands, 'retryPendingPublish')
      .mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();

    renderHook(() => useStartupPublishRetry(), { wrapper: strictWrapperFor(queryClient) });

    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));
    // Let everything settle — a second (double-mount) fire would land here.
    await act(async () => {});
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it('stays silent when the retry fails (no throw, no unhandled rejection)', async () => {
    const spy = vi.spyOn(commands, 'retryPendingPublish').mockResolvedValue({
      status: 'error',
      error: { code: 'github_auth', message: 'GitHub rejected the token' },
    });
    const queryClient = makeClient();

    renderHook(() => useStartupPublishRetry(), { wrapper: strictWrapperFor(queryClient) });

    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));
    // Settle the rejected mutation: reaching this assertion without vitest
    // flagging an unhandled rejection IS the "quiet on error" contract.
    await act(async () => {});
    expect(spy).toHaveBeenCalledTimes(1);
  });
});
