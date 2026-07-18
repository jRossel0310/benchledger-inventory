import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listBom: vi.fn(),
      search: vi.fn(),
      addBomItem: vi.fn(),
      updateBomItem: vi.fn(),
      removeBomItem: vi.fn(),
      setBomSubstitutes: vi.fn(),
      getPart: vi.fn(),
    },
  };
});

const openPartInspector = vi.fn();
vi.mock('../part/PartInspectorContext', () => ({
  usePartInspector: () => ({ open: openPartInspector }),
}));

import type { BomItemRecord, SearchHit } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { BomTable } from './BomTable';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

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
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
});

afterEach(cleanup);

function bomItem(overrides: Partial<BomItemRecord> = {}): BomItemRecord {
  return {
    id: 'b1',
    project_id: 'proj1',
    part_id: 'p1',
    part_display_name: '10k 0603 1% resistor',
    quantity_per_build: 10_000,
    total_required: 50_000,
    reference_designators: 'R1-R10',
    required: true,
    notes: '',
    substitutes: [],
    reserved: 50_000,
    consumed: 0,
    available: 100_000,
    missing: 0,
    ...overrides,
  };
}

function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    part_id: 'p2',
    display_name: '5mm red LED',
    category_name: 'LED',
    bin_label: 'B4',
    available: 200_000,
    reserved: 0,
    checked_out: 0,
    low_stock_threshold: null,
    archived: false,
    ...overrides,
  };
}

function renderBomTable(projectId = 'proj1') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <BomTable projectId={projectId} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

function openRowMenu(name: string) {
  // Radix's `DropdownMenuTrigger` opens on `pointerdown`, not `click`.
  const trigger = screen.getByRole('button', { name: new RegExp(`actions for ${name}`, 'i') });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.click(trigger);
}

