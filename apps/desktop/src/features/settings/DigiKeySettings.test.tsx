import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { DigiKeyStatus, DigiKeyTestResult } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { commands } from '../../lib/commands';
import { DigiKeySettings } from './DigiKeySettings';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

function digikeyStatus(overrides: Partial<DigiKeyStatus> = {}): DigiKeyStatus {
  return { configured: false, environment: 'sandbox', ...overrides };
}

function testResult(overrides: Partial<DigiKeyTestResult> = {}): DigiKeyTestResult {
  return { ok: false, environment: 'sandbox', message: 'not configured', ...overrides };
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
  return render(<DigiKeySettings />, { wrapper: Wrapper });
}

beforeEach(() => {
  vi.spyOn(commands, 'getDigikeyStatus').mockReturnValue(ok(digikeyStatus()));
  vi.spyOn(commands, 'setDigikeyCredentials').mockReturnValue(ok(null));
  vi.spyOn(commands, 'clearDigikeyCredentials').mockReturnValue(ok(null));
  vi.spyOn(commands, 'setDigikeyEnvironment').mockReturnValue(ok(null));
  vi.spyOn(commands, 'testDigikeyConnection').mockReturnValue(ok(testResult()));
});

afterEach(() => {
  vi.restoreAllMocks();
  cleanup();
});

describe('Status card', () => {
  it('shows configured: No and the current environment when unconfigured', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(
      ok(digikeyStatus({ configured: false, environment: 'sandbox' })),
    );
    renderSettings();

    expect(await screen.findByText('No')).toBeTruthy();
    // "Sandbox" appears twice once loaded (status readout + radio label) —
    // scope the status assertion to its <dt>/<dd> pair, mirroring
    // `PartDetailMetadata.test.tsx`'s `valueFor` helper.
    await waitFor(() =>
      expect(screen.getByText('Current environment').nextElementSibling?.textContent).toBe(
        'Sandbox',
      ),
    );
    // Unconfigured: the form reads as first-time setup, not Replace.
    expect(screen.getByRole('button', { name: 'Save credentials' })).toBeTruthy();
  });

  it('shows configured: Yes and offers Replace/Remove when configured', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(
      ok(digikeyStatus({ configured: true, environment: 'production' })),
    );
    renderSettings();

    expect(await screen.findByText('Yes')).toBeTruthy();
    // "Production" appears twice once loaded (status readout + radio label)
    // — scope the status assertion to the <dt>/<dd> pair, mirroring
    // `PartDetailMetadata.test.tsx`'s `valueFor` helper.
    await waitFor(() =>
      expect(screen.getByText('Current environment').nextElementSibling?.textContent).toBe(
        'Production',
      ),
    );
    expect(screen.getByRole('button', { name: 'Replace credentials' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Remove' })).toBeTruthy();
  });

  it('never renders a stored credential value, masked or otherwise', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(
      ok(digikeyStatus({ configured: true, environment: 'production' })),
    );
    renderSettings();

    await screen.findByText('Yes');
    expect(screen.queryByText(/••••/)).toBeNull();
    expect(screen.queryByText(/\*{4,}/)).toBeNull();
  });
});

