import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listCategories: vi.fn(),
      categoryAttributeDefs: vi.fn(),
      previewUnitValue: vi.fn(),
      findMatches: vi.fn(),
      createPart: vi.fn(),
      updatePart: vi.fn(),
      setAttribute: vi.fn(),
      setTags: vi.fn(),
      addDimension: vi.fn(),
      addVariant: vi.fn(),
      addSupplierListing: vi.fn(),
      getPart: vi.fn(),
      getAttributes: vi.fn(),
      listDimensions: vi.fn(),
      listVariants: vi.fn(),
      getTags: vi.fn(),
    },
  };
});

const navigate = vi.fn();
vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => navigate,
}));

const openQuickAction = vi.fn();
vi.mock('../quick/QuickActionContext', () => ({
  useQuickAction: () => ({ open: openQuickAction }),
}));

import type {
  AttributeDefRow,
  CategoryRecord,
  DimensionRecord,
  ListingRecord,
  PartRecord,
  VariantRecord,
} from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { PartForm } from './PartForm';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

const CATEGORIES: CategoryRecord[] = [
  { id: 'c-res', name: 'Resistor', group_name: 'Passive components', built_in: true },
  { id: 'c-cap', name: 'Capacitor', group_name: 'Passive components', built_in: true },
];

function def(overrides: Partial<AttributeDefRow>): AttributeDefRow {
  return {
    key: 'resistance',
    label: 'Resistance',
    data_type: 'number_unit',
    unit_kind: 'resistance',
    identity: true,
    display_order: 0,
    hidden: false,
    choices: [],
    ...overrides,
  };
}

const RESISTOR_DEFS: AttributeDefRow[] = [
  def({ key: 'resistance', label: 'Resistance', display_order: 0 }),
  def({ key: 'tolerance', label: 'Tolerance', unit_kind: 'percent', display_order: 1 }),
  def({
    key: 'package',
    label: 'Package',
    data_type: 'text',
    unit_kind: null,
    display_order: 2,
  }),
];

const CAPACITOR_DEFS: AttributeDefRow[] = [
  def({ key: 'capacitance', label: 'Capacitance', unit_kind: 'capacitance', display_order: 0 }),
];

function partRecord(overrides: Partial<PartRecord> = {}): PartRecord {
  return {
    id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_id: 'c-res',
    description: 'Thick film chip resistor',
    bin_label: 'A10',
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each',
    low_stock_threshold: 100_000,
    public_notes: 'pub',
    private_notes: 'priv',
    metadata_complete: true,
    archived: false,
    created_at: '2026-01-01 00:00:00',
    modified_at: '2026-01-01 00:00:00',
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(commands.listCategories).mockReturnValue(ok(CATEGORIES));
  vi.mocked(commands.categoryAttributeDefs).mockImplementation((id: string) =>
    ok(id === 'c-res' ? RESISTOR_DEFS : id === 'c-cap' ? CAPACITOR_DEFS : []),
  );
  vi.mocked(commands.previewUnitValue).mockReturnValue(ok('10 kΩ'));
  vi.mocked(commands.findMatches).mockReturnValue(ok([]));
  vi.mocked(commands.createPart).mockReturnValue(ok(partRecord()));
  vi.mocked(commands.updatePart).mockReturnValue(ok(null));
  vi.mocked(commands.setAttribute).mockReturnValue(ok(null));
  vi.mocked(commands.setTags).mockReturnValue(ok(null));
  vi.mocked(commands.addDimension).mockReturnValue(ok({ id: 'd1' } as unknown as DimensionRecord));
  vi.mocked(commands.addVariant).mockReturnValue(ok({ id: 'v1' } as unknown as VariantRecord));
  vi.mocked(commands.addSupplierListing).mockReturnValue(
    ok({ id: 'l1' } as unknown as ListingRecord),
  );
});

afterEach(cleanup);

function renderForm(partId?: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const Wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>{children}</ToastProvider>
    </QueryClientProvider>
  );
  return render(<PartForm partId={partId} />, { wrapper: Wrapper });
}

/** Wait for the async-loaded category options to be present, then select
 * one — firing the change before `listCategories` resolves would set the
 * value to an option that doesn't exist yet and be dropped. */
