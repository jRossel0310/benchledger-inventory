import { describe, expect, it } from 'vitest';

import type {
  ImportLineId,
  ImportReviewLine,
  LineDecision,
  ProposedAction,
} from '../../bindings.gen';
import {
  decisionFromProposed,
  isCreateNewIncomplete,
  placeholderPartDraft,
  prefillListing,
  prefillVariant,
  summarizeDecisions,
} from './lineDecisions';

const CONTEXT = { currency: 'USD', orderDate: '2026-07-01' };

function line(overrides: Partial<ImportReviewLine> = {}): ImportReviewLine {
  return {
    line_id: 'line1',
    line_number: 1,
    kind: 'part',
    supplier_sku: 'DK123',
    mpn: 'MPN1',
    manufacturer: 'Acme',
    description: 'Resistor 1k',
    receive_qty_milli: 5000,
    ordered_milli: 5000,
    backordered_milli: 0,
    unit_price_micros: 100_000,
    matches: [],
    proposed: { type: 'create_new' },
    warning: null,
    ...overrides,
  };
}

describe('decisionFromProposed', () => {
  it('maps add_stock_to_existing to add_stock against the same part', () => {
    const proposed: ProposedAction = {
      type: 'add_stock_to_existing',
      part_id: 'part1',
      verdict_kind: 'exact_sku',
    };
    const { decision, warning } = decisionFromProposed(line({ proposed }), CONTEXT);
    expect(decision).toEqual({ type: 'add_stock', part_id: 'part1' });
    expect(warning).toBeNull();
  });

  it('maps create_new to a create_new decision with an incomplete placeholder draft', () => {
    const { decision, warning } = decisionFromProposed(
      line({ proposed: { type: 'create_new' } }),
      CONTEXT,
    );
    expect(decision.type).toBe('create_new');
    expect(isCreateNewIncomplete(decision)).toBe(true);
    expect(warning).toBeNull();
    if (decision.type === 'create_new') {
      expect(decision.draft.display_name).toBe('Resistor 1k');
      expect(decision.variant.manufacturer).toBe('Acme');
      expect(decision.variant.mpn).toBe('MPN1');
      expect(decision.listing.supplier).toBe('DigiKey');
      expect(decision.listing.supplier_sku).toBe('DK123');
      expect(decision.listing.currency).toBe('USD');
      expect(decision.listing.last_purchase_date).toBe('2026-07-01');
    }
  });

  it('maps non_inventory (fee/tariff/no_charge) to skip', () => {
    const { decision, warning } = decisionFromProposed(
      line({ kind: 'fee', proposed: { type: 'non_inventory' } }),
      CONTEXT,
    );
    expect(decision).toEqual({ type: 'skip' });
    expect(warning).toBeNull();
  });

  it('maps ignore (unclassified lines) to skip', () => {
    const { decision, warning } = decisionFromProposed(
      line({ kind: 'unknown', proposed: { type: 'ignore' } }),
      CONTEXT,
    );
    expect(decision).toEqual({ type: 'skip' });
    expect(warning).toBeNull();
  });

  it('defaults an unrecognized proposed action to skip with a visible warning', () => {
    const proposed = { type: 'some_future_action' } as unknown as ProposedAction;
    const { decision, warning } = decisionFromProposed(line({ proposed }), CONTEXT);
    expect(decision).toEqual({ type: 'skip' });
    expect(warning).toContain('some_future_action');
  });
});

describe('prefillVariant / prefillListing / placeholderPartDraft', () => {
  it('never fabricates fields the line does not carry', () => {
    const bare = line({ manufacturer: null, mpn: null, description: null, supplier_sku: null });
    const variant = prefillVariant(bare);
    expect(variant.manufacturer).toBe('');
    expect(variant.mpn).toBe('');
    expect(variant.package).toBeNull();
    expect(variant.datasheet_url).toBeNull();

    const listing = prefillListing(bare, CONTEXT);
    expect(listing.supplier_sku).toBe('');
    expect(listing.product_url).toBeNull();

    const draft = placeholderPartDraft(bare);
    expect(draft.category_id).toBe('');
    expect(draft.display_name).toBe('New part');
  });

  it('placeholderPartDraft is always flagged incomplete', () => {
    const decision: LineDecision = {
      type: 'create_new',
      draft: placeholderPartDraft(line()),
      variant: prefillVariant(line()),
      listing: prefillListing(line(), CONTEXT),
    };
    expect(isCreateNewIncomplete(decision)).toBe(true);
  });

  it('isCreateNewIncomplete is false for every other decision type', () => {
    expect(isCreateNewIncomplete({ type: 'add_stock', part_id: 'part1' })).toBe(false);
    expect(isCreateNewIncomplete({ type: 'skip' })).toBe(false);
  });
});

