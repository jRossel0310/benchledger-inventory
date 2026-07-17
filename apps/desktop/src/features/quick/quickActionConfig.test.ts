import { describe, expect, it } from 'vitest';

import {
  buildLedgerOp,
  composeReceiveNote,
  quickActionConfig,
  QUICK_ACTIONS,
  quickActionToastTitle,
} from './quickActionConfig';

describe('QUICK_ACTIONS / quickActionConfig', () => {
  it('lists exactly the six ledger-backed quick actions with instrument-voice labels', () => {
    const kinds = QUICK_ACTIONS.map((a) => a.kind);
    expect(kinds).toEqual([
      'receive',
      'consume_available',
      'reserve',
      'release_reservation',
      'check_out',
      'return',
    ]);
    // Actions name their effect, never "Submit" (design voice).
    expect(QUICK_ACTIONS.map((a) => a.label)).toEqual([
      'Add stock',
      'Consume',
      'Reserve for project',
      'Release reservation',
      'Check out',
      'Return',
    ]);
  });

  it('flags project requirement per action: none for receive, optional for consume, required for the rest', () => {
    expect(quickActionConfig('receive').project).toBe('none');
    expect(quickActionConfig('consume_available').project).toBe('optional');
    expect(quickActionConfig('reserve').project).toBe('required');
    expect(quickActionConfig('release_reservation').project).toBe('required');
    expect(quickActionConfig('check_out').project).toBe('required');
    expect(quickActionConfig('return').project).toBe('required');
  });

  it('throws for an unknown kind rather than silently returning undefined', () => {
    // @ts-expect-error -- deliberately an invalid kind to test the guard.
    expect(() => quickActionConfig('bogus')).toThrow(/unknown quick action kind/);
  });
});

describe('buildLedgerOp', () => {
  const partId = 'p1';

  it('builds a receive op with no project field', () => {
    expect(
      buildLedgerOp({
        kind: 'receive',
        partId,
        quantityMilli: 10_000,
        note: 'restock',
        projectId: null,
      }),
    ).toEqual({ type: 'receive', part_id: partId, quantity: 10_000, note: 'restock' });
  });

  it('builds a consume_available op carrying a nullable project and note', () => {
    expect(
      buildLedgerOp({
        kind: 'consume_available',
        partId,
        quantityMilli: 5_000,
        note: 'LED driver',
        projectId: null,
      }),
    ).toEqual({
      type: 'consume_available',
      part_id: partId,
      quantity: 5_000,
      project_id: null,
      note: 'LED driver',
    });

    expect(
      buildLedgerOp({
        kind: 'consume_available',
        partId,
        quantityMilli: 5_000,
        note: '',
        projectId: 'pr1',
      }),
    ).toEqual({
      type: 'consume_available',
      part_id: partId,
      quantity: 5_000,
      project_id: 'pr1',
      note: '',
    });
  });

  it('builds reserve/release_reservation/check_out/return ops with the required project id', () => {
    expect(
      buildLedgerOp({ kind: 'reserve', partId, quantityMilli: 3_000, note: '', projectId: 'pr1' }),
    ).toEqual({ type: 'reserve', part_id: partId, quantity: 3_000, project_id: 'pr1' });

    expect(
      buildLedgerOp({
        kind: 'release_reservation',
        partId,
        quantityMilli: 2_000,
        note: '',
        projectId: 'pr1',
      }),
    ).toEqual({ type: 'release_reservation', part_id: partId, quantity: 2_000, project_id: 'pr1' });

    expect(
      buildLedgerOp({
        kind: 'check_out',
        partId,
        quantityMilli: 1_000,
        note: '',
        projectId: 'pr1',
      }),
    ).toEqual({ type: 'check_out', part_id: partId, quantity: 1_000, project_id: 'pr1' });

    expect(
      buildLedgerOp({ kind: 'return', partId, quantityMilli: 1_000, note: '', projectId: 'pr1' }),
    ).toEqual({ type: 'return', part_id: partId, quantity: 1_000, project_id: 'pr1' });
  });

  it('refuses to build a project-required op without a project rather than sending a bad request', () => {
    expect(() =>
      buildLedgerOp({ kind: 'reserve', partId, quantityMilli: 1_000, note: '', projectId: null }),
    ).toThrow(/reserve requires a project/);
    expect(() =>
      buildLedgerOp({
        kind: 'release_reservation',
        partId,
        quantityMilli: 1_000,
        note: '',
        projectId: null,
      }),
    ).toThrow(/release_reservation requires a project/);
    expect(() =>
      buildLedgerOp({ kind: 'check_out', partId, quantityMilli: 1_000, note: '', projectId: null }),
    ).toThrow(/check_out requires a project/);
    expect(() =>
      buildLedgerOp({ kind: 'return', partId, quantityMilli: 1_000, note: '', projectId: null }),
    ).toThrow(/return requires a project/);
  });
});

describe('quickActionToastTitle', () => {
  it('names the effect for every action, matching the design voice ("Received 10", not "Submitted")', () => {
    expect(quickActionToastTitle('receive', 10_000, 'each')).toBe('Received 10');
    expect(quickActionToastTitle('consume_available', 5_000, 'each')).toBe('Consumed 5');
  });

  it('appends the project name for project-scoped actions when known', () => {
    expect(quickActionToastTitle('reserve', 5_000, 'each', 'Blinky Board')).toBe(
      'Reserved 5 for Blinky Board',
    );
    expect(quickActionToastTitle('check_out', 3_000, 'each', 'Blinky Board')).toBe(
      'Checked out 3 for Blinky Board',
    );
    expect(quickActionToastTitle('release_reservation', 2_000, 'each', 'Blinky Board')).toBe(
      'Released 2 from Blinky Board',
    );
    expect(quickActionToastTitle('return', 1_000, 'each', 'Blinky Board')).toBe(
      'Returned 1 from Blinky Board',
    );
  });

  it('omits the project clause entirely when no project name is known', () => {
    expect(quickActionToastTitle('reserve', 5_000, 'each')).toBe('Reserved 5');
  });
});

describe('composeReceiveNote', () => {
  it('folds the optional "Add details" fields and the free-text note into one string', () => {
    expect(
      composeReceiveNote({
        note: 'from the big order',
        supplier: 'DigiKey',
        order: 'PO-123',
        date: '2026-07-15',
        cost: '4.20',
      }),
    ).toBe(
      'Supplier: DigiKey · Order: PO-123 · Date: 2026-07-15 · Cost: 4.20 · from the big order',
    );
  });

  it('omits any field left blank, including an entirely blank note', () => {
    expect(
      composeReceiveNote({ note: '', supplier: 'DigiKey', order: '', date: '', cost: '' }),
    ).toBe('Supplier: DigiKey');
  });

  it('is just the free-text note when no detail fields are filled', () => {
    expect(
      composeReceiveNote({ note: 'restock', supplier: '', order: '', date: '', cost: '' }),
    ).toBe('restock');
  });

  it('is an empty string when everything is blank', () => {
    expect(composeReceiveNote({ note: '', supplier: '', order: '', date: '', cost: '' })).toBe('');
  });

  it('trims whitespace-only fields as blank', () => {
    expect(composeReceiveNote({ note: '  ', supplier: '  ', order: '', date: '', cost: '' })).toBe(
      '',
    );
  });
});