async function selectCategory(name: string, value: string) {
  await screen.findByRole('option', { name });
  fireEvent.change(screen.getByLabelText('Category'), { target: { value } });
}

describe('PartForm — category-adaptive attribute rendering', () => {
  it("renders no attribute fields until a category is chosen, then the category's attributes", async () => {
    renderForm();
    expect(screen.queryByLabelText('Resistance')).toBeNull();

    await selectCategory('Resistor', 'c-res');

    expect(await screen.findByLabelText('Resistance')).toBeTruthy();
    expect(screen.getByLabelText('Tolerance')).toBeTruthy();
    expect(screen.getByLabelText('Package')).toBeTruthy();
  });

  it('swaps the attribute fields when the category changes', async () => {
    renderForm();
    await selectCategory('Resistor', 'c-res');
    expect(await screen.findByLabelText('Resistance')).toBeTruthy();

    fireEvent.change(screen.getByLabelText('Category'), { target: { value: 'c-cap' } });
    expect(await screen.findByLabelText('Capacitance')).toBeTruthy();
    expect(screen.queryByLabelText('Resistance')).toBeNull();
  });
});

describe('PartForm — suggested name', () => {
  it('fills the display name from category + entered identity attributes', async () => {
    renderForm();
    await selectCategory('Resistor', 'c-res');
    fireEvent.change(await screen.findByLabelText('Resistance'), { target: { value: '10k' } });
    fireEvent.change(screen.getByLabelText('Package'), { target: { value: '0603' } });

    fireEvent.click(screen.getByRole('button', { name: 'Suggest' }));

    expect((screen.getByLabelText('Display name') as HTMLInputElement).value).toBe(
      '10k 0603 resistor',
    );
  });
});

