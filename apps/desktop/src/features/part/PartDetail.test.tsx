import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      getPart: vi.fn(),
      getStock: vi.fn(),
      listCategories: vi.fn(),
      getAttributes: vi.fn(),
      categoryAttributeDefs: vi.fn(),
      getTags: vi.fn(),
      listDimensions: vi.fn(),
      listVariants: vi.fn(),
      listTransactions: vi.fn(),
      listProjects: vi.fn(),
    },
  };
});

const openQuickAction = vi.fn();
vi.mock('../quick/QuickActionContext', () => ({
  useQuickAction: () => ({ open: openQuickAction }),
}));

// `EnrichmentDiffDialog` gets its own full test suite
// (`EnrichmentDiffDialog.test.tsx`) — here we only need to prove PartDetail
// wires "Refresh product data" to mount/unmount it, never that it fetches
// or renders correctly on its own.
vi.mock('./EnrichmentDiffDialog', () => ({
  EnrichmentDiffDialog: ({ partId, onClose }: { partId: string; onClose: () => void }) => (
    <div data-testid="enrichment-diff-dialog-stub">
      <span>Enrichment dialog for {partId}</span>
      <button type="button" onClick={onClose}>
        Close enrichment dialog
      </button>
    </div>
  ),
}));

import type { AttributeDefRow, CategoryRecord, PartRecord, PartStockRow } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { PartDetail } from './PartDetail';

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
    public_notes: '',
    private_notes: '',
    metadata_complete: true,
    archived: false,
    created_at: '2026-01-01 00:00:00',
    modified_at: '2026-01-02 00:00:00',
    ...overrides,
  };
}

function stock(overrides: Partial<PartStockRow> = {}): PartStockRow {
  return {
    available: 450_000,
    reserved: 50_000,
    checked_out: 0,
    lifetime_received: 500_000,
    lifetime_consumed: 0,
    ...overrides,
  };
}

const CATEGORY: CategoryRecord = {
  id: 'cat-resistor',
  name: 'Resistor',
  group_name: 'Passives',
  built_in: true,
};

const IDENTITY_DEF: AttributeDefRow = {
  key: 'resistance',
  label: 'Resistance',
  data_type: 'number_unit',
  unit_kind: 'resistance',
  identity: true,
  display_order: 1,
  hidden: false,
  choices: [],
};

function mockDefaults() {
  vi.mocked(commands.getPart).mockReturnValue(ok(part()));
  vi.mocked(commands.getStock).mockReturnValue(ok(stock()));
  vi.mocked(commands.listCategories).mockReturnValue(ok([CATEGORY]));
  vi.mocked(commands.getAttributes).mockReturnValue(ok([['resistance', '10k', 10_000]]));
  vi.mocked(commands.categoryAttributeDefs).mockReturnValue(ok([IDENTITY_DEF]));
  vi.mocked(commands.getTags).mockReturnValue(ok([]));
  vi.mocked(commands.listDimensions).mockReturnValue(ok([]));
  vi.mocked(commands.listVariants).mockReturnValue(ok([]));
  vi.mocked(commands.listTransactions).mockReturnValue(ok([]));
  vi.mocked(commands.listProjects).mockReturnValue(ok([]));
}

beforeEach(() => {
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
});

afterEach(cleanup);

/** A minimal router with the routes `PartDetail`'s Edit/full-page links
 * target, so those `<Link>`s render real hrefs without a full app shell. */
