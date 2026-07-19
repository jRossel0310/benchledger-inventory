import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { GitHubTestResult, PublishOutcomeDto, PublishStatus } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { commands } from '../../lib/commands';
import { PublishSettings } from './PublishSettings';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

function publishStatus(overrides: Partial<PublishStatus> = {}): PublishStatus {
  return {
    configured: false,
    repo: null,
    last_published_at: null,
    pending: false,
    vercel_url: null,
    ...overrides,
  };
}

function testResult(overrides: Partial<GitHubTestResult> = {}): GitHubTestResult {
  return { ok: false, message: 'not configured', ...overrides };
}

function outcome(overrides: Partial<PublishOutcomeDto> = {}): PublishOutcomeDto {
  return { status: 'published', digest: 'abc123', ...overrides };
}

function renderSettings() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const Wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>{children}</ToastProvider>
    </QueryClientProvider>
  );
  return render(<PublishSettings />, { wrapper: Wrapper });
}

beforeEach(() => {
  vi.spyOn(commands, 'getPublishStatus').mockReturnValue(ok(publishStatus()));
  vi.spyOn(commands, 'setPublishConfig').mockReturnValue(ok(null));
  vi.spyOn(commands, 'setGithubToken').mockReturnValue(ok(null));
  vi.spyOn(commands, 'clearGithubToken').mockReturnValue(ok(null));
  vi.spyOn(commands, 'testGithubConnection').mockReturnValue(ok(testResult()));
  vi.spyOn(commands, 'publishNow').mockReturnValue(ok(outcome()));
  vi.spyOn(commands, 'retryPendingPublish').mockReturnValue(ok(outcome()));
});

afterEach(() => {
  vi.restoreAllMocks();
  cleanup();
});

describe('Status card', () => {
  it('shows Not configured, no repo, and Never when nothing is set up', async () => {
    renderSettings();

    expect(await screen.findByText('No')).toBeTruthy();
    expect(screen.getByText('Repository').nextElementSibling?.textContent).toBe('—');
    expect(screen.getByText('Last published').nextElementSibling?.textContent).toBe('Never');
    expect(screen.queryByText('Publish pending — will retry on launch')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  });

  it('shows the repo, a formatted last-published time, and the site link when configured', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(
        publishStatus({
          configured: true,
          repo: 'example/bench-ledger-site',
          last_published_at: '2026-07-18 09:30:00',
          vercel_url: 'https://bench-ledger.vercel.app',
        }),
      ),
    );
    renderSettings();

    expect(await screen.findByText('Yes')).toBeTruthy();
    expect(screen.getByText('Repository').nextElementSibling?.textContent).toBe(
      'example/bench-ledger-site',
    );
    // Formatted, not the raw ISO-ish string.
    const lastPublished = screen.getByText('Last published').nextElementSibling?.textContent ?? '';
    expect(lastPublished).not.toBe('Never');
    expect(lastPublished).not.toBe('2026-07-18 09:30:00');
    expect(lastPublished).toContain('2026');

    const siteLink = screen.getByRole('link', {
      name: 'https://bench-ledger.vercel.app',
    }) as HTMLAnchorElement;
    expect(siteLink.getAttribute('href')).toBe('https://bench-ledger.vercel.app');
  });

  it('shows the pending warning row with a Retry button when a publish is pending', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(publishStatus({ configured: true, repo: 'example/site', pending: true })),
    );
    renderSettings();

    expect(await screen.findByText('Publish pending — will retry on launch')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
  });
});

