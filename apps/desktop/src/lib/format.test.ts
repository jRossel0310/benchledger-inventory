import { describe, expect, it } from 'vitest';
import {
  errorHint,
  errorMessage,
  formatGroupKind,
  formatPrice,
  formatQuantity,
  formatTimestamp,
} from './format';

describe('formatQuantity', () => {
  it('renders "each" quantities as whole units with no unit suffix', () => {
    expect(formatQuantity(1000, 'each')).toBe('1');
    expect(formatQuantity(2000, 'each')).toBe('2');
    expect(formatQuantity(0, 'each')).toBe('0');
  });

  it('renders meter quantities with decimals and an "m" suffix', () => {
    expect(formatQuantity(1500, 'meter')).toBe('1.5 m');
    expect(formatQuantity(1000, 'meter')).toBe('1 m');
    expect(formatQuantity(1234, 'meter')).toBe('1.234 m');
  });

  it('renders foot quantities with decimals and an "ft" suffix', () => {
    expect(formatQuantity(2500, 'foot')).toBe('2.5 ft');
  });

  it('accepts display-abbreviation unit strings directly', () => {
    expect(formatQuantity(1500, 'm')).toBe('1.5 m');
    expect(formatQuantity(2500, 'ft')).toBe('2.5 ft');
  });
});

describe('formatPrice', () => {
  it('formats micros as a currency-symbol-prefixed decimal', () => {
    expect(formatPrice(440000, 'USD')).toBe('$0.44');
  });

  it('renders null prices as an em dash', () => {
    expect(formatPrice(null, 'USD')).toBe('—');
  });

  it('falls back to the currency code when the symbol is unknown', () => {
    expect(formatPrice(1500000, 'XYZ')).toBe('XYZ 1.50');
  });

  it('formats a whole-dollar price without a currency code as a plain number', () => {
    expect(formatPrice(1000000, null)).toBe('1.00');
  });
});

describe('formatTimestamp', () => {
  it('renders a SQLite UTC timestamp as a readable local string', () => {
    const out = formatTimestamp('2026-07-16 12:34:56');
    expect(out).toMatch(/2026/);
    expect(out).not.toBe('2026-07-16 12:34:56');
  });

  it('renders a full ISO timestamp with an explicit offset', () => {
    const out = formatTimestamp('2026-01-02T03:04:05Z');
    expect(out).toMatch(/2026/);
  });

  it('falls back to the raw string when it cannot be parsed', () => {
    expect(formatTimestamp('not-a-date')).toBe('not-a-date');
  });
});

describe('errorMessage', () => {
  it('pulls .message from a CommandError-shaped object', () => {
    expect(
      errorMessage({ code: 'insufficient_stock', message: 'insufficient stock: only 2 available' }),
    ).toBe('insufficient stock: only 2 available');
  });

  it('unwraps a native Error', () => {
    expect(errorMessage(new Error('boom'))).toBe('boom');
  });

  it('stringifies anything else', () => {
    expect(errorMessage('plain string')).toBe('plain string');
    expect(errorMessage(42)).toBe('42');
  });
});

describe('errorHint', () => {
  it('returns a recovery hint for a known error code', () => {
    const hint = errorHint('insufficient_stock');
    expect(hint).toBeTruthy();
    expect(typeof hint).toBe('string');
  });

  it('returns null for an unknown error code', () => {
    expect(errorHint('some_made_up_code')).toBeNull();
  });
});

describe('formatGroupKind', () => {
  it('title-cases snake_case group kinds', () => {
    expect(formatGroupKind('receive_batch')).toBe('Receive Batch');
    expect(formatGroupKind('reserve_bom')).toBe('Reserve Bom');
  });

  it('renders a reversal kind as "Reversal of" the original', () => {
    expect(formatGroupKind('reverse:receive_batch')).toBe('Reversal of Receive Batch');
  });

  it('handles a single-word kind', () => {
    expect(formatGroupKind('batch')).toBe('Batch');
  });
});
