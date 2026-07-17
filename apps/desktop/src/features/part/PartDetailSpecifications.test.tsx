import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      getAttributes: vi.fn(),
      categoryAttributeDefs: vi.fn(),
    },
  };
});

import type { AttributeDefRow } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { PartDetailSpecifications } from './PartDetailSpecifications';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function def(overrides: Partial<AttributeDefRow> = {}): AttributeDefRow {
  return {
    key: 'resistance',
    label: 'Resistance',
    data_type: 'number_unit',
    unit_kind: 'resistance',
    identity: true,
    display_order: 1,
    hidden: false,
    choices: [],
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
});

afterEach(cleanup);

function renderSpecs() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <PartDetailSpecifications partId="p1" categoryId="cat-resistor" />
    </QueryClientProvider>,
  );
}

describe('PartDetailSpecifications', () => {
  it('renders each attribute label, original text, and normalized value', async () => {
    vi.mocked(commands.categoryAttributeDefs).mockReturnValue(ok([def()]));
    vi.mocked(commands.getAttributes).mockReturnValue(ok([['resistance', '10k', 10000]]));

    renderSpecs();

    await waitFor(() => expect(screen.getByText('Resistance')).toBeTruthy());
    expect(screen.getByText('10k')).toBeTruthy();
    expect(screen.getByText('10000')).toBeTruthy();
  });

  it('falls back to the raw key when no matching attribute def is loaded', async () => {
    vi.mocked(commands.categoryAttributeDefs).mockReturnValue(ok([]));
    vi.mocked(commands.getAttributes).mockReturnValue(ok([['custom_field', 'foo', null]]));

    renderSpecs();

    await waitFor(() => expect(screen.getByText('custom_field')).toBeTruthy());
    expect(screen.getByText('foo')).toBeTruthy();
    // No numeric normalized value — rendered as an em dash, never blank.
    expect(screen.getByText('—')).toBeTruthy();
  });

  it('shows an empty state when the part has no attribute values set', async () => {
    vi.mocked(commands.categoryAttributeDefs).mockReturnValue(ok([def()]));
    vi.mocked(commands.getAttributes).mockReturnValue(ok([]));

    renderSpecs();

    await waitFor(() => expect(screen.getByText(/no specifications/i)).toBeTruthy());
  });
});
