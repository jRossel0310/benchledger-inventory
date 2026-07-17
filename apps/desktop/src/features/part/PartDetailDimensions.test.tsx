import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listDimensions: vi.fn(),
    },
  };
});

import type { DimensionRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { PartDetailDimensions } from './PartDetailDimensions';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function dimension(overrides: Partial<DimensionRecord> = {}): DimensionRecord {
  return {
    id: 'd1',
    part_id: 'p1',
    group: 'overall',
    name: 'Length',
    value_num: 5.1,
    display_unit: 'mm',
    normalized_value: 5.1,
    source: 'datasheet',
    notes: '',
    measured_date: null,
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
});

afterEach(cleanup);

function renderDimensions() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <PartDetailDimensions partId="p1" />
    </QueryClientProvider>,
  );
}

describe('PartDetailDimensions', () => {
  it('renders name, value+unit, group, and source', async () => {
    vi.mocked(commands.listDimensions).mockReturnValue(ok([dimension()]));

    renderDimensions();

    await waitFor(() => expect(screen.getByText('Length')).toBeTruthy());
    expect(screen.getByText('5.1 mm')).toBeTruthy();
    expect(screen.getByText('Overall')).toBeTruthy();
    expect(screen.getByText('Datasheet')).toBeTruthy();
  });

  it('renders notes when present', async () => {
    vi.mocked(commands.listDimensions).mockReturnValue(
      ok([dimension({ notes: 'Measured with calipers.' })]),
    );

    renderDimensions();

    await waitFor(() => expect(screen.getByText('Measured with calipers.')).toBeTruthy());
  });

  it('shows an empty state when there are no dimensions', async () => {
    vi.mocked(commands.listDimensions).mockReturnValue(ok([]));

    renderDimensions();

    await waitFor(() => expect(screen.getByText(/no dimensions/i)).toBeTruthy());
  });
});