function renderPartDetail(onClose?: () => void) {
  const rootRoute = createRootRoute();
  const partDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/$partId',
    component: () => <PartDetail partId="p1" onClose={onClose} />,
  });
  const editRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/inventory/$partId/edit',
    component: () => <div>Edit part stub</div>,
  });
  const routeTree = rootRoute.addChildren([partDetailRoute, editRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/inventory/p1'] }),
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

describe('PartDetail', () => {
  it('renders the header: display name, category, identity specs, bin, and the panel-size gauge', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    expect(screen.getByText('Resistor')).toBeTruthy();
    // Identity specs depend on two further queries (attributes + category
    // attribute defs) that only enable once the part itself has loaded, so
    // they can land a tick after the title/category above.
    await waitFor(() => expect(screen.getByText('10k')).toBeTruthy()); // identity spec value
    expect(screen.getByText('A12')).toBeTruthy(); // bin
    expect(
      screen.getByRole('img', { name: '450 available, 50 reserved, 0 checked out' }),
    ).toBeTruthy();
  });

  it('renders the four quantity figures', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    expect(screen.getByText('Available')).toBeTruthy();
    expect(screen.getByText('Reserved')).toBeTruthy();
    expect(screen.getByText('Checked out')).toBeTruthy();
    expect(screen.getByText('Current stock')).toBeTruthy();
  });

  it('shows a low-stock badge when available is under the threshold', async () => {
    vi.mocked(commands.getPart).mockReturnValue(ok(part({ low_stock_threshold: 500_000 })));
    vi.mocked(commands.getStock).mockReturnValue(ok(stock({ available: 8_000 })));
    vi.mocked(commands.listCategories).mockReturnValue(ok([CATEGORY]));
    vi.mocked(commands.getAttributes).mockReturnValue(ok([]));
    vi.mocked(commands.categoryAttributeDefs).mockReturnValue(ok([]));
    vi.mocked(commands.getTags).mockReturnValue(ok([]));
    vi.mocked(commands.listDimensions).mockReturnValue(ok([]));
    vi.mocked(commands.listVariants).mockReturnValue(ok([]));
    vi.mocked(commands.listTransactions).mockReturnValue(ok([]));
    vi.mocked(commands.listProjects).mockReturnValue(ok([]));

    renderPartDetail();

    await waitFor(() => expect(screen.getByText(/low stock/i)).toBeTruthy());
  });

  it('renders every section tab', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    for (const tab of [
      'Overview',
      'Specifications',
      'Dimensions',
      'Variants',
      'Supplier listings',
      'Transactions',
      'Attachments',
      'Metadata',
    ]) {
      expect(screen.getByRole('tab', { name: tab })).toBeTruthy();
    }
  });

  it('switches tab content when a different tab is clicked', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    await waitFor(() => expect(screen.getByText('A thick-film chip resistor.')).toBeTruthy()); // Overview description

    const metadataTab = screen.getByRole('tab', { name: 'Metadata' });
    // Radix's `Tabs.Trigger` selects on `mousedown`, not `click` (see
    // `@radix-ui/react-tabs`'s trigger source) — `fireEvent.click` alone
    // never dispatches a `mousedown`, so the tab would stay unselected.
    fireEvent.mouseDown(metadataTab, { button: 0, ctrlKey: false });
    fireEvent.click(metadataTab);

    await waitFor(() => expect(screen.getByText('Metadata complete')).toBeTruthy());
  });

  it('a primary action (Add stock) opens the QuickAction dialog preselected to this part', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'Add stock' }));

    expect(openQuickAction).toHaveBeenCalledWith({
      kind: 'receive',
      part: { id: 'p1', displayName: '10k 0603 1% resistor' },
    });
  });

  it('Reserve and Check out primary actions also open QuickAction preselected', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'Reserve' }));
    expect(openQuickAction).toHaveBeenCalledWith({
      kind: 'reserve',
      part: { id: 'p1', displayName: '10k 0603 1% resistor' },
    });

    fireEvent.click(screen.getByRole('button', { name: 'Check out' }));
    expect(openQuickAction).toHaveBeenCalledWith({
      kind: 'check_out',
      part: { id: 'p1', displayName: '10k 0603 1% resistor' },
    });
  });

  it('the Edit button links to the edit route', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    const editLink = screen.getByRole('link', { name: 'Edit' });
    expect(editLink.getAttribute('href')).toBe('/inventory/p1/edit');
  });

  it('"Refresh product data" opens the enrichment diff dialog only once clicked, never before', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    expect(screen.queryByTestId('enrichment-diff-dialog-stub')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: /refresh product data/i }));

    await waitFor(() => expect(screen.getByTestId('enrichment-diff-dialog-stub')).toBeTruthy());
    expect(screen.getByText('Enrichment dialog for p1')).toBeTruthy();
  });

  it('closing the enrichment diff dialog unmounts it', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: /refresh product data/i }));
    await waitFor(() => expect(screen.getByTestId('enrichment-diff-dialog-stub')).toBeTruthy());

    fireEvent.click(screen.getByRole('button', { name: 'Close enrichment dialog' }));
    await waitFor(() => expect(screen.queryByTestId('enrichment-diff-dialog-stub')).toBeNull());
  });

  it('shows a close control when onClose is provided (drawer mode)', async () => {
    mockDefaults();
    const onClose = vi.fn();
    renderPartDetail(onClose);

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(onClose).toHaveBeenCalled();
  });

  it('omits the close control in full-page mode (no onClose)', async () => {
    mockDefaults();
    renderPartDetail();

    await waitFor(() => expect(screen.getByText('10k 0603 1% resistor')).toBeTruthy());
    expect(screen.queryByRole('button', { name: /close/i })).toBeNull();
  });

  it('shows a friendly not-found message when get_part resolves null', async () => {
    vi.mocked(commands.getPart).mockReturnValue(ok(null));

    renderPartDetail();

    await waitFor(() => expect(screen.getByText(/part not found/i)).toBeTruthy());
  });

  it('shows an error message when get_part fails', async () => {
    vi.mocked(commands.getPart).mockReturnValue(
      Promise.resolve({
        status: 'error' as const,
        error: { code: 'sqlite', message: 'db locked' },
      }),
    );

    renderPartDetail();

    await waitFor(() => expect(screen.getByText(/could not load this part/i)).toBeTruthy());
  });
});
