/**
 * Settings' DigiKey section (Phase 5d Task 6): the only UI surface in the
 * app that ever collects a DigiKey secret. Four pieces, in the order they
 * read naturally — status, credentials, environment, test:
 *
 * - Status card: `useDigiKeyStatus()`'s `configured`/`environment` — a
 *   plain boolean and a non-secret string, never a credential (see the doc
 *   comment on `DigiKeyStatus` in `bindings.gen.ts`). There is nothing to
 *   render for a stored credential — no masked `••••` placeholder either,
 *   since that would fake a value the backend never returns.
 * - Credentials form: Client ID (plain text) + Client Secret
 *   (`type="password"`). `set_digikey_credentials` is write-only — it
 *   returns nothing (`hooks/enrichment.ts`) — so `clientId`/`clientSecret`
 *   cross IPC exactly once and this component clears both fields from
 *   local `useState` the instant the mutation succeeds. Nothing ever
 *   persists them beyond that (no draft storage, no effect that
 *   re-populates them from anywhere) — the same local `useState` also
 *   empties itself for free the moment this component unmounts. When
 *   already configured, the same form doubles as Replace — the button
 *   label says so.
 * - Environment toggle: sandbox/production radios, current value from
 *   `useDigiKeyStatus()`, `useSetDigiKeyEnvironment()` on change.
 * - Test connection: `useTestDigiKeyConnection()` — an OAuth2 token-fetch
 *   probe only, never a product call — surfaced inline (fixed strings the
 *   backend returns; never a response body or a credential).
 */

import * as Dialog from '@radix-ui/react-dialog';
import { useId, useRef, useState, type FormEvent } from 'react';

import { useToast } from '../../components/Toast';
import {
  useClearDigiKeyCredentials,
  useDigiKeyStatus,
  useSetDigiKeyCredentials,
  useSetDigiKeyEnvironment,
  useTestDigiKeyConnection,
} from '../../hooks/enrichment';
import { errorHint, errorMessage } from '../../lib/format';
import './DigiKeySettings.css';

const ENVIRONMENTS = [
  { value: 'sandbox', label: 'Sandbox' },
  { value: 'production', label: 'Production' },
] as const;

function environmentLabel(value: string): string {
  return ENVIRONMENTS.find((env) => env.value === value)?.label ?? value;
}

