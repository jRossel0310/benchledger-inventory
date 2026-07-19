import { describe, expect, it } from 'vitest';
import { parseUnitValue, parseWithKind, type UnitKind } from './units';
import fixture from '../fixtures/unit-cases.json';

interface FixtureCase {
  input: string;
  kind: string;
  mantissa: number;
  exp10: number;
}
interface FixtureReject {
  input: string;
  kind: string;
}
interface UnitFixture {
  cases: FixtureCase[];
  equalPairs: [string, string, string][];
  rejects: FixtureReject[];
}

const fx = fixture as unknown as UnitFixture;

const CANONICAL_UNIT_BY_KIND: Record<string, string> = {
  resistance: 'Ω',
  capacitance: 'F',
  inductance: 'H',
  voltage: 'V',
  current: 'A',
  power: 'W',
  frequency: 'Hz',
  length: 'm',
  mass: 'g',
  time: 's',
  percent: '%',
  charge: 'C',
};

function expectedValue(c: FixtureCase): number {
  return c.mantissa * Math.pow(10, c.exp10);
}

/** Compares by relative error rather than `toBeCloseTo`'s fixed absolute
 * precision, which is meaningless across values spanning 1e-12 to 1e6. */
function expectValueEqual(actual: number, expected: number) {
  if (expected === 0) {
    expect(actual).toBe(0);
    return;
  }
  const relErr = Math.abs((actual - expected) / expected);
  expect(relErr).toBeLessThan(1e-9);
}

describe('parseWithKind (fixture-driven, mirrors units.rs::parse_with_kind)', () => {
  it('has a non-trivial number of fixture entries (truncation guard)', () => {
    expect(fx.cases.length).toBeGreaterThanOrEqual(40);
    expect(fx.equalPairs.length).toBeGreaterThanOrEqual(4);
    expect(fx.rejects.length).toBeGreaterThanOrEqual(4);
  });

  for (const c of fx.cases) {
    it(`parses ${JSON.stringify(c.input)} (${c.kind}) to the canonical value`, () => {
      const parsed = parseWithKind(c.input, c.kind as UnitKind);
      expect(parsed).not.toBeNull();
      expectValueEqual(parsed!.value, expectedValue(c));
      expect(parsed!.canonicalUnit).toBe(CANONICAL_UNIT_BY_KIND[c.kind]);
    });
  }

  for (const r of fx.rejects) {
    it(`rejects ${JSON.stringify(r.input)} (${r.kind})`, () => {
      expect(parseWithKind(r.input, r.kind as UnitKind)).toBeNull();
    });
  }

  for (const [a, b, kind] of fx.equalPairs) {
    it(`${JSON.stringify(a)} and ${JSON.stringify(b)} (${kind}) parse equal`, () => {
      const pa = parseWithKind(a, kind as UnitKind);
      const pb = parseWithKind(b, kind as UnitKind);
      expect(pa).not.toBeNull();
      expect(pb).not.toBeNull();
      expectValueEqual(pa!.value, pb!.value);
      expect(pa!.canonicalUnit).toBe(pb!.canonicalUnit);
    });
  }

  // Phase 6's kind-disambiguated pair, called out explicitly (also covered
  // generically by the fixture loop above): the SAME text "5m" means two
  // different things depending on which kind resolves it.
  it('disambiguates "5m" by kind: 5 milliohms (resistance) vs 5 meters (length)', () => {
    const asResistance = parseWithKind('5m', 'resistance');
    const asLength = parseWithKind('5m', 'length');
    expect(asResistance).not.toBeNull();
    expect(asLength).not.toBeNull();
    expectValueEqual(asResistance!.value, 0.005);
    expectValueEqual(asLength!.value, 5);
  });

  it('accepts the U+2126 OHM SIGN as equivalent to U+03A9 GREEK CAPITAL LETTER OMEGA', () => {
    const withOhmSign = parseWithKind('10 kΩ', 'resistance');
    const withOmega = parseWithKind('10 kΩ', 'resistance');
    expect(withOhmSign).not.toBeNull();
    expect(withOmega).not.toBeNull();
    expectValueEqual(withOhmSign!.value, 10000);
    expectValueEqual(withOhmSign!.value, withOmega!.value);
  });
});

describe('parseUnitValue (kind-blind, mirrors units.rs::detect_and_parse)', () => {
  it('is ambiguous (null) for a bare SI-prefixed number with no unit letter', () => {
    // Matches Rust's own detect_and_parse test: "100n"/"10" are ambiguous
    // or symbol-less; every UnitKind's bare-prefix rule matches "10k" or
    // "5m" identically, so no single kind can be inferred.
    expect(parseUnitValue('10k')).toBeNull();
    expect(parseUnitValue('5m')).toBeNull();
    expect(parseUnitValue('500m')).toBeNull();
    expect(parseUnitValue('100n')).toBeNull();
    expect(parseUnitValue('10')).toBeNull();
    expect(parseUnitValue('')).toBeNull();
  });

  it('resolves an unambiguous unit symbol to the right kind', () => {
    const capacitance = parseUnitValue('100nF');
    expect(capacitance).not.toBeNull();
    expectValueEqual(capacitance!.value, 1e-7);
    expect(capacitance!.canonicalUnit).toBe('F');

    const power = parseUnitValue('0.25W');
    expect(power).not.toBeNull();
    expectValueEqual(power!.value, 0.25);

    const voltage = parseUnitValue('3V3');
    expect(voltage).not.toBeNull();
    expectValueEqual(voltage!.value, 3.3);
  });

  it('resolves the U+2126 OHM SIGN unambiguously to resistance', () => {
    const parsed = parseUnitValue('10 kΩ');
    expect(parsed).not.toBeNull();
    expectValueEqual(parsed!.value, 10000);
    expect(parsed!.canonicalUnit).toBe('Ω');
  });
});
