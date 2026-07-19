import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      enrichPartPreview: vi.fn(),
      applyEnrichment: vi.fn(),
      getDigikeyStatus: vi.fn(),
    },
  };
});

import type { DigiKeyStatus, EnrichmentDiff, FieldDiff } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { EnrichmentDiffDialog } from './EnrichmentDiffDialog';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function err(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

const UNPROTECTED_DIFF: FieldDiff = {
  key: 'variant.datasheet_url',
  current: null,
  proposed: 'https://example.com/ds.pdf',
  source: 'digikey',
  current_source: null,
  requires_review: false,
};

// requires_review because current_source is 'manual' — a human typed the
// current description in deliberately.
const MANUAL_PROTECTED_DIFF: FieldDiff = {
  key: 'description',
  current: 'Old hand-written description',
  proposed: 'New DigiKey description',
  source: 'digikey',
  current_source: 'manual',
  requires_review: true,
};

// requires_review because the candidate's own source is 'inferred' AND a
// current value already exists (current_source is NOT 'manual' here).
const INFERRED_PROTECTED_DIFF: FieldDiff = {
  key: 'attr.tolerance',
  current: '5%',
  proposed: '1%',
  source: 'inferred',
  current_source: 'digikey',
  requires_review: true,
};

function diff(overrides: Partial<EnrichmentDiff> = {}): EnrichmentDiff {
  return {
    part_id: 'p1',
    diffs: [UNPROTECTED_DIFF, MANUAL_PROTECTED_DIFF],
    notes: ['Ran DigiKey then description parsing.'],
    provider_summary: ['digikey: 2 fields', 'description: ok'],
    images: [],
    ...overrides,
  };
}

function status(overrides: Partial<DigiKeyStatus> = {}): DigiKeyStatus {
  return { configured: true, environment: 'production', ...overrides };
}

function mockDefaults() {
  vi.mocked(commands.enrichPartPreview).mockReturnValue(ok(diff()));
  vi.mocked(commands.getDigikeyStatus).mockReturnValue(ok(status()));
  vi.mocked(commands.applyEnrichment).mockReturnValue(ok(null));
}

beforeEach(() => {
  vi.resetAllMocks();
});

afterEach(cleanup);

function renderDialog(onClose: () => void = vi.fn()) {
  const rootRoute = createRootRoute();
  const dialogRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: () => <EnrichmentDiffDialog partId="p1" onClose={onClose} />,
  });
  const settingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/settings',
    component: () => <div>Settings stub</div>,
  });
  const routeTree = rootRoute.addChildren([dialogRoute, settingsRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

/** Toggles mounting `EnrichmentDiffDialog` itself — proves the lazy-fetch
 * contract at the level the dialog is actually used from (`PartDetail`
 * conditionally renders it only once "Refresh product data" is clicked). */
function ToggleHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Open
      </button>
      {open ? <EnrichmentDiffDialog partId="p1" onClose={() => setOpen(false)} /> : null}
    </>
  );
}

function renderToggleHarness() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <ToggleHarness />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

function row(labelText: string): HTMLElement {
  return screen.getByText(labelText).closest('li') as HTMLElement;
}

