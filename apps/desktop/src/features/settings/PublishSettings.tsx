/**
 * Settings' publishing section (Phase 6 Task 5): the only UI surface that
 * ever collects the GitHub publish token. Five pieces, in the order they
 * read naturally — status, repository config, token, test, publish:
 *
 * - Status card: `usePublishStatus()`'s configured/repo/last-published/
 *   pending/Vercel URL — never the token (`PublishStatus` in
 *   `bindings.gen.ts` has no field that could carry one). The pending row
 *   carries a Retry button (`useRetryPendingPublish`); either outcome
 *   refetches status so the warning clears or persists honestly.
 * - Repository form: owner + repo (required), branch/path (blank submits
 *   as `""` — the backend fills `main` / the default snapshot path; the
 *   defaults appear as placeholders only, never as submitted values), and
 *   an optional Vercel URL (blank clears the stored one via `null`).
 * - Token form: `type="password"`, write-only (`set_github_token` returns
 *   nothing — `hooks/publish.ts`), cleared from local `useState` the
 *   instant the save succeeds; nothing ever re-populates it. Remove goes
 *   through a confirm dialog into `useClearGitHubToken` — clearing is
 *   idempotent server-side, so the button shows regardless of whether a
 *   token is currently stored (status deliberately doesn't say).
 * - Test connection: `useTestGitHubConnection()` — a single contents-GET
 *   probe, surfaced inline as the backend's fixed strings; never a
 *   response body or the token.
 * - Publish now: `usePublishNow()` — "Published" / "Already up to date"
 *   toast; a failure toasts the message and refetches status, because the
 *   backend has just set the pending marker.
 */

import * as Dialog from '@radix-ui/react-dialog';
import { useQueryClient } from '@tanstack/react-query';
import { useId, useState, type FormEvent } from 'react';

import { useToast } from '../../components/Toast';
import {
  keys,
  useClearGitHubToken,
  usePublishNow,
  usePublishStatus,
  useRetryPendingPublish,
  useSetGitHubToken,
  useSetPublishConfig,
  useTestGitHubConnection,
} from '../../hooks/publish';
import { errorHint, errorMessage, formatTimestamp } from '../../lib/format';
import './PublishSettings.css';

/** Mirror of `inventory_sync::publish::DEFAULT_BRANCH` — placeholder only,
 * never submitted (a blank field submits `""` and the backend defaults). */
const DEFAULT_BRANCH_PLACEHOLDER = 'main';
/** Mirror of `inventory_sync::publish::DEFAULT_PATH` — placeholder only. */
const DEFAULT_PATH_PLACEHOLDER = 'apps/web/public/inventory.snapshot.json';

