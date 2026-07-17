import { describe, expect, it } from 'vitest';

import type { AttributeDefRow } from '../../bindings.gen';
import { buildMatchCandidate } from './buildMatchCandidate';

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
  def({ key: 'resistance', display_order: 0 }),
  def({ key: 'tolerance', label: 'Tolerance', unit_kind: 'percent', display_order: 1 }),
  def({
    key: 'package',
    label: 'Package',
    data_type: 'text',
    unit_kind: null,
    display_order: 2,
  }),
  def({
    key: 'mounting_style',
    label: 'Mounting style',
    data_type: 'choice',
    unit_kind: null,
    identity: false,
    display_order: 3,
  }),
];

describe('buildMatchCandidate', () => {
  it('returns null when nothing identity-defining has been entered yet', () => {
    expect(
      buildMatchCandidate({
        categoryId: 'c1',
        attributeDefs: RESISTOR_DEFS,
        attributeValues: {},
      }),
    ).toBeNull();

    // A non-identity attribute alone (mounting_style) still isn't enough
    // signal to search on.
    expect(
      buildMatchCandidate({
        categoryId: 'c1',
        attributeDefs: RESISTOR_DEFS,
        attributeValues: { mounting_style: 'SMD' },
      }),
    ).toBeNull();
  });

  it('builds a candidate once an identity attribute is entered, carrying every entered attribute', () => {
    const candidate = buildMatchCandidate({
      categoryId: 'c1',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: { resistance: '10k', mounting_style: 'SMD' },
    });
    expect(candidate).toEqual({
      supplier: null,
      supplier_sku: null,
      manufacturer: null,
      mpn: null,
      category_id: 'c1',
      attributes: [
        ['resistance', '10k'],
        ['mounting_style', 'SMD'],
      ],
      package: null,
    });
  });

  it('is triggered by package or mpn alone, even with no identity attribute entered', () => {
    expect(
      buildMatchCandidate({
        categoryId: 'c1',
        attributeDefs: RESISTOR_DEFS,
        attributeValues: { package: '0603' },
      }),
    ).not.toBeNull();

    expect(
      buildMatchCandidate({
        categoryId: 'c1',
        attributeDefs: RESISTOR_DEFS,
        attributeValues: {},
        variant: {
          manufacturer: 'Yageo',
          mpn: 'RC0603FR-0710KL',
          package: null,
          supplier: '',
          supplierSku: '',
        },
      }),
    ).not.toBeNull();
  });

  it('pulls manufacturer/mpn/supplier/supplier_sku from the primary variant', () => {
    const candidate = buildMatchCandidate({
      categoryId: 'c1',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: { resistance: '10k' },
      variant: {
        manufacturer: 'Yageo',
        mpn: 'RC0603FR-0710KL',
        package: null,
        supplier: 'DigiKey',
        supplierSku: '311-10.0KLRCT-ND',
      },
    });
    expect(candidate).toMatchObject({
      manufacturer: 'Yageo',
      mpn: 'RC0603FR-0710KL',
      supplier: 'DigiKey',
      supplier_sku: '311-10.0KLRCT-ND',
    });
  });

  it('prefers an explicit "package" attribute over the variant package field, never double-counting', () => {
    const candidate = buildMatchCandidate({
      categoryId: 'c1',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: { resistance: '10k', package: '0603' },
      variant: {
        manufacturer: '',
        mpn: '',
        package: '0805',
        supplier: '',
        supplierSku: '',
      },
    });
    expect(candidate?.package).toBe('0603');
  });

  it('falls back to the variant package field when no "package" attribute is entered', () => {
    const candidate = buildMatchCandidate({
      categoryId: 'c1',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: { resistance: '10k' },
      variant: {
        manufacturer: '',
        mpn: '',
        package: '0805',
        supplier: '',
        supplierSku: '',
      },
    });
    expect(candidate?.package).toBe('0805');
  });

  it('trims whitespace and treats a blank category as null', () => {
    const candidate = buildMatchCandidate({
      categoryId: '',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: { resistance: '  10k  ' },
    });
    expect(candidate?.category_id).toBeNull();
    expect(candidate?.attributes).toEqual([['resistance', '10k']]);
  });
});
