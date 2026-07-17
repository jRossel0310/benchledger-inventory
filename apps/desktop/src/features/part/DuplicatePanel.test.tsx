import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      findMatches: vi.fn(),
      addVariant: vi.fn(),
      recordEquivalence: vi.fn(),
    },
  };
});

const openQuickAction = vi.fn();
vi.mock('../quick/QuickActionContext', () => ({
  useQuickAction: () => ({ open: openQuickAction }),
}));

import type { MatchCandidate, MatchResult, VariantDraft, VariantRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { DuplicatePanel } from './DuplicatePanel';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

const CANDIDATE: MatchCandidate = {
  supplier: null,
  supplier_sku: null,
  manufacturer: null,
  mpn: null,
  category_id: 'c1',
  attributes: [
    ['resistance', '10k'],
    ['tolerance', '1%'],
    ['package', '0603'],
  ],
  package: '0603',
};

function match(overrides: Partial<MatchResult> = {}): MatchResult {
  return {
    part_id: 'p-seed',
    display_name: '10k 0603 1% resistor',
    verdict_kind: 'probable_equivalent',
    explanation: 'Resistance and package agree; power rating is not entered on the candidate.',
    rank: 5,
    ...overrides,
  };
}

const PRIMARY_VARIANT: VariantDraft = {
  manufacturer: 'Yageo',
  mpn: 'RC0603FR-0710KL',
  description: '',
  package: '0603',
  datasheet_url: null,
  product_url: null,
  lifecycle: null,
  notes: '',
};

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(cleanup);

function renderPanel(props: Partial<Parameters<typeof DuplicatePanel>[0]> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const Wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>{children}</ToastProvider>
    </QueryClientProvider>
  );
  return render(<DuplicatePanel candidate={CANDIDATE} primaryVariant={null} {...props} />, {
    wrapper: Wrapper,
  });
}

describe('DuplicatePanel', () => {
  it('renders nothing when the candidate is null (not enough signal yet)', () => {
    const { container } = renderPanel({ candidate: null });
    expect(container.querySelector('.duplicate-panel')).toBeNull();
    expect(commands.findMatches).not.toHaveBeenCalled();
  });

  it('fires find_matches from the candidate and renders the match with its verbatim explanation', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([match()]));
    renderPanel();

    await waitFor(() => expect(commands.findMatches).toHaveBeenCalledWith(CANDIDATE));
    expect(await screen.findByText('10k 0603 1% resistor')).toBeTruthy();
    expect(
      screen.getByText(
        'Resistance and package agree; power rating is not entered on the candidate.',
      ),
    ).toBeTruthy();
    expect(screen.getByText('Probably the same part')).toBeTruthy();
  });

  it('renders nothing when find_matches returns no matches', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([]));
    const { container } = renderPanel();
    await waitFor(() => expect(commands.findMatches).toHaveBeenCalled());
    await waitFor(() => expect(container.querySelector('.duplicate-panel')).toBeNull());
  });

  it('"Add stock to this part" opens the add-stock QuickAction preselected to the match', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([match()]));
    renderPanel();

    fireEvent.click(await screen.findByText('Add stock to this part'));

    expect(openQuickAction).toHaveBeenCalledWith({
      kind: 'receive',
      part: { id: 'p-seed', displayName: '10k 0603 1% resistor' },
    });
  });

  it('offers "Add as a variant" only when a primary variant is available, and calls add_variant', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([match()]));
    vi.mocked(commands.addVariant).mockReturnValue(ok({ id: 'v1' } as unknown as VariantRecord));
    renderPanel({ primaryVariant: PRIMARY_VARIANT });

    fireEvent.click(await screen.findByText('Add as a variant of this part'));

    await waitFor(() =>
      expect(commands.addVariant).toHaveBeenCalledWith('p-seed', PRIMARY_VARIANT),
    );
  });

  it('hides "Add as a variant" when there is no primary variant', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([match()]));
    renderPanel({ primaryVariant: null });

    await screen.findByText('Add stock to this part');
    expect(screen.queryByText('Add as a variant of this part')).toBeNull();
  });

  it('"Create separate part anyway" dismisses the match without any backend call', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([match()]));
    renderPanel();

    fireEvent.click(await screen.findByText('Create separate part anyway'));

    await waitFor(() => expect(screen.queryByText('10k 0603 1% resistor')).toBeNull());
    expect(commands.recordEquivalence).not.toHaveBeenCalled();
  });

  it('does not offer "Not equivalent" in create mode (candidate has no id)', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([match()]));
    renderPanel();

    await screen.findByText('Add stock to this part');
    expect(screen.queryByText('Not equivalent')).toBeNull();
  });

  it('offers "Not equivalent" in edit mode and records a rejected equivalence between the two part ids', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([match()]));
    vi.mocked(commands.recordEquivalence).mockReturnValue(ok(null));
    renderPanel({ currentPartId: 'p-current' });

    fireEvent.click(await screen.findByText('Not equivalent'));

    await waitFor(() =>
      expect(commands.recordEquivalence).toHaveBeenCalledWith(
        'p-current',
        'p-seed',
        'rejected',
        '',
      ),
    );
  });

  it('title-cases an unknown verdict kind rather than dropping the match', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(ok([match({ verdict_kind: 'exact_sku' })]));
    renderPanel();
    expect(await screen.findByText('Same supplier SKU')).toBeTruthy();
  });
});
