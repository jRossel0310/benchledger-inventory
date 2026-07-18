import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      planBuild: vi.fn(),
      buildFromBom: vi.fn(),
    },
  };
});

import type { BuildPlan, BuildPlanLine, GroupRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { BuildReview } from './BuildReview';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

function line(overrides: Partial<BuildPlanLine> = {}): BuildPlanLine {
  return {
    bom_item_id: 'b1',
    part_id: 'p1',
    part_display_name: 'Part',
    required: true,
    is_reusable: false,
    reserved_qty: 0,
    available_needed: 0,
    checkout_qty: 0,
    missing: 0,
    planned_ops_preview: 'nothing to do',
    ...overrides,
  };
}

// The four line shapes the spec calls out: a reserved-only line, an
// available-draw line (the only one with a checkbox), a reusable checkout
// line, and a required-but-missing line that must be flagged without
// blocking the build.
const reservedOnlyLine = line({
  bom_item_id: 'b1',
  part_display_name: '10k 0603 resistor',
  reserved_qty: 5_000,
  planned_ops_preview: 'consume 5 reserved',
});
const availableDrawLine = line({
  bom_item_id: 'b2',
  part_display_name: '5mm red LED',
  required: false,
  reserved_qty: 1_000,
  available_needed: 3_000,
  planned_ops_preview: 'consume 1 reserved; consume 3 available (requires approval)',
});
const reusableCheckoutLine = line({
  bom_item_id: 'b3',
  part_display_name: 'Hot air rework station',
  required: false,
  is_reusable: true,
  checkout_qty: 1_000,
  planned_ops_preview: 'check out 1',
});
const requiredMissingLine = line({
  bom_item_id: 'b4',
  part_display_name: 'Rare programmable IC',
  required: true,
  missing: 2_000,
  planned_ops_preview: '2 still missing',
});

function plan(
  lines: BuildPlanLine[] = [
    reservedOnlyLine,
    availableDrawLine,
    reusableCheckoutLine,
    requiredMissingLine,
  ],
): BuildPlan {
  return { lines };
}

function group(overrides: Partial<GroupRecord> = {}): GroupRecord {
  return {
    id: 'g1',
    kind: 'build_from_bom',
    note: '',
    reversed_group_id: null,
    created_at: '2026-01-01 00:00:00',
    transactions: [],
    ...overrides,
  };
}

function renderBuildReview(onClose = vi.fn()) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const utils = render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <BuildReview projectId="proj1" onClose={onClose} />
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { ...utils, onClose };
}

beforeEach(() => {
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
});

afterEach(cleanup);

describe('BuildReview', () => {
  it('renders every plan line with its ops, badges, and the summary counts', async () => {
    vi.mocked(commands.planBuild).mockReturnValue(ok(plan()));
    renderBuildReview();

    await waitFor(() => expect(screen.getByText('10k 0603 resistor')).toBeTruthy());
    expect(screen.getByText('5mm red LED')).toBeTruthy();
    expect(screen.getByText('Hot air rework station')).toBeTruthy();
    expect(screen.getByText('Rare programmable IC')).toBeTruthy();

    // Reserved-only line: consumed unconditionally, no checkbox.
    expect(screen.getByText('Consume reserved 5')).toBeTruthy();

    // Available-draw line: gated behind exactly one checkbox on the screen.
    expect(screen.getByText('Consume reserved 1')).toBeTruthy();
    const checkboxes = screen.getAllByRole('checkbox');
    expect(checkboxes).toHaveLength(1);
    expect(screen.getByText(/Draw 3 from available/)).toBeTruthy();

    // Reusable line: checked out, not consumed.
    expect(screen.getByText('Check out 1')).toBeTruthy();

    // Required-but-missing line: flagged, not blocking.
    expect(screen.getByText('Short by 2')).toBeTruthy();
    expect(screen.getByText('Unmet required')).toBeTruthy();

    // Summary strip.
    expect(screen.getByText('4 lines')).toBeTruthy();
    expect(screen.getByText('2 consuming reserved')).toBeTruthy();
    expect(screen.getByText('0 drawing available (approved)')).toBeTruthy();
    expect(screen.getByText('1 checkout')).toBeTruthy();
    expect(screen.getByText('1 unmet required')).toBeTruthy();

    const confirmButton = screen.getByRole('button', { name: 'Confirm build' });
    expect(confirmButton.hasAttribute('disabled')).toBe(false);
  });

  it('checking the available-draw line and confirming sends exactly that line’s bom_item_id, excluding unchecked lines', async () => {
    vi.mocked(commands.planBuild).mockReturnValue(ok(plan()));
    vi.mocked(commands.buildFromBom).mockReturnValue(ok(group()));
    const { onClose } = renderBuildReview();

    await waitFor(() => expect(screen.getByText('5mm red LED')).toBeTruthy());
    fireEvent.click(screen.getByRole('checkbox'));
    await waitFor(() => expect(screen.getByText('1 drawing available (approved)')).toBeTruthy());

    fireEvent.click(screen.getByRole('button', { name: 'Confirm build' }));

    await waitFor(() => expect(commands.buildFromBom).toHaveBeenCalledWith('proj1', ['b2']));
    await waitFor(() => expect(screen.getByText('Build committed')).toBeTruthy());
    expect(onClose).toHaveBeenCalled();
  });

  it('confirming without checking any available-draw line sends an empty approval list', async () => {
    vi.mocked(commands.planBuild).mockReturnValue(ok(plan()));
    vi.mocked(commands.buildFromBom).mockReturnValue(ok(group()));
    renderBuildReview();

    await waitFor(() => expect(screen.getByText('Rare programmable IC')).toBeTruthy());
    // The required-but-missing line has no checkbox and does not disable
    // Confirm — "build what CAN be built, flag the rest".
    fireEvent.click(screen.getByRole('button', { name: 'Confirm build' }));

    await waitFor(() => expect(commands.buildFromBom).toHaveBeenCalledWith('proj1', []));
  });

  it('shows the command error and does not close when the build fails', async () => {
    vi.mocked(commands.planBuild).mockReturnValue(ok(plan()));
    vi.mocked(commands.buildFromBom).mockReturnValue(
      commandError('insufficient_stock', 'not enough stock'),
    );
    const { onClose } = renderBuildReview();

    await waitFor(() => expect(screen.getByText('10k 0603 resistor')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'Confirm build' }));

    const dialog = await screen.findByRole('dialog');
    await waitFor(() =>
      expect(
        within(dialog).getByText(
          'Not enough stock is available for this action — receive more or lower the quantity.',
        ),
      ).toBeTruthy(),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it('Cancel closes without building', async () => {
    vi.mocked(commands.planBuild).mockReturnValue(ok(plan()));
    const { onClose } = renderBuildReview();

    await waitFor(() => expect(screen.getByText('10k 0603 resistor')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(onClose).toHaveBeenCalled();
    expect(commands.buildFromBom).not.toHaveBeenCalled();
  });
});