describe('Retry', () => {
  beforeEach(() => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(publishStatus({ configured: true, repo: 'example/site', pending: true })),
    );
  });

  it('calls retry_pending_publish and toasts Published on success', async () => {
    renderSettings();

    fireEvent.click(await screen.findByRole('button', { name: 'Retry' }));

    await waitFor(() => expect(commands.retryPendingPublish).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('Published')).toBeTruthy();
  });

  it('toasts the failure message and refetches status when the retry fails', async () => {
    vi.mocked(commands.retryPendingPublish).mockReturnValue(
      commandError('github_rejected', 'GitHub rejected the stored token'),
    );
    renderSettings();

    await screen.findByRole('button', { name: 'Retry' });
    const statusCallsBefore = vi.mocked(commands.getPublishStatus).mock.calls.length;
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    expect(await screen.findByText('Could not publish')).toBeTruthy();
    expect(screen.getByText('GitHub rejected the stored token')).toBeTruthy();
    // Failure still refetches status — the backend set/kept the pending flag.
    await waitFor(() =>
      expect(vi.mocked(commands.getPublishStatus).mock.calls.length).toBeGreaterThan(
        statusCallsBefore,
      ),
    );
  });
});

describe('Repository form', () => {
  it('disables Save until both owner and repo are filled', async () => {
    renderSettings();
    const saveButton = (await screen.findByRole('button', {
      name: 'Save publish settings',
    })) as HTMLButtonElement;
    expect(saveButton.disabled).toBe(true);

    fireEvent.change(screen.getByLabelText('Owner'), { target: { value: 'example' } });
    expect(saveButton.disabled).toBe(true);

    fireEvent.change(screen.getByLabelText('Repository name'), { target: { value: 'site' } });
    expect(saveButton.disabled).toBe(false);

    fireEvent.change(screen.getByLabelText('Owner'), { target: { value: '   ' } });
    expect(saveButton.disabled).toBe(true);
  });

  it('saves trimmed values, sends blank branch/path as empty strings, and toasts', async () => {
    renderSettings();

    // Branch and path show their server-side defaults as placeholders only —
    // an untouched field must submit as "" (the backend fills the default),
    // never as the placeholder text.
    const branchInput = (await screen.findByLabelText('Branch')) as HTMLInputElement;
    const pathInput = screen.getByLabelText('Snapshot path') as HTMLInputElement;
    expect(branchInput.placeholder).toBe('main');
    expect(pathInput.placeholder).toBe('apps/web/public/inventory.snapshot.json');
    expect(branchInput.value).toBe('');
    expect(pathInput.value).toBe('');

    fireEvent.change(screen.getByLabelText('Owner'), { target: { value: '  example  ' } });
    fireEvent.change(screen.getByLabelText('Repository name'), { target: { value: ' site ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save publish settings' }));

    await waitFor(() =>
      expect(commands.setPublishConfig).toHaveBeenCalledWith('example', 'site', '', '', null),
    );
    expect(commands.setPublishConfig).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('Publish settings saved')).toBeTruthy();
  });

  it('passes explicit branch, path, and Vercel URL through trimmed', async () => {
    renderSettings();

    fireEvent.change(await screen.findByLabelText('Owner'), { target: { value: 'example' } });
    fireEvent.change(screen.getByLabelText('Repository name'), { target: { value: 'site' } });
    fireEvent.change(screen.getByLabelText('Branch'), { target: { value: ' release ' } });
    fireEvent.change(screen.getByLabelText('Snapshot path'), {
      target: { value: ' data/snapshot.json ' },
    });
    fireEvent.change(screen.getByLabelText('Site URL (optional)'), {
      target: { value: ' https://example.vercel.app ' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save publish settings' }));

    await waitFor(() =>
      expect(commands.setPublishConfig).toHaveBeenCalledWith(
        'example',
        'site',
        'release',
        'data/snapshot.json',
        'https://example.vercel.app',
      ),
    );
  });

  it('toasts an error when the save fails', async () => {
    vi.mocked(commands.setPublishConfig).mockReturnValue(
      commandError('invalid_publish_config', 'owner and repo must not be blank'),
    );
    renderSettings();

    fireEvent.change(await screen.findByLabelText('Owner'), { target: { value: 'example' } });
    fireEvent.change(screen.getByLabelText('Repository name'), { target: { value: 'site' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save publish settings' }));

    expect(await screen.findByText('Could not save publish settings')).toBeTruthy();
  });
});

describe('Token form', () => {
  it('disables Save token until the field is non-empty', async () => {
    renderSettings();
    const saveButton = (await screen.findByRole('button', {
      name: 'Save token',
    })) as HTMLButtonElement;
    expect(saveButton.disabled).toBe(true);

    fireEvent.change(screen.getByLabelText('GitHub token'), { target: { value: 'tok' } });
    expect(saveButton.disabled).toBe(false);

    fireEvent.change(screen.getByLabelText('GitHub token'), { target: { value: '' } });
    expect(saveButton.disabled).toBe(true);
  });

  it('is a masked input, saves the exact entered value, then clears the field and toasts', async () => {
    renderSettings();
    const tokenInput = (await screen.findByLabelText('GitHub token')) as HTMLInputElement;
    expect(tokenInput.type).toBe('password');

    fireEvent.change(tokenInput, { target: { value: 'ghp_example_token_value' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save token' }));

    await waitFor(() =>
      expect(commands.setGithubToken).toHaveBeenCalledWith('ghp_example_token_value'),
    );
    expect(commands.setGithubToken).toHaveBeenCalledTimes(1);

    // The crux: the field is empty again once the save succeeds.
    await waitFor(() => expect(tokenInput.value).toBe(''));
    expect(await screen.findByText('Token saved')).toBeTruthy();
  });

  it('never renders the entered token anywhere in the DOM after saving (DOM-wide)', async () => {
    renderSettings();
    const token = 'super-secret-publish-token-42';
    fireEvent.change(await screen.findByLabelText('GitHub token'), { target: { value: token } });
    fireEvent.click(screen.getByRole('button', { name: 'Save token' }));

    await screen.findByText('Token saved');

    expect(document.body.innerHTML).not.toContain(token);
    expect(document.body.textContent ?? '').not.toContain(token);
  });

  it('shows an error toast and does not clear the field when the save fails', async () => {
    vi.mocked(commands.setGithubToken).mockReturnValue(
      commandError('invalid_token', 'token must not be empty'),
    );
    renderSettings();
    const tokenInput = (await screen.findByLabelText('GitHub token')) as HTMLInputElement;
    fireEvent.change(tokenInput, { target: { value: 'still-here' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save token' }));

    expect(await screen.findByText('Could not save token')).toBeTruthy();
    expect(tokenInput.value).toBe('still-here');
  });

  it('never renders a stored token value, masked or otherwise', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(publishStatus({ configured: true, repo: 'example/site' })),
    );
    renderSettings();

    await screen.findByText('Yes');
    expect(screen.queryByText(/••••/)).toBeNull();
    expect(screen.queryByText(/\*{4,}/)).toBeNull();
  });
});

describe('Remove token', () => {
  it('cancelling the confirmation does not call clear', async () => {
    renderSettings();
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    expect(await screen.findByText(/Removes the stored GitHub token/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(screen.queryByText(/Removes the stored GitHub token/)).toBeNull());
    expect(commands.clearGithubToken).not.toHaveBeenCalled();
  });

  it('confirming calls clear and toasts success', async () => {
    renderSettings();
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Remove token' }));

    await waitFor(() => expect(commands.clearGithubToken).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('Token removed')).toBeTruthy();
  });
});

describe('Test connection', () => {
  it('shows a pending state while the probe is in flight', async () => {
    let resolvePromise: (value: { status: 'ok'; data: GitHubTestResult }) => void = () => {};
    vi.mocked(commands.testGithubConnection).mockReturnValue(
      new Promise((resolve) => {
        resolvePromise = resolve;
      }),
    );
    renderSettings();

    fireEvent.click(await screen.findByRole('button', { name: 'Test connection' }));
    const pendingButton = (await screen.findByRole('button', {
      name: 'Testing…',
    })) as HTMLButtonElement;
    expect(pendingButton.disabled).toBe(true);

    resolvePromise({ status: 'ok', data: testResult({ ok: true, message: 'connected' }) });
    await screen.findByRole('button', { name: 'Test connection' });
  });

  it('renders the success-toned "connected" result on ok', async () => {
    vi.mocked(commands.testGithubConnection).mockReturnValue(
      ok(testResult({ ok: true, message: 'connected' })),
    );
    renderSettings();

    fireEvent.click(await screen.findByRole('button', { name: 'Test connection' }));

    const message = await screen.findByText('connected');
    expect(message.className).toContain('publish-settings-test-ok');
  });

  it('renders the fixed backend failure message, warning-toned, on !ok', async () => {
    vi.mocked(commands.testGithubConnection).mockReturnValue(
      ok(testResult({ ok: false, message: 'rejected — check token' })),
    );
    renderSettings();

    fireEvent.click(await screen.findByRole('button', { name: 'Test connection' }));

    const message = await screen.findByText('rejected — check token');
    expect(message.className).toContain('publish-settings-test-warn');
  });
});

describe('Publish now', () => {
  it('is disabled while publishing is not configured', async () => {
    renderSettings();

    const publishButton = (await screen.findByRole('button', {
      name: 'Publish now',
    })) as HTMLButtonElement;
    await screen.findByText('No');
    expect(publishButton.disabled).toBe(true);
  });

  it('toasts Published when the publish uploads a new snapshot', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(publishStatus({ configured: true, repo: 'example/site' })),
    );
    vi.mocked(commands.publishNow).mockReturnValue(
      ok(outcome({ status: 'published', digest: 'deadbeef' })),
    );
    renderSettings();

    await screen.findByText('Yes');
    const publishButton = (await screen.findByRole('button', {
      name: 'Publish now',
    })) as HTMLButtonElement;
    await waitFor(() => expect(publishButton.disabled).toBe(false));
    fireEvent.click(publishButton);

    await waitFor(() => expect(commands.publishNow).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('Published')).toBeTruthy();
  });

  it('toasts Already up to date on the unchanged outcome', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(publishStatus({ configured: true, repo: 'example/site' })),
    );
    vi.mocked(commands.publishNow).mockReturnValue(
      ok(outcome({ status: 'unchanged', digest: null })),
    );
    renderSettings();

    await screen.findByText('Yes');
    const publishButton = (await screen.findByRole('button', {
      name: 'Publish now',
    })) as HTMLButtonElement;
    await waitFor(() => expect(publishButton.disabled).toBe(false));
    fireEvent.click(publishButton);

    expect(await screen.findByText('Already up to date')).toBeTruthy();
  });

  it('toasts the failure and refetches status when the publish fails', async () => {
    vi.mocked(commands.getPublishStatus).mockReturnValue(
      ok(publishStatus({ configured: true, repo: 'example/site' })),
    );
    vi.mocked(commands.publishNow).mockReturnValue(
      commandError('github_unreachable', 'network error or timeout'),
    );
    renderSettings();

    await screen.findByText('Yes');
    const publishButton = (await screen.findByRole('button', {
      name: 'Publish now',
    })) as HTMLButtonElement;
    await waitFor(() => expect(publishButton.disabled).toBe(false));
    const statusCallsBefore = vi.mocked(commands.getPublishStatus).mock.calls.length;
    fireEvent.click(publishButton);

    expect(await screen.findByText('Could not publish')).toBeTruthy();
    expect(screen.getByText('network error or timeout')).toBeTruthy();
    // The backend set the pending marker — the status readout must refetch
    // so the pending warning row appears without a manual reload.
    await waitFor(() =>
      expect(vi.mocked(commands.getPublishStatus).mock.calls.length).toBeGreaterThan(
        statusCallsBefore,
      ),
    );
  });
});
