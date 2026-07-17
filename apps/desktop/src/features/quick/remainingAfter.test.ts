import { describe, expect, it } from 'vitest';

import type { PartStockRow } from '../../bindings.gen';
import { formatRemainingAfter, previewAfter, wouldGoNegative } from './remainingAfter';

function stock(overrides: Partial<PartStockRow> = {}): PartStockRow {
  return {
    available: 40_000,
    reserved: 0,
    checked_out: 0,
    lifetime_received: 40_000,
    lifetime_consumed: 0,
    ...overrides,
  };
}

describe('previewAfter', () => {
  it('receive adds to available only', () => {
    expect(previewAfter('receive', stock({ available: 40_000 }), 10_000)).toEqual({
      available: 50_000,
      reserved: 0,
      checkedOut: 0,
    });
  });

  it('consume_available subtracts from available only (design example: 40 - 5 = 35)', () => {
    expect(previewAfter('consume_available', stock({ available: 40_000 }), 5_000)).toEqual({
      available: 35_000,
      reserved: 0,
      checkedOut: 0,
    });
  });

  it('reserve moves available into reserved', () => {
    expect(previewAfter('reserve', stock({ available: 40_000, reserved: 5_000 }), 8_000)).toEqual({
      available: 32_000,
      reserved: 13_000,
      checkedOut: 0,
    });
  });

  it('release_reservation moves reserved back into available', () => {
    expect(
      previewAfter('release_reservation', stock({ available: 10_000, reserved: 8_000 }), 3_000),
    ).toEqual({ available: 13_000, reserved: 5_000, checkedOut: 0 });
  });

  it('check_out moves available into checked_out', () => {
    expect(previewAfter('check_out', stock({ available: 20_000, checked_out: 0 }), 5_000)).toEqual({
      available: 15_000,
      reserved: 0,
      checkedOut: 5_000,
    });
  });

  it('return moves checked_out back into available', () => {
    expect(previewAfter('return', stock({ available: 10_000, checked_out: 6_000 }), 6_000)).toEqual(
      { available: 16_000, reserved: 0, checkedOut: 0 },
    );
  });
});

describe('wouldGoNegative', () => {
  it('is true when consuming more than available', () => {
    expect(wouldGoNegative('consume_available', stock({ available: 2_000 }), 5_000)).toBe(true);
  });

  it('is false when there is enough stock in the source pool', () => {
    expect(wouldGoNegative('consume_available', stock({ available: 5_000 }), 2_000)).toBe(false);
    expect(wouldGoNegative('consume_available', stock({ available: 5_000 }), 5_000)).toBe(false);
  });

  it('is always false for receive (no source pool to overdraw)', () => {
    expect(wouldGoNegative('receive', stock({ available: 0 }), 1_000_000)).toBe(false);
  });

  it('checks the correct source pool per action (reserved for release, checked_out for return)', () => {
    expect(wouldGoNegative('release_reservation', stock({ reserved: 2_000 }), 5_000)).toBe(true);
    expect(wouldGoNegative('return', stock({ checked_out: 2_000 }), 5_000)).toBe(true);
    expect(wouldGoNegative('check_out', stock({ available: 2_000 }), 5_000)).toBe(true);
  });
});

describe('formatRemainingAfter', () => {
  it('shows only the touched pool for a single-pool op (design example: "35 available after")', () => {
    expect(
      formatRemainingAfter('consume_available', stock({ available: 40_000 }), 5_000, 'each'),
    ).toBe('35 available after');
    expect(formatRemainingAfter('receive', stock({ available: 40_000 }), 5_000, 'each')).toBe(
      '45 available after',
    );
  });

  it('shows both touched pools, ordered available/reserved/checked-out, for a two-pool op', () => {
    expect(
      formatRemainingAfter('reserve', stock({ available: 40_000, reserved: 5_000 }), 8_000, 'each'),
    ).toBe('32 available, 13 reserved after');
    expect(
      formatRemainingAfter(
        'release_reservation',
        stock({ available: 10_000, reserved: 8_000 }),
        3_000,
        'each',
      ),
    ).toBe('13 available, 5 reserved after');
    expect(
      formatRemainingAfter(
        'check_out',
        stock({ available: 20_000, checked_out: 0 }),
        5_000,
        'each',
      ),
    ).toBe('15 available, 5 checked out after');
    expect(
      formatRemainingAfter(
        'return',
        stock({ available: 10_000, checked_out: 6_000 }),
        6_000,
        'each',
      ),
    ).toBe('16 available, 0 checked out after');
  });

  it('formats with the part unit (continuous units, e.g. meters of wire)', () => {
    expect(
      formatRemainingAfter('consume_available', stock({ available: 10_500 }), 2_000, 'meter'),
    ).toBe('8.5 m available after');
  });
});
