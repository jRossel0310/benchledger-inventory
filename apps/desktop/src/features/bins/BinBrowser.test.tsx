import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listBins: vi.fn(),
      search: vi.fn(),
      renameBin: vi.fn(),
      getPart: vi.fn(),
      updatePart: vi.fn(),
    },
  };
});

// A row click opens the part-detail inspector drawer (Task 7); this suite
// tests the bin grid + parts-table wiring, not the drawer itself.
vi.mock('../part/PartInspectorContext', () => ({
  usePartInspector: () => ({ open: vi.fn() }),
}));

// Reserve/Check out row actions open this dialog; irrelevant here.
vi.mock('../quick/QuickActionContext', () => ({
  useQuickAction: () => ({ open: vi.fn() }),
}));

import type { BinSummary, SearchHit } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { BinBrowser } from './BinBrowser';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

function bin(overrides: Partial<BinSummary> = {}): BinSummary {
  return { bin_label: 'A1', part_count: 1, ...overrides };
}

function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    part_id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_name: 'Resistor',
    bin_label: 'A1',
    available: 450_000,
    reserved: 0,
    checked_out: 0,
    low_stock_threshold: null,
    archived: false,
    ...overrides,
  };
}

// @tanstack/react-virtual measures the scroll container via
// offsetHeight/offsetWidth; jsdom always reports 0, which would hide every
// row as "out of view" — same fix as InventoryTable.test.tsx/DataTable.test.tsx.
let offsetHeightSpy: ReturnType<typeof vi.spyOn>;
let offsetWidthSpy: ReturnType<typeof vi.spyOn>;

beforeAll(() => {
  offsetHeightSpy = vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(400);
  offsetWidthSpy = vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(1000);
});

afterAll(() => {
  offsetHeightSpy.mockRestore();
  offsetWidthSpy.mockRestore();
});

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(commands.search).mockReturnValue(ok<SearchHit[]>([]));
});

afterEach(cleanup);

