import { describe, expect, it } from 'vitest';

import type { AttributeDefRow } from '../../bindings.gen';
import { suggestedName } from './suggestedName';

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
  def({
    key: 'tolerance',
    label: 'Tolerance',
    unit_kind: 'percent',
    display_order: 1,
  }),
  def({
    key: 'power_rating',
    label: 'Power rating',
    unit_kind: 'power',
    display_order: 2,
  }),
  def({
    key: 'package',
    label: 'Package',
    data_type: 'text',
    unit_kind: null,
    display_order: 3,
  }),
  def({
    key: 'mounting_style',
    label: 'Mounting style',
    data_type: 'choice',
    unit_kind: null,
    identity: false,
    display_order: 4,
  }),
];

describe('suggestedName', () => {
  it('joins entered identity attribute values, in display order, with the lowercased category name', () => {
    const name = suggestedName({
      categoryName: 'Resistor',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: {
        resistance: '10k',
        tolerance: '1%',
        power_rating: '1/4W',
        package: '0603',
        mounting_style: 'SMD',
      },
    });
    expect(name).toBe('10k 1% 1/4W 0603 resistor');
  });

  it('skips identity attributes that have not been entered yet', () => {
    const name = suggestedName({
      categoryName: 'Resistor',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: { resistance: '10k', package: '0603' },
    });
    expect(name).toBe('10k 0603 resistor');
  });

  it('ignores non-identity attributes entirely, even when filled in', () => {
    const name = suggestedName({
      categoryName: 'Resistor',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: { mounting_style: 'SMD' },
    });
    expect(name).toBe('resistor');
  });

  it('trims whitespace-only entered values as if they were empty', () => {
    const name = suggestedName({
      categoryName: 'Resistor',
      attributeDefs: RESISTOR_DEFS,
      attributeValues: { resistance: '10k', tolerance: '   ' },
    });
    expect(name).toBe('10k resistor');
  });

  it('returns an empty string when no category is selected yet', () => {
    expect(
      suggestedName({ categoryName: '', attributeDefs: RESISTOR_DEFS, attributeValues: {} }),
    ).toBe('');
  });

  it('returns just the lowercased category name when no attributes are entered', () => {
    expect(
      suggestedName({ categoryName: 'Capacitor', attributeDefs: [], attributeValues: {} }),
    ).toBe('capacitor');
  });
});
