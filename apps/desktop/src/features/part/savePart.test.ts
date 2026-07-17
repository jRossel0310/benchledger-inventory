import { describe, expect, it, vi } from 'vitest';

import type { PartDraft, PartRecord, VariantRecord } from '../../bindings.gen';
import { savePart, type SavePartDeps, type SavePartError, type SavePartInput } from './savePart';

function partRecord(overrides: Partial<PartRecord> = {}): PartRecord {
  return {
    id: 'p-new',
    display_name: '10k 0603 1% resistor',
    category_id: 'c1',
    description: '',
    bin_label: null,
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each',
    low_stock_threshold: null,
    public_notes: '',
    private_notes: '',
    metadata_complete: false,
    archived: false,
    created_at: '2026-01-01 00:00:00',
    modified_at: '2026-01-01 00:00:00',
    ...overrides,
  };
}

function draft(): PartDraft {
  return {
    display_name: '10k 0603 1% resistor',
    category_id: 'c1',
    description: '',
    bin_label: 'A12',
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each',
    low_stock_threshold: null,
    public_notes: '',
    private_notes: '',
  };
}

function deps(overrides: Partial<SavePartDeps> = {}) {
  const calls: string[] = [];
  const base: SavePartDeps = {
    createPart: vi.fn(async () => {
      calls.push('create_part');
      return partRecord();
    }),
    updatePart: vi.fn(async () => {
      calls.push('update_part');
    }),
    setAttribute: vi.fn(async (_p, key) => {
      calls.push(`set_attribute:${key}`);
    }),
    setTags: vi.fn(async () => {
      calls.push('set_tags');
    }),
    addDimension: vi.fn(async (_p, d) => {
      calls.push(`add_dimension:${d.name}`);
    }),
    addVariant: vi.fn(async (_p, v) => {
      calls.push(`add_variant:${v.mpn}`);
      return { id: `variant-${v.mpn}` } as unknown as VariantRecord;
    }),
    addSupplierListing: vi.fn(async (variantId, l) => {
      calls.push(`add_supplier_listing:${variantId}:${l.supplier}`);
    }),
    ...overrides,
  };
  return { deps: base, calls };
}

function input(overrides: Partial<SavePartInput> = {}): SavePartInput {
  return {
    mode: { kind: 'create', draft: draft() },
    attributes: [],
    tags: [],
    dimensions: [],
    variants: [],
    ...overrides,
  };
}

describe('savePart — create sequence', () => {
  it('creates the part first, then attributes, tags, dimensions, variants+listings, in order', async () => {
    const { deps: d, calls } = deps();
    const partId = await savePart(
      d,
      input({
        attributes: [
          ['resistance', '10k'],
          ['tolerance', '1%'],
        ],
        tags: ['smd', 'e96'],
        dimensions: [
          {
            group: 'overall',
            name: 'Length',
            raw_value: '1.6mm',
            source: 'datasheet',
            notes: '',
            measured_date: null,
          },
        ],
        variants: [
          {
            draft: {
              manufacturer: 'Yageo',
              mpn: 'RC0603',
              description: '',
              package: '0603',
              datasheet_url: null,
              product_url: null,
              lifecycle: null,
              notes: '',
            },
            listings: [
              {
                supplier: 'DigiKey',
                supplier_sku: 'X',
                product_url: null,
                packaging: null,
                typical_order: null,
                last_unit_price_micros: null,
                currency: null,
                last_purchase_date: null,
              },
            ],
          },
        ],
      }),
    );

    expect(partId).toBe('p-new');
    expect(calls).toEqual([
      'create_part',
      'set_attribute:resistance',
      'set_attribute:tolerance',
      'set_tags',
      'add_dimension:Length',
      'add_variant:RC0603',
      'add_supplier_listing:variant-RC0603:DigiKey',
    ]);
  });

  it('always sets tags even when empty (set_tags replaces the whole set)', async () => {
    const { deps: d } = deps();
    await savePart(d, input());
    expect(d.setTags).toHaveBeenCalledWith('p-new', []);
  });

  it('passes each attribute to set_attribute keyed on the created part id', async () => {
    const { deps: d } = deps();
    await savePart(d, input({ attributes: [['resistance', '10k']] }));
    expect(d.setAttribute).toHaveBeenCalledWith('p-new', 'resistance', '10k');
  });
});

describe('savePart — update sequence', () => {
  it('calls update_part (not create_part) and reuses the existing id', async () => {
    const { deps: d, calls } = deps();
    const record = partRecord({ id: 'p-existing', display_name: 'renamed' });
    const partId = await savePart(
      d,
      input({ mode: { kind: 'update', record }, attributes: [['resistance', '22k']] }),
    );

    expect(partId).toBe('p-existing');
    expect(d.createPart).not.toHaveBeenCalled();
    expect(d.updatePart).toHaveBeenCalledWith(record);
    expect(calls).toEqual(['update_part', 'set_attribute:resistance', 'set_tags']);
  });
});

describe('savePart — non-atomic partial-failure contract', () => {
  it('throws with partId=null when create_part itself fails (nothing written)', async () => {
    const { deps: d } = deps({
      createPart: vi.fn(async () => {
        throw { code: 'category_not_found', message: 'no such category' };
      }),
    });

    await expect(savePart(d, input())).rejects.toMatchObject({
      step: 'create_part',
      partId: null,
    });
    expect(d.setTags).not.toHaveBeenCalled();
  });

  it('throws with the created partId and failing step when a later mutation fails (part persists)', async () => {
    const { deps: d } = deps({
      setAttribute: vi.fn(async () => {
        throw { code: 'invalid_attribute_value', message: 'nope' };
      }),
    });

    const failure = (await savePart(d, input({ attributes: [['resistance', 'bad']] })).catch(
      (e) => e,
    )) as SavePartError;

    expect(failure.partId).toBe('p-new');
    expect(failure.step).toBe('set_attribute:resistance');
    // The part was created before the attribute failed — not rolled back.
    expect(d.createPart).toHaveBeenCalled();
    // …and the sequence stopped at the failure (tags never ran).
    expect(d.setTags).not.toHaveBeenCalled();
  });

  it('reports a listing failure against the part, without re-wrapping across the variant loop', async () => {
    const { deps: d } = deps({
      addSupplierListing: vi.fn(async () => {
        throw { code: 'sqlite', message: 'boom' };
      }),
    });

    const failure = (await savePart(
      d,
      input({
        variants: [
          {
            draft: {
              manufacturer: 'Yageo',
              mpn: 'RC0603',
              description: '',
              package: null,
              datasheet_url: null,
              product_url: null,
              lifecycle: null,
              notes: '',
            },
            listings: [
              {
                supplier: 'DigiKey',
                supplier_sku: 'X',
                product_url: null,
                packaging: null,
                typical_order: null,
                last_unit_price_micros: null,
                currency: null,
                last_purchase_date: null,
              },
            ],
          },
        ],
      }),
    ).catch((e) => e)) as SavePartError;

    expect(failure.step).toBe('add_supplier_listing:DigiKey');
    expect(failure.partId).toBe('p-new');
  });
});
