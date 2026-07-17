import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      getSetting: vi.fn(),
      setSetting: vi.fn(),
    },
  };
});

import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { SavedViews } from './SavedViews';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

beforeEach(() => {
  vi.resetAllMocks();
});

afterEach(cleanup);

function renderSavedViews(query: string, onSelect = vi.fn()) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const utils = render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <SavedViews query={query} onSelect={onSelect} />
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { ...utils, onSelect };
}

describe('SavedViews', () => {
  it('renders the built-in presets: All parts, Low stock, Archived', async () => {
    vi.mocked(commands.getSetting).mockReturnValue(ok(null));
    renderSavedViews('');

    await waitFor(() => expect(screen.getByText('All parts')).toBeTruthy());
    expect(screen.getByText('Low stock')).toBeTruthy();
    expect(screen.getByText('Archived')).toBeTruthy();
  });

  it('clicking a preset calls onSelect with its query', async () => {
    vi.mocked(commands.getSetting).mockReturnValue(ok(null));
    const { onSelect } = renderSavedViews('');
    await waitFor(() => expect(screen.getByText('Low stock')).toBeTruthy());

    fireEvent.click(screen.getByText('Low stock'));
    expect(onSelect).toHaveBeenCalledWith('low stock');

    fireEvent.click(screen.getByText('Archived'));
    expect(onSelect).toHaveBeenCalledWith('is:archived');

    fireEvent.click(screen.getByText('All parts'));
    expect(onSelect).toHaveBeenCalledWith('');
  });

  it('renders saved views persisted under the saved_views setting', async () => {
    const saved = JSON.stringify([{ id: 'v1', name: 'My resistors', query: 'category:Resistor' }]);
    vi.mocked(commands.getSetting).mockReturnValue(ok(saved));
    const { onSelect } = renderSavedViews('');

    await waitFor(() => expect(screen.getByText('My resistors')).toBeTruthy());
    fireEvent.click(screen.getByText('My resistors'));
    expect(onSelect).toHaveBeenCalledWith('category:Resistor');
  });

  it('degrades to no saved views when the setting is missing or invalid JSON', async () => {
    vi.mocked(commands.getSetting).mockReturnValue(ok('not json'));
    renderSavedViews('');

    await waitFor(() => expect(screen.getByText('All parts')).toBeTruthy());
    // Only the 3 built-ins render, plus the "save current view" control.
    expect(screen.queryByText('My resistors')).toBeNull();
  });

  it('saving the current view persists the composed query under a new name', async () => {
    vi.mocked(commands.getSetting).mockReturnValue(ok(null));
    vi.mocked(commands.setSetting).mockReturnValue(ok(null));
    renderSavedViews('10k low stock');

    await waitFor(() => expect(screen.getByText('All parts')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: /save current view/i }));
    fireEvent.change(screen.getByPlaceholderText('View name'), {
      target: { value: 'Low 10k parts' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(commands.setSetting).toHaveBeenCalled());
    const [key, value] = vi.mocked(commands.setSetting).mock.calls[0] as [string, string];
    expect(key).toBe('saved_views');
    const persisted = JSON.parse(value) as { id: string; name: string; query: string }[];
    expect(persisted).toHaveLength(1);
    expect(persisted[0]).toMatchObject({ name: 'Low 10k parts', query: '10k low stock' });
    expect(typeof persisted[0]?.id).toBe('string');
  });

  it('removing a saved view persists the list without it', async () => {
    const saved = JSON.stringify([{ id: 'v1', name: 'My resistors', query: 'category:Resistor' }]);
    vi.mocked(commands.getSetting).mockReturnValue(ok(saved));
    vi.mocked(commands.setSetting).mockReturnValue(ok(null));
    renderSavedViews('');

    await waitFor(() => expect(screen.getByText('My resistors')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: /remove saved view my resistors/i }));

    await waitFor(() =>
      expect(commands.setSetting).toHaveBeenCalledWith('saved_views', JSON.stringify([])),
    );
  });
});