export function PublishSettings() {
  const { toast } = useToast();
  const queryClient = useQueryClient();
  const statusQuery = usePublishStatus();

  const [owner, setOwner] = useState('');
  const [repo, setRepo] = useState('');
  const [branch, setBranch] = useState('');
  const [path, setPath] = useState('');
  const [vercelUrl, setVercelUrl] = useState('');
  const [token, setToken] = useState('');
  const [confirmingRemove, setConfirmingRemove] = useState(false);

  const ownerInputId = useId();
  const repoInputId = useId();
  const branchInputId = useId();
  const pathInputId = useId();
  const vercelUrlInputId = useId();
  const tokenInputId = useId();

  function refetchStatus() {
    void queryClient.invalidateQueries({ queryKey: keys.publishStatus });
  }

  const setConfig = useSetPublishConfig({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not save publish settings',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Publish settings saved', kind: 'success' });
      // A test result from before the config change now describes the wrong
      // target — stale, and misleading if left on screen.
      testConnection.reset();
    },
  });

  const setGitHubToken = useSetGitHubToken({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not save token',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      // The crux: clear the secret-bearing field the instant the write
      // succeeds. Nothing downstream of this point ever holds the value
      // again.
      setToken('');
      toast({ title: 'Token saved', kind: 'success' });
    },
  });

  const clearToken = useClearGitHubToken({
    onDone: (error) => {
      setConfirmingRemove(false);
      if (error) {
        toast({
          title: 'Could not remove token',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Token removed', kind: 'success' });
    },
  });

  const testConnection = useTestGitHubConnection();

  const publishNow = usePublishNow({
    onDone: (error, outcome) => {
      if (error) {
        toast({
          title: 'Could not publish',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        // The backend has just set the pending marker — refetch so the
        // pending warning row appears without a manual reload.
        refetchStatus();
        return;
      }
      toast({
        title: outcome?.status === 'unchanged' ? 'Already up to date' : 'Published',
        kind: 'success',
      });
    },
  });

  const retryPending = useRetryPendingPublish({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not publish',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        // Either way status refetches — on failure the pending flag stays,
        // and the readout should keep saying so from fresh data.
        refetchStatus();
        return;
      }
      toast({ title: 'Published', kind: 'success' });
    },
  });

  const status = statusQuery.data;
  const configured = status?.configured ?? false;

  const canSaveConfig = owner.trim().length > 0 && repo.trim().length > 0 && !setConfig.isPending;
  const canSaveToken = token.trim().length > 0 && !setGitHubToken.isPending;

  function handleConfigSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSaveConfig) return;
    const trimmedVercelUrl = vercelUrl.trim();
    setConfig.mutate({
      owner: owner.trim(),
      repo: repo.trim(),
      // Blank submits as "" — the backend fills DEFAULT_BRANCH/DEFAULT_PATH.
      branch: branch.trim(),
      path: path.trim(),
      vercelUrl: trimmedVercelUrl.length > 0 ? trimmedVercelUrl : null,
    });
  }

  function handleTokenSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSaveToken) return;
    setGitHubToken.mutate(token);
  }

  return (
    <div className="publish-settings">
      <section className="publish-settings-section">
        <h2 className="publish-settings-section-title">Publishing status</h2>
        {statusQuery.isPending ? (
          <p className="publish-settings-muted">Loading status…</p>
        ) : statusQuery.isError ? (
          <p className="publish-settings-error">
            Could not load publish status: {errorMessage(statusQuery.error)}
          </p>
        ) : (
          <>
            <dl className="publish-settings-status-list">
              <div>
                <dt>Configured</dt>
                <dd>{configured ? 'Yes' : 'No'}</dd>
              </div>
              <div>
                <dt>Repository</dt>
                <dd>{status?.repo ?? '—'}</dd>
              </div>
              <div>
                <dt>Last published</dt>
                <dd>
                  {status?.last_published_at ? formatTimestamp(status.last_published_at) : 'Never'}
                </dd>
              </div>
              {status?.vercel_url ? (
                <div>
                  <dt>Site</dt>
                  <dd>
                    <a
                      href={status.vercel_url}
                      target="_blank"
                      rel="noreferrer"
                      className="publish-settings-link"
                    >
                      {status.vercel_url}
                    </a>
                  </dd>
                </div>
              ) : null}
            </dl>
            {status?.pending ? (
              <div className="publish-settings-pending-row">
                <span className="publish-settings-pending-text">
                  Publish pending — will retry on launch
                </span>
                <button
                  type="button"
                  className="publish-settings-secondary-button"
                  onClick={() => retryPending.mutate()}
                  disabled={retryPending.isPending}
                >
                  {retryPending.isPending ? 'Retrying…' : 'Retry'}
                </button>
              </div>
            ) : null}
          </>
        )}
      </section>

      <section className="publish-settings-section">
        <h2 className="publish-settings-section-title">Publish repository</h2>
        <p className="publish-settings-hint">
          The public GitHub repository the inventory snapshot is published to. Leave branch and path
          blank to use the defaults shown.
        </p>
        <form className="publish-settings-form" onSubmit={handleConfigSubmit}>
          <div className="field">
            <label className="field-label" htmlFor={ownerInputId}>
              Owner
            </label>
            <input
              id={ownerInputId}
              type="text"
              className="field-input"
              autoComplete="off"
              value={owner}
              disabled={setConfig.isPending}
              onChange={(event) => setOwner(event.target.value)}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor={repoInputId}>
              Repository name
            </label>
            <input
              id={repoInputId}
              type="text"
              className="field-input"
              autoComplete="off"
              value={repo}
              disabled={setConfig.isPending}
              onChange={(event) => setRepo(event.target.value)}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor={branchInputId}>
              Branch
            </label>
            <input
              id={branchInputId}
              type="text"
              className="field-input"
              autoComplete="off"
              placeholder={DEFAULT_BRANCH_PLACEHOLDER}
              value={branch}
              disabled={setConfig.isPending}
              onChange={(event) => setBranch(event.target.value)}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor={pathInputId}>
              Snapshot path
            </label>
            <input
              id={pathInputId}
              type="text"
              className="field-input"
              autoComplete="off"
              placeholder={DEFAULT_PATH_PLACEHOLDER}
              value={path}
              disabled={setConfig.isPending}
              onChange={(event) => setPath(event.target.value)}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor={vercelUrlInputId}>
              Site URL (optional)
            </label>
            <input
              id={vercelUrlInputId}
              type="text"
              className="field-input"
              autoComplete="off"
              value={vercelUrl}
              disabled={setConfig.isPending}
              onChange={(event) => setVercelUrl(event.target.value)}
            />
          </div>
          <div className="publish-settings-form-buttons">
            <button
              type="submit"
              className="publish-settings-primary-button"
              disabled={!canSaveConfig}
            >
              {setConfig.isPending ? 'Saving…' : 'Save publish settings'}
            </button>
          </div>
        </form>
      </section>

      <section className="publish-settings-section">
        <h2 className="publish-settings-section-title">GitHub token</h2>
        <p className="publish-settings-hint">
          A fine-grained personal access token with Contents read/write on the publish repository
          only — stored in Windows Credential Manager, never in this app’s database.
        </p>
        <form className="publish-settings-form" onSubmit={handleTokenSubmit}>
          <div className="field">
            <label className="field-label" htmlFor={tokenInputId}>
              GitHub token
            </label>
            <input
              id={tokenInputId}
              type="password"
              className="field-input"
              autoComplete="off"
              value={token}
              disabled={setGitHubToken.isPending}
              onChange={(event) => setToken(event.target.value)}
            />
          </div>
          <div className="publish-settings-form-buttons">
            <button
              type="submit"
              className="publish-settings-primary-button"
              disabled={!canSaveToken}
            >
              {setGitHubToken.isPending ? 'Saving…' : 'Save token'}
            </button>
            <button
              type="button"
              className="publish-settings-secondary-button"
              onClick={() => setConfirmingRemove(true)}
              disabled={clearToken.isPending}
            >
              Remove
            </button>
          </div>
        </form>
      </section>

      <section className="publish-settings-section">
        <h2 className="publish-settings-section-title">Test connection</h2>
        <button
          type="button"
          className="publish-settings-secondary-button"
          onClick={() => testConnection.mutate()}
          disabled={testConnection.isPending}
        >
          {testConnection.isPending ? 'Testing…' : 'Test connection'}
        </button>
        {testConnection.data ? (
          <p
            className={
              testConnection.data.ok
                ? 'publish-settings-test-result publish-settings-test-ok'
                : 'publish-settings-test-result publish-settings-test-warn'
            }
          >
            {testConnection.data.message}
          </p>
        ) : testConnection.isError ? (
          <p className="publish-settings-test-result publish-settings-test-warn">
            {errorMessage(testConnection.error)}
          </p>
        ) : null}
      </section>

      <section className="publish-settings-section">
        <h2 className="publish-settings-section-title">Publish</h2>
        <button
          type="button"
          className="publish-settings-primary-button"
          onClick={() => publishNow.mutate()}
          disabled={!configured || publishNow.isPending}
        >
          {publishNow.isPending ? 'Publishing…' : 'Publish now'}
        </button>
        <p className="publish-settings-hint">
          Uploads the current inventory snapshot to the configured repository. Skipped automatically
          when nothing has changed since the last publish.
        </p>
      </section>

      {confirmingRemove ? (
        <Dialog.Root open onOpenChange={(open) => !open && setConfirmingRemove(false)}>
          <Dialog.Portal>
            <Dialog.Overlay className="publish-settings-dialog-overlay" />
            <Dialog.Content className="publish-settings-dialog-content">
              <Dialog.Title className="publish-settings-dialog-title">
                Remove GitHub token?
              </Dialog.Title>
              <Dialog.Description className="publish-settings-dialog-description">
                Removes the stored GitHub token from Windows Credential Manager. Publishing stops
                until a new token is saved.
              </Dialog.Description>
              <div className="publish-settings-dialog-buttons">
                <button
                  type="button"
                  className="publish-settings-secondary-button"
                  onClick={() => setConfirmingRemove(false)}
                  disabled={clearToken.isPending}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  className="publish-settings-primary-button"
                  onClick={() => clearToken.mutate()}
                  disabled={clearToken.isPending}
                >
                  {clearToken.isPending ? 'Removing…' : 'Remove token'}
                </button>
              </div>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      ) : null}
    </div>
  );
}
