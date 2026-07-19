import { describe, expect, it } from 'vitest';

import type { ImportReviewLine, LineDecision, ProposedAction } from '../../bindings.gen';
import {
  decisionFromProposed,
  isCreateNewIncomplete,
  placeholderPartDraft,
  prefillListing,
  prefillVariant,
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
