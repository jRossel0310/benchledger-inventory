import { describe, expect, it } from 'vitest';
import { parseQuery, type ParsedQuery } from './query';
import fixture from '../fixtures/query-cases.json';

interface FixtureFilter {
  key: string;
  op: string;
  value: string;
}
interface FixtureFlags {
  low_stock: boolean;
  archived: boolean | null;
}
interface FixtureParsed {
  text: string;
  filters: FixtureFilter[];
  flags: FixtureFlags;
}
interface FixtureCase {
  input: string;
  parsed: FixtureParsed;
}
interface QueryFixture {
  comment: string;
  cases: FixtureCase[];
}

const fx = fixture as QueryFixture;

/** Converts a fixture case's Rust-shaped `parsed` object (snake_case
 * `low_stock`) into this module's camelCase `ParsedQuery` shape. The ONE
 * rename in the whole tree -- see query.ts's field-mapping doc header --
 * lives here, not in production code. */
function fromFixture(p: FixtureParsed): ParsedQuery {
  return {
    text: p.text,
    filters: p.filters.map((f) => ({
      key: f.key,
      op: f.op as ParsedQuery['filters'][number]['op'],
      value: f.value,
    })),
    flags: { lowStock: p.flags.low_stock, archived: p.flags.archived },
  };
}

describe('parseQuery (fixture-driven, mirrors search.rs::parse_query)', () => {
  it('has a non-trivial number of fixture cases (truncation guard)', () => {
    expect(fx.cases.length).toBeGreaterThanOrEqual(25);
  });

  for (const c of fx.cases) {
    it(`parses ${JSON.stringify(c.input)} identically to parse_query`, () => {
      expect(parseQuery(c.input)).toEqual(fromFixture(c.parsed));
    });
  }
});
