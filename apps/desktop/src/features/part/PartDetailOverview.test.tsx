import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      getTags: vi.fn(),
    },
  };
});

import type { PartRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { PartDetailOverview } from './PartDetailOverview';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function part(overrides: Partial<PartRecord> = {}): PartRecord {
  return {
    id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_id: 'cat-resistor',
    description: 'A thick-film chip resistor.',
    bin_label: 'A12',
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each',
    low_stock_threshold: 10_000,
    public_notes: 'Buy from Digikey.',
    private_notes: '',
    metadata_complete: true,
    archived: false,
    created_at: '2026-01-01 00:00:00',
    modified_at: '2026-01-02 00:00:00',
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
});

afterEach(cleanup);

function renderOverview(p: PartRecord) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <PartDetailOverview part={p} />
    </QueryClientProvider>,
  );
}

describe('PartDetailOverview', () => {
  it('renders description, usage behavior, and public notes', async () => {
    vi.mocked(commands.getTags).mockReturnValue(ok([]));
    renderOverview(part());

    expect(screen.getByText('A thick-film chip resistor.')).toBeTruthy();
    expect(screen.getByText('Usually consumed')).toBeTruthy();
    expect(screen.getByText('Buy from Digikey.')).toBeTruthy();
  });

  it('renders tags from get_tags', async () => {
    vi.mocked(commands.getTags).mockReturnValue(ok(['smd', 'thick-film']));
    renderOverview(part());

    await waitFor(() => expect(screen.getByText('smd')).toBeTruthy());
    expect(screen.getByText('thick-film')).toBeTruthy();
  });

  it('shows private notes marked local-only, when present', async () => {
    vi.mocked(commands.getTags).mockReturnValue(ok([]));
    renderOverview(part({ private_notes: 'Bought too many, do not reorder.' }));

    expect(screen.getByText('Bought too many, do not reorder.')).toBeTruthy();
    expect(screen.getByText(/local only/i)).toBeTruthy();
  });

  it('omits the private-notes block entirely when there are none', async () => {
    vi.mocked(commands.getTags).mockReturnValue(ok([]));
    renderOverview(part({ private_notes: '' }));

    expect(screen.queryByText(/local only/i)).toBeNull();
  });

  it('shows an inviting empty state for a blank description and no tags', async () => {
    vi.mocked(commands.getTags).mockReturnValue(ok([]));
    renderOverview(part({ description: '' }));

    await waitFor(() => expect(screen.getByText(/no description/i)).toBeTruthy());
    expect(screen.getByText(/no tags/i)).toBeTruthy();
  });
});