describe('EnrichmentDiffDialog', () => {
  it('never calls enrich_part_preview until the dialog is actually mounted/opened', async () => {
    mockDefaults();
    renderToggleHarness();

    expect(commands.enrichPartPreview).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Open' }));

    await waitFor(() => expect(commands.enrichPartPreview).toHaveBeenCalledWith('p1'));
  });

  it('shows a loading state while the preview is pending', async () => {
    mockDefaults();
    vi.mocked(commands.enrichPartPreview).mockReturnValue(new Promise(() => {}));
    renderDialog();

    await waitFor(() => expect(screen.getByText(/loading/i)).toBeTruthy());
  });

  it('shows the CommandError message when the preview fails', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(ok(status()));
    vi.mocked(commands.enrichPartPreview).mockReturnValue(err('sqlite', 'db is locked'));
    renderDialog();

    await waitFor(() => expect(screen.getByText(/db is locked/)).toBeTruthy());
  });

  it('shows not-configured guidance with a link to Settings, and still lists description candidates', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(ok(status({ configured: false })));
    vi.mocked(commands.enrichPartPreview).mockReturnValue(
      ok(diff({ diffs: [{ ...UNPROTECTED_DIFF, source: 'inferred' }] })),
    );
    renderDialog();

    await waitFor(() => expect(screen.getByText(/isn.t configured/i)).toBeTruthy());
    expect(screen.getByRole('link', { name: /settings/i }).getAttribute('href')).toBe('/settings');
    expect(screen.getByText('Datasheet URL')).toBeTruthy();
  });

  it('does not show the not-configured guidance when DigiKey is configured', async () => {
    mockDefaults();
    renderDialog();

    await waitFor(() => expect(screen.getByText('Datasheet URL')).toBeTruthy());
    expect(screen.queryByText(/isn.t configured/i)).toBeNull();
  });

  it('an unprotected row: checking include and applying calls apply with acknowledge_review false', async () => {
    mockDefaults();
    renderDialog();

    await waitFor(() => expect(screen.getByText('Datasheet URL')).toBeTruthy());
    const scoped = within(row('Datasheet URL'));
    fireEvent.click(scoped.getByRole('checkbox', { name: /include/i }));

    fireEvent.click(screen.getByRole('button', { name: /^apply/i }));

    await waitFor(() =>
      expect(commands.applyEnrichment).toHaveBeenCalledWith('p1', [
        {
          key: 'variant.datasheet_url',
          value: 'https://example.com/ds.pdf',
          source: 'digikey',
          acknowledge_review: false,
        },
      ]),
    );
  });

  it('a protected row (manual current) checked but NOT confirmed is excluded from apply, and Apply is disabled when it is the only row', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(ok(status()));
    vi.mocked(commands.enrichPartPreview).mockReturnValue(
      ok(diff({ diffs: [MANUAL_PROTECTED_DIFF] })),
    );
    renderDialog();

    await waitFor(() => expect(screen.getByText('Description')).toBeTruthy());
    const scoped = within(row('Description'));
    expect(scoped.getByText(/overwrite manually-set value/i)).toBeTruthy();

    fireEvent.click(scoped.getByRole('checkbox', { name: /include/i }));

    expect((screen.getByRole('button', { name: /^apply/i }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect(scoped.getByText(/confirm to include this change/i)).toBeTruthy();
  });

  it('a protected row (manual current): include + explicit confirm arms it with acknowledge_review true', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(ok(status()));
    vi.mocked(commands.enrichPartPreview).mockReturnValue(
      ok(diff({ diffs: [MANUAL_PROTECTED_DIFF] })),
    );
    renderDialog();

    await waitFor(() => expect(screen.getByText('Description')).toBeTruthy());
    const scoped = within(row('Description'));
    fireEvent.click(scoped.getByRole('checkbox', { name: /include/i }));
    fireEvent.click(scoped.getByRole('checkbox', { name: /overwrite manually-set value/i }));

    fireEvent.click(screen.getByRole('button', { name: /^apply/i }));

    await waitFor(() =>
      expect(commands.applyEnrichment).toHaveBeenCalledWith('p1', [
        {
          key: 'description',
          value: 'New DigiKey description',
          source: 'digikey',
          acknowledge_review: true,
        },
      ]),
    );
  });

  it('a protected row (inferred over existing) shows the "Accept inferred over existing" confirmation', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(ok(status()));
    vi.mocked(commands.enrichPartPreview).mockReturnValue(
      ok(diff({ diffs: [INFERRED_PROTECTED_DIFF] })),
    );
    renderDialog();

    await waitFor(() => expect(screen.getByText('Tolerance')).toBeTruthy());
    const scoped = within(row('Tolerance'));
    expect(scoped.getByText(/accept inferred over existing/i)).toBeTruthy();
    expect(scoped.queryByText(/overwrite manually-set value/i)).toBeNull();
  });

  it("unchecking include also resets a protected row's confirmation", async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(ok(status()));
    vi.mocked(commands.enrichPartPreview).mockReturnValue(
      ok(diff({ diffs: [MANUAL_PROTECTED_DIFF] })),
    );
    renderDialog();

    await waitFor(() => expect(screen.getByText('Description')).toBeTruthy());
    const scoped = within(row('Description'));
    const include = scoped.getByRole('checkbox', { name: /include/i });
    const confirm = scoped.getByRole('checkbox', {
      name: /overwrite manually-set value/i,
    }) as HTMLInputElement;

    fireEvent.click(include);
    fireEvent.click(confirm);
    expect(confirm.checked).toBe(true);

    fireEvent.click(include); // uncheck
    fireEvent.click(include); // re-check
    expect(
      (scoped.getByRole('checkbox', { name: /overwrite manually-set value/i }) as HTMLInputElement)
        .checked,
    ).toBe(false);
  });

  it('select-all arms only unprotected rows, leaving protected rows untouched', async () => {
    vi.mocked(commands.getDigikeyStatus).mockReturnValue(ok(status()));
    vi.mocked(commands.enrichPartPreview).mockReturnValue(
      ok(diff({ diffs: [UNPROTECTED_DIFF, MANUAL_PROTECTED_DIFF] })),
    );
    renderDialog();

    await waitFor(() => expect(screen.getByText('Datasheet URL')).toBeTruthy());
    fireEvent.click(screen.getByRole('checkbox', { name: /select all/i }));

    expect(
      (
        within(row('Datasheet URL')).getByRole('checkbox', {
          name: /include/i,
        }) as HTMLInputElement
      ).checked,
    ).toBe(true);
    expect(
      (
        within(row('Description')).getByRole('checkbox', {
          name: /include/i,
        }) as HTMLInputElement
      ).checked,
    ).toBe(false);
  });

  it('renders an image thumbnail strip when diff.images is non-empty', async () => {
    mockDefaults();
    vi.mocked(commands.enrichPartPreview).mockReturnValue(
      ok(diff({ images: ['https://example.com/a.jpg', 'https://example.com/b.jpg'] })),
    );
    renderDialog();

    await waitFor(() => expect(screen.getAllByRole('img').length).toBe(2));
  });

  it('renders no image strip when diff.images is empty', async () => {
    mockDefaults();
    renderDialog();

    await waitFor(() => expect(screen.getByText('Datasheet URL')).toBeTruthy());
    expect(screen.queryAllByRole('img').length).toBe(0);
  });

  it('a broken image degrades silently (is removed rather than showing a broken-image icon)', async () => {
    mockDefaults();
    vi.mocked(commands.enrichPartPreview).mockReturnValue(
      ok(diff({ images: ['https://example.com/broken.jpg'] })),
    );
    renderDialog();

    await waitFor(() => expect(screen.getAllByRole('img').length).toBe(1));
    fireEvent.error(screen.getByRole('img'));
    await waitFor(() => expect(screen.queryAllByRole('img').length).toBe(0));
  });

  it('shows notes and provider_summary in a collapsed footer', async () => {
    mockDefaults();
    renderDialog();

    await waitFor(() => expect(screen.getByText('Datasheet URL')).toBeTruthy());
    expect(screen.getByText('Ran DigiKey then description parsing.')).toBeTruthy();
    expect(screen.getByText('digikey: 2 fields')).toBeTruthy();
  });

  it('applies armed rows, toasts "Applied N fields", and closes the dialog', async () => {
    mockDefaults();
    const onClose = vi.fn();
    renderDialog(onClose);

    await waitFor(() => expect(screen.getByText('Datasheet URL')).toBeTruthy());
    fireEvent.click(within(row('Datasheet URL')).getByRole('checkbox', { name: /include/i }));
    fireEvent.click(screen.getByRole('button', { name: /^apply/i }));

    await waitFor(() => expect(screen.getByText('Applied 1 field')).toBeTruthy());
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it('an enrichment_review_required apply error shows a plain message and keeps the dialog open with nothing applied', async () => {
    mockDefaults();
    vi.mocked(commands.applyEnrichment).mockReturnValue(
      err('enrichment_review_required', "field 'description' requires review"),
    );
    const onClose = vi.fn();
    renderDialog(onClose);

    await waitFor(() => expect(screen.getByText('Datasheet URL')).toBeTruthy());
    fireEvent.click(within(row('Datasheet URL')).getByRole('checkbox', { name: /include/i }));
    fireEvent.click(screen.getByRole('button', { name: /^apply/i }));

    await waitFor(() => expect(screen.getByText(/nothing was saved/i)).toBeTruthy());
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog')).toBeTruthy();
  });
});