function renderBinBrowser() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <BinBrowser />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('BinBrowser', () => {
  it('renders the bin grid with counts, including a distinct Unassigned entry', async () => {
    vi.mocked(commands.listBins).mockReturnValue(
      ok([bin({ bin_label: 'A1', part_count: 3 }), bin({ bin_label: null, part_count: 2 })]),
    );
    renderBinBrowser();

    expect(await screen.findByText('A1')).toBeTruthy();
    expect(screen.getByText('3')).toBeTruthy();
    expect(screen.getByText('Unassigned')).toBeTruthy();
    expect(screen.getByText('2')).toBeTruthy();
  });

  it('always shows the Unassigned tile at a count of 0, even when every part has a bin', async () => {
    vi.mocked(commands.listBins).mockReturnValue(ok([bin({ bin_label: 'A1', part_count: 1 })]));
    renderBinBrowser();

    const label = await screen.findByText('Unassigned');
    const unassignedTile = label.closest('.bin-tile');
    expect(unassignedTile?.textContent).toContain('0');
  });

  it('shows an empty state when there are no non-archived parts at all', async () => {
    vi.mocked(commands.listBins).mockReturnValue(ok([]));
    renderBinBrowser();

    expect(await screen.findByText('No parts yet')).toBeTruthy();
  });

  it('shows a hint until a bin is selected', async () => {
    vi.mocked(commands.listBins).mockReturnValue(ok([bin({ bin_label: 'A1', part_count: 1 })]));
    renderBinBrowser();

    expect(await screen.findByText('Select a bin to see its parts.')).toBeTruthy();
  });

  it('selecting a named bin queries search with a quoted bin: filter and renders its parts', async () => {
    vi.mocked(commands.listBins).mockReturnValue(
      ok([bin({ bin_label: 'Drawer 3', part_count: 1 })]),
    );
    vi.mocked(commands.search).mockReturnValue(
      ok([hit({ bin_label: 'Drawer 3', display_name: 'Part in drawer 3' })]),
    );
    renderBinBrowser();

    fireEvent.click(await screen.findByText('Drawer 3'));

    await waitFor(() => expect(commands.search).toHaveBeenCalledWith('bin:"Drawer 3"'));
    expect(await screen.findByText('Part in drawer 3')).toBeTruthy();
  });

  it('selecting Unassigned shows only parts with no bin, filtered client-side over the full list', async () => {
    vi.mocked(commands.listBins).mockReturnValue(
      ok([bin({ bin_label: 'A1', part_count: 1 }), bin({ bin_label: null, part_count: 1 })]),
    );
    vi.mocked(commands.search).mockReturnValue(
      ok([
        hit({ part_id: 'p1', display_name: 'Binned part', bin_label: 'A1' }),
        hit({ part_id: 'p2', display_name: 'Unbinned part', bin_label: null }),
      ]),
    );
    renderBinBrowser();

    fireEvent.click(await screen.findByText('Unassigned'));

    await waitFor(() => expect(commands.search).toHaveBeenCalledWith(''));
    expect(await screen.findByText('Unbinned part')).toBeTruthy();
    expect(screen.queryByText('Binned part')).toBeNull();
  });

  it('shows a friendly empty message for a bin/bucket with no parts', async () => {
    vi.mocked(commands.listBins).mockReturnValue(ok([bin({ bin_label: null, part_count: 0 })]));
    vi.mocked(commands.search).mockReturnValue(ok([]));
    renderBinBrowser();

    fireEvent.click(await screen.findByText('Unassigned'));

    expect(await screen.findByText('No unassigned parts — every part has a bin.')).toBeTruthy();
  });

  describe('rename', () => {
    async function selectBinA1() {
      vi.mocked(commands.listBins).mockReturnValue(
        ok([bin({ bin_label: 'A1', part_count: 2 }), bin({ bin_label: 'B2', part_count: 3 })]),
      );
      renderBinBrowser();
      fireEvent.click(await screen.findByText('A1'));
      await screen.findByLabelText('Rename bin');
    }

    it('renaming to a free label calls renameBin and toasts the moved count', async () => {
      vi.mocked(commands.renameBin).mockReturnValue(ok(2));
      await selectBinA1();

      fireEvent.change(screen.getByLabelText('Rename bin'), { target: { value: 'A1-NEW' } });
      fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

      await waitFor(() => expect(commands.renameBin).toHaveBeenCalledWith('A1', 'A1-NEW'));
      expect(await screen.findByText('Renamed to A1-NEW (2 parts moved)')).toBeTruthy();
    });

    it('renaming into an already-occupied bin warns but does not block — proceeds on "Rename anyway"', async () => {
      vi.mocked(commands.renameBin).mockReturnValue(ok(2));
      await selectBinA1();

      fireEvent.change(screen.getByLabelText('Rename bin'), { target: { value: 'B2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

      expect(await screen.findByText('Bin B2 already holds 3 parts — rename anyway?')).toBeTruthy();
      expect(commands.renameBin).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole('button', { name: 'Rename anyway' }));

      await waitFor(() => expect(commands.renameBin).toHaveBeenCalledWith('A1', 'B2'));
    });

    it('canceling the merge warning does not rename', async () => {
      await selectBinA1();

      fireEvent.change(screen.getByLabelText('Rename bin'), { target: { value: 'B2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
      expect(await screen.findByText(/already holds 3 parts/)).toBeTruthy();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      expect(commands.renameBin).not.toHaveBeenCalled();
    });

    it('the Rename button stays disabled for an unchanged or blank label', async () => {
      await selectBinA1();

      const submit = () => screen.getByRole('button', { name: 'Rename' }) as HTMLButtonElement;
      expect(submit().disabled).toBe(true);

      fireEvent.change(screen.getByLabelText('Rename bin'), { target: { value: '   ' } });
      expect(submit().disabled).toBe(true);

      fireEvent.change(screen.getByLabelText('Rename bin'), { target: { value: 'a1' } });
      expect(submit().disabled).toBe(true);
    });

    it('toasts a hinted error when the rename fails', async () => {
      vi.mocked(commands.renameBin).mockReturnValue(
        commandError('invalid_bin_label', 'invalid bin label: new bin label cannot be empty'),
      );
      await selectBinA1();

      fireEvent.change(screen.getByLabelText('Rename bin'), { target: { value: 'A1-NEW' } });
      fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

      await waitFor(() => expect(screen.getByText('Could not rename bin')).toBeTruthy());
    });

    it('does not show a rename control for the Unassigned bucket', async () => {
      vi.mocked(commands.listBins).mockReturnValue(ok([bin({ bin_label: null, part_count: 1 })]));
      renderBinBrowser();

      fireEvent.click(await screen.findByText('Unassigned'));

      await waitFor(() => expect(commands.search).toHaveBeenCalled());
      expect(screen.queryByLabelText('Rename bin')).toBeNull();
    });
  });
});