export function DigiKeySettings() {
  const { toast } = useToast();
  const statusQuery = useDigiKeyStatus();

  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const clientIdInputId = useId();
  const clientSecretInputId = useId();
  const envGroupName = useId();

  // The environment `useSetDigiKeyEnvironment`'s `onDone` callback (which
  // only receives error/data, not the variables that were submitted — see
  // `MutationCallbacks`) is currently applying, for the success toast copy.
  // Mirrors `RowActions.tsx`'s `AssignBinDialog` `pendingLabelRef` pattern.
  const pendingEnvironmentRef = useRef<string | null>(null);

  const setCredentials = useSetDigiKeyCredentials({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not save credentials',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      // The crux: clear the secret-bearing fields the instant the write
      // succeeds. Nothing downstream of this point ever holds the values
      // again.
      setClientId('');
      setClientSecret('');
      toast({ title: 'Credentials saved', kind: 'success' });
    },
  });

  const clearCredentials = useClearDigiKeyCredentials({
    onDone: (error) => {
      setConfirmingRemove(false);
      if (error) {
        toast({
          title: 'Could not remove credentials',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Credentials removed', kind: 'success' });
    },
  });

  const setEnvironment = useSetDigiKeyEnvironment({
    onDone: (error) => {
      const environment = pendingEnvironmentRef.current;
      if (error) {
        toast({
          title: 'Could not change environment',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({
        title: `Environment set to ${environment ? environmentLabel(environment) : 'the new value'}`,
        kind: 'success',
      });
      // A test result from before the switch is now against the wrong
      // environment — stale, and misleading if left on screen.
      testConnection.reset();
    },
  });

  const testConnection = useTestDigiKeyConnection();

  const status = statusQuery.data;
  const configured = status?.configured ?? false;
  const currentEnvironment = status?.environment ?? null;

  const canSave =
    clientId.trim().length > 0 && clientSecret.trim().length > 0 && !setCredentials.isPending;

  function handleSaveSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSave) return;
    setCredentials.mutate({ clientId, clientSecret });
  }

  function handleEnvironmentChange(value: string) {
    if (value === currentEnvironment) return;
    pendingEnvironmentRef.current = value;
    setEnvironment.mutate(value);
  }

  return (
    <div className="digikey-settings">
      <section className="digikey-settings-section">
        <h2 className="digikey-settings-section-title">Status</h2>
        {statusQuery.isPending ? (
          <p className="digikey-settings-muted">Loading status…</p>
        ) : statusQuery.isError ? (
          <p className="digikey-settings-error">
            Could not load DigiKey status: {errorMessage(statusQuery.error)}
          </p>
        ) : (
          <dl className="digikey-settings-status-list">
            <div>
              <dt>Configured</dt>
              <dd>{configured ? 'Yes' : 'No'}</dd>
            </div>
            <div>
              <dt>Current environment</dt>
              <dd>{currentEnvironment ? environmentLabel(currentEnvironment) : '—'}</dd>
            </div>
          </dl>
        )}
      </section>

      <section className="digikey-settings-section">
        <h2 className="digikey-settings-section-title">
          {configured ? 'Replace credentials' : 'Credentials'}
        </h2>
        <p className="digikey-settings-hint">
          {configured
            ? 'Enter a new Client ID and Client Secret to replace the credentials currently stored in Windows Credential Manager.'
            : 'DigiKey Client ID and Client Secret from your registered app at developer.digikey.com — stored in Windows Credential Manager, never in this app’s database.'}
        </p>
        <form className="digikey-settings-form" onSubmit={handleSaveSubmit}>
          <div className="field">
            <label className="field-label" htmlFor={clientIdInputId}>
              Client ID
            </label>
            <input
              id={clientIdInputId}
              type="text"
              className="field-input"
              autoComplete="off"
              value={clientId}
              disabled={setCredentials.isPending}
              onChange={(event) => setClientId(event.target.value)}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor={clientSecretInputId}>
              Client Secret
            </label>
            <input
              id={clientSecretInputId}
              type="password"
              className="field-input"
              autoComplete="off"
              value={clientSecret}
              disabled={setCredentials.isPending}
              onChange={(event) => setClientSecret(event.target.value)}
            />
          </div>
          <div className="digikey-settings-form-buttons">
            <button type="submit" className="digikey-settings-primary-button" disabled={!canSave}>
              {setCredentials.isPending
                ? 'Saving…'
                : configured
                  ? 'Replace credentials'
                  : 'Save credentials'}
            </button>
            {configured ? (
              <button
                type="button"
                className="digikey-settings-secondary-button"
                onClick={() => setConfirmingRemove(true)}
                disabled={clearCredentials.isPending}
              >
                Remove
              </button>
            ) : null}
          </div>
        </form>
      </section>

      <section className="digikey-settings-section">
        <h2 className="digikey-settings-section-title">Environment</h2>
        <fieldset
          className="digikey-settings-env-fieldset"
          disabled={setEnvironment.isPending || !status}
        >
          <legend className="digikey-settings-visually-hidden">DigiKey environment</legend>
          {ENVIRONMENTS.map((env) => (
            <label key={env.value} className="digikey-settings-env-option">
              <input
                type="radio"
                name={envGroupName}
                value={env.value}
                checked={currentEnvironment === env.value}
                onChange={() => handleEnvironmentChange(env.value)}
              />
              {env.label}
            </label>
          ))}
        </fieldset>
        <p className="digikey-settings-hint">
          Production apps registered at developer.digikey.com work only against the production
          environment. Sandbox requires separate sandbox-app credentials — a “rejected” test result
          against the wrong environment is expected, not a broken key.
        </p>
      </section>

      <section className="digikey-settings-section">
        <h2 className="digikey-settings-section-title">Test connection</h2>
        <button
          type="button"
          className="digikey-settings-secondary-button"
          onClick={() => testConnection.mutate()}
          disabled={testConnection.isPending}
        >
          {testConnection.isPending ? 'Testing…' : 'Test connection'}
        </button>
        {testConnection.data ? (
          <p
            className={
              testConnection.data.ok
                ? 'digikey-settings-test-result digikey-settings-test-ok'
                : 'digikey-settings-test-result digikey-settings-test-warn'
            }
          >
            {testConnection.data.ok
              ? `Connected (${environmentLabel(testConnection.data.environment)})`
              : testConnection.data.message}
          </p>
        ) : testConnection.isError ? (
          <p className="digikey-settings-test-result digikey-settings-test-warn">
            {errorMessage(testConnection.error)}
          </p>
        ) : null}
      </section>

      {confirmingRemove ? (
        <Dialog.Root open onOpenChange={(open) => !open && setConfirmingRemove(false)}>
          <Dialog.Portal>
            <Dialog.Overlay className="digikey-settings-dialog-overlay" />
            <Dialog.Content className="digikey-settings-dialog-content">
              <Dialog.Title className="digikey-settings-dialog-title">
                Remove DigiKey credentials?
              </Dialog.Title>
              <Dialog.Description className="digikey-settings-dialog-description">
                Removes the stored DigiKey credentials from Windows Credential Manager. Enrichment
                falls back to description parsing.
              </Dialog.Description>
              <div className="digikey-settings-dialog-buttons">
                <button
                  type="button"
                  className="digikey-settings-secondary-button"
                  onClick={() => setConfirmingRemove(false)}
                  disabled={clearCredentials.isPending}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  className="digikey-settings-primary-button"
                  onClick={() => clearCredentials.mutate()}
                  disabled={clearCredentials.isPending}
                >
                  {clearCredentials.isPending ? 'Removing…' : 'Remove credentials'}
                </button>
              </div>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      ) : null}
    </div>
  );
}
