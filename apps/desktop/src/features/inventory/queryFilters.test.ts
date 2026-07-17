/**
 * Pure filter <-> search-query translation, TDD'd against
 * `inventory_core::search::parse_query`'s actual grammar (see
 * `crates/inventory-core/src/search.rs`) so every fragment these helpers
 * write is one the backend genuinely understands: `category:X` (quoted when
 * `X` has whitespace), the bare two-token `low stock` flag, `is:archived`,
 * `has:datasheet`, `has:dimensions`.
 */
import { describe, expect, it } from 'vitest';

import {
  parseActiveFilters,
  withArchived,
  withCategory,
  withHasDatasheet,
  withHasDimensions,
  withLowStock,
} from './queryFilters';

describe('withLowStock', () => {
  it('appends "low stock" to an empty query', () => {
    expect(withLowStock('', true)).toBe('low stock');
  });

  it('appends "low stock" after existing free text', () => {
    expect(withLowStock('10k', true)).toBe('10k low stock');
  });

  it('is a no-op when already present and enabling again', () => {
    expect(withLowStock('10k low stock', true)).toBe('10k low stock');
  });

  it('removes "low stock" when disabling', () => {
    expect(withLowStock('10k low stock', false)).toBe('10k');
  });

  it('disabling when absent is a no-op', () => {
    expect(withLowStock('10k', false)).toBe('10k');
  });

  it('does not touch an unrelated bare "low" that is not followed by "stock"', () => {
    expect(withLowStock('low power', false)).toBe('low power');
  });
});

describe('withArchived', () => {
  it('appends is:archived', () => {
    expect(withArchived('10k', true)).toBe('10k is:archived');
  });

  it('removes is:archived when disabling', () => {
    expect(withArchived('10k is:archived', false)).toBe('10k');
  });

  it('does not duplicate when already present', () => {
    expect(withArchived('is:archived', true)).toBe('is:archived');
  });
});

describe('withHasDatasheet / withHasDimensions', () => {
  it('appends has:datasheet', () => {
    expect(withHasDatasheet('10k', true)).toBe('10k has:datasheet');
  });

  it('removes has:datasheet when disabling', () => {
    expect(withHasDatasheet('10k has:datasheet', false)).toBe('10k');
  });

  it('appends has:dimensions', () => {
    expect(withHasDimensions('10k', true)).toBe('10k has:dimensions');
  });

  it('removes has:dimensions when disabling', () => {
    expect(withHasDimensions('10k has:dimensions', false)).toBe('10k');
  });

  it('composes independently of each other', () => {
    const q = withHasDimensions(withHasDatasheet('10k', true), true);
    expect(q).toBe('10k has:datasheet has:dimensions');
  });
});

describe('withCategory', () => {
  it('appends category:X for a single-word category', () => {
    expect(withCategory('', 'Resistor')).toBe('category:Resistor');
  });

  it('quotes a category name containing whitespace', () => {
    expect(withCategory('', 'Voltage regulator')).toBe('category:"Voltage regulator"');
  });

  it('replaces an existing category fragment rather than appending a second one', () => {
    expect(withCategory('category:Resistor', 'Capacitor')).toBe('category:Capacitor');
  });

  it('replaces a quoted existing category fragment with a new quoted one', () => {
    expect(withCategory('category:"Voltage regulator" 10k', 'LED')).toBe('10k category:LED');
  });

  it('removes the category fragment when set to null', () => {
    expect(withCategory('10k category:Resistor', null)).toBe('10k');
  });

  it('preserves free text and other filters around it', () => {
    expect(withCategory('10k low stock', 'Resistor')).toBe('10k low stock category:Resistor');
  });
});

describe('parseActiveFilters', () => {
  it('reports all filters inactive for a plain free-text query', () => {
    expect(parseActiveFilters('10k 0603')).toEqual({
      category: null,
      lowStock: false,
      archived: false,
      hasDatasheet: false,
      hasDimensions: false,
    });
  });

  it('detects every flag in a fully composed query', () => {
    const q = '10k category:"Voltage regulator" low stock is:archived has:datasheet has:dimensions';
    expect(parseActiveFilters(q)).toEqual({
      category: 'Voltage regulator',
      lowStock: true,
      archived: true,
      hasDatasheet: true,
      hasDimensions: true,
    });
  });

  it('unquotes a single-word category the same as a quoted one', () => {
    expect(parseActiveFilters('category:Resistor').category).toBe('Resistor');
  });

  it('round-trips through the with* helpers: toggling on then reading back matches', () => {
    let q = '';
    q = withLowStock(q, true);
    q = withArchived(q, true);
    q = withCategory(q, 'LED');
    const active = parseActiveFilters(q);
    expect(active.lowStock).toBe(true);
    expect(active.archived).toBe(true);
    expect(active.category).toBe('LED');
    expect(active.hasDatasheet).toBe(false);
  });
});