describe('Credentials form', () => {
  it('disables Save until both Client ID and Client Secret are filled', async () => {
    renderSettings();
    const saveButton = (await screen.findByRole('button', {
      name: 'Save credentials',
    })) as HTMLButtonElement;
    expect(saveButton.disabled).toBe(true);

    fireEvent.change(screen.getByLabelText('Client ID'), { target: { value: 'abc' } });
    expect(saveButton.disabled).toBe(true);

    fireEvent.change(screen.getByLabelText('Client Secret'), { target: { value: 'shh' } });
    expect(saveButton.disabled).toBe(false);

    fireEvent.change(screen.getByLabelText('Client ID'), { target: { value: '' } });
    expect(saveButton.disabled).toBe(true);
  });

  it('saves with the exact entered values, then clears both fields and toasts success', async () => {
    renderSettings();
    const idInput = screen.getByLabelText('Client ID') as HTMLInputElement;
    const secretInput = screen.getByLabelText('Client Secret') as HTMLInputElement;
    expect(secretInput.type).toBe('password');

    fireEvent.change(idInput, { target: { value: 'my-client-id' } });
    fireEvent.change(secretInput, { target: { value: 'my-secret-value' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save credentials' }));

    await waitFor(() =>
      expect(commands.setDigikeyCredentials).toHaveBeenCalledWith(
        'my-client-id',
        'my-secret-value',
      ),
    );
    expect(commands.setDigikeyCredentials).toHaveBeenCalledTimes(1);

    // The crux: both fields are empty again once the save succeeds.
    await waitFor(() => expect(idInput.value).toBe(''));
    expect(secretInput.value).toBe('');
    expect(await screen.findByText('Credentials saved')).toBeTruthy();
  });

  it('never renders the entered secret anywhere in the DOM after saving (DOM-wide)', async () => {
    renderSettings();
    const secret = 'super-secret-value-42';
    fireEvent.change(screen.getByLabelText('Client ID'), { target: { value: 'id-1' } });
    fireEvent.change(screen.getByLabelText('Client Secret'), { target: { value: secret } });
    fireEvent.click(screen.getByRole('button', { name: 'Save credentials' }));

    await screen.findByText('Credentials saved');

    expect(document.body.innerHTML).not.toContain(secret);
    expect(document.body.textContent ?? '').not.toContain(secret);
  });

  it('shows an error toast and does not clear the fields when the save fails', async () => {
    vi.mocked(commands.setDigikeyCredentials).mockReturnValue(
      commandError('invalid_credentials', 'must not be empty'),
    );
    renderSettings();
    const idInput = screen.getByLabelText('Client ID') as HTMLInputElement;
    const secretInput = screen.getByLabelText('Client Secret') as HTMLInputElement;
    fireEvent.change(idInput, { target: { value: 'id-1' } });
    fireEvent.change(secretInput, { target: { value: 'secret-1' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save credentials' }));

    expect(await screen.findByText('Could not save credentials')).toBeTruthy();
    expect(idInput.value).toBe('id-1');
    expect(secretInput.value).toBe('secret-1');
  });
});

describe('Remove', () => {
  beforeEach(() => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(
      ok(digikeyStatus({ configured: true, environment: 'sandbox' })),
    );
  });

  it('cancelling the confirmation does not call clear', async () => {
    renderSettings();
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    expect(await screen.findByText(/Removes the stored DigiKey credentials/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() =>
      expect(screen.queryByText(/Removes the stored DigiKey credentials/)).toBeNull(),
    );
    expect(commands.clearDigikeyCredentials).not.toHaveBeenCalled();
  });

  it('confirming calls clear and toasts success', async () => {
    renderSettings();
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Remove credentials' }));

    await waitFor(() => expect(commands.clearDigikeyCredentials).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('Credentials removed')).toBeTruthy();
  });
});

describe('Environment toggle', () => {
  it('persists the selection via useSetDigiKeyEnvironment and toasts', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(
      ok(digikeyStatus({ configured: true, environment: 'sandbox' })),
    );
    renderSettings();

    const sandboxRadio = (await screen.findByLabelText('Sandbox')) as HTMLInputElement;
    const productionRadio = screen.getByLabelText('Production') as HTMLInputElement;
    // The fieldset stays disabled until `useDigiKeyStatus` resolves (there is
    // no known current environment to check yet) — wait for that first, or
    // a click here would race the fetch and misfire a spurious mutation.
    await waitFor(() => expect(sandboxRadio.disabled).toBe(false));
    expect(sandboxRadio.checked).toBe(true);
    expect(productionRadio.checked).toBe(false);

    fireEvent.click(productionRadio);

    await waitFor(() => expect(commands.setDigikeyEnvironment).toHaveBeenCalledWith('production'));
    expect(await screen.findByText('Environment set to Production')).toBeTruthy();
  });

  it('does not re-submit when clicking the already-selected environment', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(
      ok(digikeyStatus({ configured: true, environment: 'sandbox' })),
    );
    renderSettings();

    const sandboxRadio = (await screen.findByLabelText('Sandbox')) as HTMLInputElement;
    await waitFor(() => expect(sandboxRadio.disabled).toBe(false));
    fireEvent.click(sandboxRadio);

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(commands.setDigikeyEnvironment).not.toHaveBeenCalled();
  });
});

describe('Test connection', () => {
  it('shows a pending state while the probe is in flight', async () => {
    let resolvePromise: (value: { status: 'ok'; data: DigiKeyTestResult }) => void = () => {};
    vi.mocked(commands.testDigikeyConnection).mockReturnValue(
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

    resolvePromise({ status: 'ok', data: testResult({ ok: true, environment: 'production' }) });
    await screen.findByRole('button', { name: 'Test connection' });
  });

  it('renders the success-toned "Connected (Production)" result on ok', async () => {
    vi.mocked(commands.testDigikeyConnection).mockReturnValue(
      ok(testResult({ ok: true, environment: 'production' })),
    );
    renderSettings();

    fireEvent.click(await screen.findByRole('button', { name: 'Test connection' }));

    const message = await screen.findByText('Connected (Production)');
    expect(message.className).toContain('digikey-settings-test-ok');
  });

  it('renders the fixed backend failure message, warning-toned, on !ok', async () => {
    vi.mocked(commands.testDigikeyConnection).mockReturnValue(
      ok(testResult({ ok: false, environment: 'sandbox', message: 'not configured' })),
    );
    renderSettings();

    fireEvent.click(await screen.findByRole('button', { name: 'Test connection' }));

    const message = await screen.findByText('not configured');
    expect(message.className).toContain('digikey-settings-test-warn');
  });
});
