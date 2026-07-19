import { parseSnapshot } from '@ei/shared';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import sample from '../../../packages/shared/fixtures/sample-snapshot.json';
import { App } from './App';
import type { SnapshotState } from './snapshot';

const snapshot = parseSnapshot(sample);
if (snapshot === null) throw new Error('sample-snapshot.json failed to parse');

afterEach(() => {
  cleanup();
  window.location.hash = '';
});

const loaded: SnapshotState = { kind: 'loaded', snapshot };

function renderApp(state: SnapshotState | 'pending') {
  const load = () =>
    state === 'pending' ? new Promise<SnapshotState>(() => {}) : Promise.resolve(state);
  return render(<App load={load} />);
}

describe('App', () => {
  it('shows a loading state while the snapshot is being fetched', () => {
    renderApp('pending');
    expect(screen.getByText('Loading…')).toBeTruthy();
  });

  it('shows the empty state when no snapshot has been published', async () => {
    renderApp({ kind: 'none' });
    await screen.findByText('No snapshot published yet');
  });

  it('shows the invalid state for an unreadable snapshot', async () => {
    renderApp({ kind: 'invalid' });
    await screen.findByText('Snapshot could not be read');
  });

  it('shows the read-only banner with the published timestamp', async () => {
    renderApp(loaded);
    const banner = await screen.findByText(/Read-only inventory snapshot/);
    expect(banner.textContent).toContain('last published 2026-01-01T00:00:00Z');
  });

  it('renders the table and result count once loaded', async () => {
    renderApp(loaded);
    await screen.findByText('10k 0603 1% resistor');
    expect(screen.getByText(`${snapshot.parts.length} parts`)).toBeTruthy();
  });

  it('filters the table from the (debounced) search input', async () => {
    renderApp(loaded);
    const input = await screen.findByRole('searchbox');
    fireEvent.change(input, { target: { value: 'bin:a4' } });
    await waitFor(() => {
      expect(screen.getByText('1 part')).toBeTruthy();
    });
    expect(screen.getByText('100R 0603 1% resistor')).toBeTruthy();
    expect(screen.queryByText('10k 0603 1% resistor')).toBeNull();
  });

  it('shows a chip for unsupported filters instead of dropping them silently', async () => {
    renderApp(loaded);
    const input = await screen.findByRole('searchbox');
    fireEvent.change(input, { target: { value: 'project:amp' } });
    await waitFor(() => {
      expect(screen.getByText('project:amp')).toBeTruthy();
    });
    expect(screen.getByText(/[Uu]nsupported/)).toBeTruthy();
    // The unsupported filter is ignored -- the full table is still shown.
    expect(screen.getByText(`${snapshot.parts.length} parts`)).toBeTruthy();
  });
});

describe('App routing', () => {
  it('shows the section nav with links to inventory, bins, and projects', async () => {
    renderApp(loaded);
    await screen.findByRole('navigation', { name: 'Sections' });
    expect(screen.getByRole('link', { name: 'Inventory' }).getAttribute('href')).toBe('#/');
    expect(screen.getByRole('link', { name: 'Bins' }).getAttribute('href')).toBe('#/bins');
    expect(screen.getByRole('link', { name: 'Projects' }).getAttribute('href')).toBe('#/projects');
  });

  it('renders the bins view at #/bins (and hides the search box)', async () => {
    window.location.hash = '#/bins';
    renderApp(loaded);
    await screen.findByRole('heading', { name: 'Bins' });
    expect(screen.queryByRole('searchbox')).toBeNull();
  });

  it('renders the projects view at #/projects', async () => {
    window.location.hash = '#/projects';
    renderApp(loaded);
    await screen.findByRole('heading', { name: 'Projects' });
    expect(screen.getByText('Blinky Board')).toBeTruthy();
  });

  it('renders the part detail at #/part/<id>', async () => {
    window.location.hash = '#/part/ID000000000000000000000005';
    renderApp(loaded);
    await screen.findByRole('heading', { name: '10k 0603 1% resistor' });
    expect(screen.getByText('Specifications')).toBeTruthy();
  });

  it('switches views on hashchange without reloading', async () => {
    renderApp(loaded);
    await screen.findByRole('searchbox');
    act(() => {
      window.location.hash = '#/projects';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    await screen.findByRole('heading', { name: 'Projects' });
    expect(screen.queryByRole('searchbox')).toBeNull();
    act(() => {
      window.location.hash = '#/';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    await screen.findByRole('searchbox');
  });

  it('shows a not-found panel with a home link for an unknown hash', async () => {
    window.location.hash = '#/nope';
    renderApp(loaded);
    await screen.findByText('Page not found');
    const back = screen.getByRole('link', { name: /Back to inventory/ });
    expect(back.getAttribute('href')).toBe('#/');
  });
});