describe('BomTable', () => {
  it('renders the seven spec columns computed from the record, highlighting a positive Missing', async () => {
    vi.mocked(commands.listBom).mockReturnValue(
      ok([
        bomItem({
          id: 'b1',
          part_display_name: '10k 0603 1% resistor',
          quantity_per_build: 10_000,
          total_required: 50_000,
          available: 100_000,
          reserved: 50_000,
          consumed: 0,
          missing: 0,
        }),
        bomItem({
          id: 'b2',
          part_id: 'p2',
          part_display_name: '5mm red LED',
          quantity_per_build: 2_000,
          total_required: 10_000,
          available: 1_000,
          reserved: 5_000,
          consumed: 1_000,
          missing: 3_000,
        }),
      ]),
    );

    const { container } = renderBomTable();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());

    const rows = container.querySelectorAll('.data-table-row');
    expect(rows).toHaveLength(2);

    function cellTexts(row: Element): string[] {
      return within(row as HTMLElement)
        .getAllByRole('cell')
        .map((cell) => cell.textContent ?? '');
    }

    const [row1, row2] = Array.from(rows);
    // Columns: Part, Per build, Needed, Available, Reserved, Consumed, Missing.
    const row1Cells = cellTexts(row1 as Element);
    expect(row1Cells.slice(1)).toEqual(['10', '50', '100', '50', '0', '0']);
    const row2Cells = cellTexts(row2 as Element);
    expect(row2Cells.slice(1)).toEqual(['2', '10', '1', '5', '1', '3']);

    // Missing is highlighted only when it's positive.
    const row1MissingCell = within(row1 as HTMLElement).getAllByRole('cell')[6];
    expect(row1MissingCell?.querySelector('.bom-table-missing')).toBeNull();
    const row2MissingCell = within(row2 as HTMLElement).getAllByRole('cell')[6];
    expect(row2MissingCell?.querySelector('.bom-table-missing')).toBeTruthy();
  });

  it('shows the empty state when the BOM has no lines', async () => {
    vi.mocked(commands.listBom).mockReturnValue(ok([]));
    renderBomTable();

    await waitFor(() =>
      expect(
        screen.getByText('No BOM lines yet — add a part to start the bill of materials.'),
      ).toBeTruthy(),
    );
  });

  it('activating a row opens the part inspector for that line’s part', async () => {
    vi.mocked(commands.listBom).mockReturnValue(ok([bomItem()]));
    renderBomTable();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    fireEvent.click(screen.getByText('10k 0603 1% resistor'));

    expect(openPartInspector).toHaveBeenCalledWith('p1');
  });

  it('adding a BOM line searches, picks a part, and calls addBomItem with the built draft', async () => {
    vi.mocked(commands.listBom).mockReturnValue(ok([]));
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));
    vi.mocked(commands.addBomItem).mockReturnValue(
      ok(bomItem({ id: 'b9', part_id: 'p2', part_display_name: '5mm red LED' })),
    );

    renderBomTable();
    await waitFor(() =>
      expect(
        screen.getByText('No BOM lines yet — add a part to start the bill of materials.'),
      ).toBeTruthy(),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Add part' }));
    fireEvent.change(screen.getByPlaceholderText('Search parts…'), {
      target: { value: 'LED' },
    });
    fireEvent.click(await screen.findByText('5mm red LED'));

    fireEvent.change(screen.getByLabelText('Quantity per build'), { target: { value: '2' } });
    fireEvent.change(screen.getByLabelText('Reference designators'), {
      target: { value: 'D1, D2' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add to BOM' }));

    await waitFor(() =>
      expect(commands.addBomItem).toHaveBeenCalledWith(
        'proj1',
        expect.objectContaining({
          part_id: 'p2',
          quantity_per_build: 2_000,
          reference_designators: 'D1, D2',
          required: true,
        }),
      ),
    );
  });

  it('editing a BOM line prefills its fields and calls updateBomItem on save', async () => {
    vi.mocked(commands.listBom).mockReturnValue(ok([bomItem()]));
    vi.mocked(commands.updateBomItem).mockReturnValue(ok(bomItem({ quantity_per_build: 20_000 })));

    renderBomTable();
    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());

    openRowMenu('10k 0603 1% resistor');
    fireEvent.click(await screen.findByText('Edit line'));

    const qty = (await screen.findByLabelText('Quantity per build')) as HTMLInputElement;
    expect(qty.value).toBe('10');
    fireEvent.change(qty, { target: { value: '20' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() =>
      expect(commands.updateBomItem).toHaveBeenCalledWith(
        'b1',
        expect.objectContaining({ quantity_per_build: 20_000, part_id: 'p1' }),
      ),
    );
    // Substitutes were never touched, so the dedicated command shouldn't fire.
    expect(commands.setBomSubstitutes).not.toHaveBeenCalled();
  });

  it('adding a substitute in the edit dialog calls setBomSubstitutes on save', async () => {
    vi.mocked(commands.listBom).mockReturnValue(ok([bomItem()]));
    vi.mocked(commands.updateBomItem).mockReturnValue(ok(bomItem()));
    vi.mocked(commands.setBomSubstitutes).mockReturnValue(ok(null));
    vi.mocked(commands.search).mockReturnValue(ok([hit()]));

    renderBomTable();
    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());

    openRowMenu('10k 0603 1% resistor');
    fireEvent.click(await screen.findByText('Edit line'));
    await screen.findByLabelText('Quantity per build');

    fireEvent.change(screen.getByPlaceholderText('Add a substitute part…'), {
      target: { value: 'LED' },
    });
    fireEvent.click(await screen.findByText('5mm red LED'));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(commands.setBomSubstitutes).toHaveBeenCalledWith('b1', ['p2']));
  });

  it('removing a BOM line confirms then calls removeBomItem', async () => {
    vi.mocked(commands.listBom).mockReturnValue(ok([bomItem()]));
    vi.mocked(commands.removeBomItem).mockReturnValue(ok(null));

    renderBomTable();
    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());

    openRowMenu('10k 0603 1% resistor');
    fireEvent.click(await screen.findByText('Remove'));

    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));

    await waitFor(() => expect(commands.removeBomItem).toHaveBeenCalledWith('b1'));
  });
});