describe('summarizeDecisions', () => {
  const completeDraft = {
    display_name: 'New part',
    category_id: 'cat1',
    description: '',
    bin_label: null,
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each' as const,
    low_stock_threshold: null,
    public_notes: '',
    private_notes: '',
  };
  const variant = prefillVariant(line());
  const listing = prefillListing(line(), CONTEXT);

  function feeLine(overrides: Partial<ImportReviewLine> = {}): ImportReviewLine {
    return line({
      line_id: 'fee1',
      kind: 'fee',
      receive_qty_milli: null,
      proposed: { type: 'non_inventory' },
      ...overrides,
    });
  }

  it('counts receives only for part lines with a non-skip decision AND a positive shipped quantity', () => {
    const lines = [
      line({ line_id: 'a', receive_qty_milli: 5000 }), // add_stock, shipped -> receive
      line({ line_id: 'b', receive_qty_milli: 0 }), // add_stock, zero shipped -> excluded
      line({ line_id: 'c', receive_qty_milli: null }), // add_stock, nothing shipped -> excluded
    ];
    const decisions = new Map<ImportLineId, LineDecision>([
      ['a', { type: 'add_stock', part_id: 'p1' }],
      ['b', { type: 'add_stock', part_id: 'p1' }],
      ['c', { type: 'add_stock', part_id: 'p1' }],
    ]);
    const summary = summarizeDecisions(lines, decisions);
    expect(summary.receives).toBe(1);
  });

  it('counts new parts and new variants by decision type', () => {
    const lines = [
      line({ line_id: 'a', receive_qty_milli: 1000 }),
      line({ line_id: 'b', receive_qty_milli: 1000 }),
    ];
    const decisions = new Map<ImportLineId, LineDecision>([
      ['a', { type: 'create_new', draft: completeDraft, variant, listing }],
      ['b', { type: 'add_as_variant', part_id: 'p1', variant, listing }],
    ]);
    const summary = summarizeDecisions(lines, decisions);
    expect(summary.newParts).toBe(1);
    expect(summary.newVariants).toBe(1);
    expect(summary.receives).toBe(2);
  });

  it('counts skipped part lines and never counts them as receives', () => {
    const lines = [line({ line_id: 'a', receive_qty_milli: 1000 })];
    const decisions = new Map<ImportLineId, LineDecision>([['a', { type: 'skip' }]]);
    const summary = summarizeDecisions(lines, decisions);
    expect(summary.skipped).toBe(1);
    expect(summary.receives).toBe(0);
  });

  it('counts every non-part line as non-inventory, independent of the decisions map', () => {
    const lines = [line({ line_id: 'a' }), feeLine(), feeLine({ line_id: 'fee2', kind: 'tariff' })];
    const summary = summarizeDecisions(lines, new Map());
    expect(summary.nonInventoryLines).toBe(2);
  });

  it('flags hasIncompleteDraft when any part line is an incomplete create_new, and only then', () => {
    const lines = [line({ line_id: 'a' }), line({ line_id: 'b' })];
    const incomplete = new Map<ImportLineId, LineDecision>([
      ['a', { type: 'create_new', draft: placeholderPartDraft(line()), variant, listing }],
      ['b', { type: 'skip' }],
    ]);
    expect(summarizeDecisions(lines, incomplete).hasIncompleteDraft).toBe(true);

    const complete = new Map<ImportLineId, LineDecision>([
      ['a', { type: 'create_new', draft: completeDraft, variant, listing }],
      ['b', { type: 'skip' }],
    ]);
    expect(summarizeDecisions(lines, complete).hasIncompleteDraft).toBe(false);
  });

  it('a part line with no decision entry yet contributes nothing (defensive — ImportReview always seeds one)', () => {
    const lines = [line({ line_id: 'a', receive_qty_milli: 1000 })];
    const summary = summarizeDecisions(lines, new Map());
    expect(summary.receives).toBe(0);
    expect(summary.skipped).toBe(0);
  });
});