describe('PartForm — save sequence (create)', () => {
  it('calls create_part, then set_attribute per attribute, then set_tags, in order', async () => {
    const order: string[] = [];
    vi.mocked(commands.createPart).mockImplementation(() => {
      order.push('create_part');
      return ok(partRecord());
    });
    vi.mocked(commands.setAttribute).mockImplementation((_p, key) => {
      order.push(`set_attribute:${key}`);
      return ok(null);
    });
    vi.mocked(commands.setTags).mockImplementation(() => {
      order.push('set_tags');
      return ok(null);
    });

    renderForm();
    fireEvent.change(await screen.findByLabelText('Display name'), {
      target: { value: '10k 0603 1% resistor' },
    });
    await selectCategory('Resistor', 'c-res');
    fireEvent.change(await screen.findByLabelText('Resistance'), { target: { value: '10k' } });
    fireEvent.change(screen.getByLabelText('Package'), { target: { value: '0603' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create part' }));

    await waitFor(() => expect(commands.createPart).toHaveBeenCalled());
    await waitFor(() => expect(commands.setTags).toHaveBeenCalled());

    // create_part first; both attributes before set_tags; set_tags last.
    expect(order[0]).toBe('create_part');
    expect(order.indexOf('set_attribute:resistance')).toBeGreaterThan(0);
    expect(order.indexOf('set_attribute:package')).toBeGreaterThan(0);
    expect(order[order.length - 1]).toBe('set_tags');
  });

  it('sends the entered basic-info fields in the create_part draft', async () => {
    renderForm();
    fireEvent.change(await screen.findByLabelText('Display name'), {
      target: { value: 'My resistor' },
    });
    await selectCategory('Resistor', 'c-res');
    fireEvent.change(screen.getByLabelText('Bin'), { target: { value: 'A10' } });
    fireEvent.change(screen.getByLabelText('Low-stock threshold'), { target: { value: '100' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create part' }));

    await waitFor(() =>
      expect(commands.createPart).toHaveBeenCalledWith(
        expect.objectContaining({
          display_name: 'My resistor',
          category_id: 'c-res',
          bin_label: 'A10',
          low_stock_threshold: 100_000,
          quantity_unit: 'each',
          usage_behavior: 'usually_consumed',
        }),
      ),
    );
  });

  it('navigates to the new part on success', async () => {
    renderForm();
    fireEvent.change(await screen.findByLabelText('Display name'), {
      target: { value: 'My resistor' },
    });
    await selectCategory('Resistor', 'c-res');
    fireEvent.click(screen.getByRole('button', { name: 'Create part' }));

    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({
        to: '/inventory/$partId',
        params: { partId: 'p1' },
      }),
    );
  });

  it('keeps the part and shows a partial-failure message when a later mutation fails', async () => {
    vi.mocked(commands.setAttribute).mockReturnValue(
      commandError('invalid_attribute_value', 'bad'),
    );
    renderForm();
    fireEvent.change(await screen.findByLabelText('Display name'), {
      target: { value: 'My resistor' },
    });
    await selectCategory('Resistor', 'c-res');
    fireEvent.change(await screen.findByLabelText('Resistance'), { target: { value: 'bad' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create part' }));

    await waitFor(() => expect(commands.createPart).toHaveBeenCalled());
    expect(await screen.findByText(/The part was saved, but/)).toBeTruthy();
    // Did not navigate away — the user can fix and retry.
    expect(navigate).not.toHaveBeenCalled();
  });

  it('does not submit without a display name and category', async () => {
    renderForm();
    // Category chosen but name left blank: submit stays disabled.
    await selectCategory('Resistor', 'c-res');
    expect(
      (screen.getByRole('button', { name: 'Create part' }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});

describe('PartForm — edit mode', () => {
  beforeEach(() => {
    vi.mocked(commands.getPart).mockReturnValue(ok(partRecord()));
    vi.mocked(commands.getAttributes).mockReturnValue(
      ok([
        ['resistance', '10k', 10000],
        ['package', '0603', null],
      ] as [string, string, number | null][]),
    );
    vi.mocked(commands.listDimensions).mockReturnValue(ok([]));
    vi.mocked(commands.listVariants).mockReturnValue(ok([]));
    vi.mocked(commands.getTags).mockReturnValue(ok(['smd']));
  });

  it('loads the existing part into the form fields', async () => {
    renderForm('p1');

    await waitFor(() =>
      expect((screen.getByLabelText('Display name') as HTMLInputElement).value).toBe(
        '10k 0603 1% resistor',
      ),
    );
    expect((screen.getByLabelText('Bin') as HTMLInputElement).value).toBe('A10');
    expect(await screen.findByDisplayValue('10k')).toBeTruthy();
    expect(screen.getByText('smd')).toBeTruthy();
  });

  it('calls update_part (not create_part) on save and reuses the part id', async () => {
    renderForm('p1');
    await waitFor(() =>
      expect((screen.getByLabelText('Display name') as HTMLInputElement).value).toBe(
        '10k 0603 1% resistor',
      ),
    );

    fireEvent.change(screen.getByLabelText('Display name'), { target: { value: 'renamed' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() =>
      expect(commands.updatePart).toHaveBeenCalledWith(
        expect.objectContaining({ id: 'p1', display_name: 'renamed' }),
      ),
    );
    expect(commands.createPart).not.toHaveBeenCalled();
  });
});

describe('PartForm — duplicate panel wiring', () => {
  it('fires find_matches from the identity fields and renders the match + explanation', async () => {
    vi.mocked(commands.findMatches).mockReturnValue(
      ok([
        {
          part_id: 'p-seed',
          display_name: '10k 0603 1% resistor',
          verdict_kind: 'probable_equivalent',
          explanation: 'Resistance and package agree; power rating is not entered.',
          rank: 5,
        },
      ]),
    );

    renderForm();
    await selectCategory('Resistor', 'c-res');
    fireEvent.change(await screen.findByLabelText('Resistance'), { target: { value: '10k' } });
    fireEvent.change(screen.getByLabelText('Package'), { target: { value: '0603' } });

    const panel = await screen.findByRole('region', { name: 'Possible duplicates' });
    expect(within(panel).getByText('10k 0603 1% resistor')).toBeTruthy();
    expect(
      within(panel).getByText('Resistance and package agree; power rating is not entered.'),
    ).toBeTruthy();
    expect(within(panel).getByText('Add stock to this part')).toBeTruthy();
    await waitFor(() =>
      expect(commands.findMatches).toHaveBeenCalledWith(
        expect.objectContaining({
          category_id: 'c-res',
          attributes: expect.arrayContaining([['resistance', '10k']]),
        }),
      ),
    );
  });
});
