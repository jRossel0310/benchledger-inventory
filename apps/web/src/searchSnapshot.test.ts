import { parseSnapshot, type Snapshot, type SnapshotPart } from '@ei/shared';
import { describe, expect, it } from 'vitest';
import sample from '../../../packages/shared/fixtures/sample-snapshot.json';
import { searchSnapshot } from './searchSnapshot';

const snapshot = parseSnapshot(sample);
if (snapshot === null) throw new Error('sample-snapshot.json failed to parse');

function names(result: { parts: SnapshotPart[] }): string[] {
  return result.parts.map((p) => p.displayName);
}

/** A minimal hand-built snapshot for cases the generated fixture doesn't
 * cover (its variants all have `datasheet_url: null`). */
function tinySnapshot(parts: SnapshotPart[]): Snapshot {
  return {
    publishedAt: '2026-07-19T00:00:00Z',
    schemaVersion: '1',
    parts,
    bins: [],
    projects: [],
  };
}

function makePart(overrides: Partial<SnapshotPart>): SnapshotPart {
  return {
    id: 'IDX00000000000000000000000',
    displayName: 'Test part',
    category: 'Test',
    description: '',
    tags: [],
    bin: null,
    publicNotes: '',
    usageBehavior: 'usually_consumed',
    quantityUnit: 'each',
    lowStockThresholdMilli: null,
    lowStock: false,
    stock: { availableMilli: 1000, reservedMilli: 0, checkedOutMilli: 0 },
    attributes: [],
    dimensions: [],
    variants: [],
    ...overrides,
  };
}

describe('searchSnapshot', () => {
  it('returns every part with no unsupported filters for the empty query', () => {
    const result = searchSnapshot(snapshot, '');
    expect(result.parts).toHaveLength(snapshot.parts.length);
    expect(result.unsupported).toEqual([]);
  });

  it('AND-matches free-text terms case-insensitively across name fields', () => {
    expect(names(searchSnapshot(snapshot, 'WATCH crystal'))).toEqual(['32.768kHz watch crystal']);
    // Terms must ALL match -- 'watch resistor' hits nothing.
    expect(searchSnapshot(snapshot, 'watch resistor').parts).toHaveLength(0);
  });

  it('strips literal quote characters from free-text terms', () => {
    // The shared grammar keeps a bare quoted term's surrounding quotes in
    // `.text` (documented quirk); desktop FTS strips them at tokenization,
    // so the web mirror must too. Neither side has phrase semantics.
    // Same three hits as an unquoted `crystal` (the 22pF cap's description
    // is "Crystal load capacitor") -- quotes must not change the result.
    expect(names(searchSnapshot(snapshot, '"crystal"'))).toEqual([
      '16MHz HC-49 crystal',
      '22pF 0603 C0G capacitor',
      '32.768kHz watch crystal',
    ]);
    const tiny = tinySnapshot([
      makePart({ id: 'IDX00000000000000000000010', displayName: 'Watch crystal 0603' }),
      makePart({ id: 'IDX00000000000000000000011', displayName: 'Watch crystal 0805' }),
    ]);
    expect(names(searchSnapshot(tiny, '"watch crystal" 0603'))).toEqual(['Watch crystal 0603']);
  });

  it('matches free text against attribute keys, attribute text, and dimension names', () => {
    const tiny = tinySnapshot([
      makePart({
        id: 'IDX00000000000000000000012',
        displayName: 'Ceramic capacitor',
        attributes: [
          {
            key: 'dielectric',
            label: 'Dielectric',
            originalText: 'X7R',
            normalizedValue: null,
            canonicalUnit: null,
          },
        ],
      }),
      makePart({
        id: 'IDX00000000000000000000013',
        displayName: 'Machined standoff',
        dimensions: [
          { group: 'body', name: 'wingspan', valueNum: 12, displayUnit: 'mm', normalizedValue: 12 },
        ],
      }),
    ]);
    // 'X7R' occurs ONLY in an attribute's originalText, 'dielectric' only
    // in its key, 'wingspan' only in a dimension name -- all indexed by the
    // desktop's refresh_search_text, so free text must find them here too.
    expect(names(searchSnapshot(tiny, 'x7r'))).toEqual(['Ceramic capacitor']);
    expect(names(searchSnapshot(tiny, 'dielectric'))).toEqual(['Ceramic capacitor']);
    expect(names(searchSnapshot(tiny, 'wingspan'))).toEqual(['Machined standoff']);
  });

  it('matches free text against variant MPNs and supplier SKUs', () => {
    expect(names(searchSnapshot(snapshot, 'RC0603FR'))).toEqual(['10k 0603 1% resistor']);
    expect(names(searchSnapshot(snapshot, '1050-1024'))).toEqual(['Uno-style development board']);
  });

  it('filters bin: by exact case-insensitive label', () => {
    expect(names(searchSnapshot(snapshot, 'bin:a4'))).toEqual(['100R 0603 1% resistor']);
    // Exact, not substring: 'A' alone matches no bin.
    expect(searchSnapshot(snapshot, 'bin:A').parts).toHaveLength(0);
  });

  it('filters category: by exact case-insensitive name', () => {
    expect(searchSnapshot(snapshot, 'category:capacitor').parts).toHaveLength(5);
  });

  it('applies numeric stock ops in whole units against milli values', () => {
    const available = searchSnapshot(snapshot, 'available:>=500');
    // 10k resistor has 450 available (+50 reserved): excluded here...
    expect(names(available)).not.toContain('10k 0603 1% resistor');
    expect(names(available)).toContain('100nF 0603 X7R capacitor');
    // ...but included by stock: (the available+reserved+checked-out sum).
    expect(names(searchSnapshot(snapshot, 'stock:>=500'))).toContain('10k 0603 1% resistor');
    expect(names(searchSnapshot(snapshot, 'reserved:>=50'))).toEqual(['10k 0603 1% resistor']);
    expect(names(searchSnapshot(snapshot, 'checked_out:>=2'))).toEqual([
      '5mm red LED',
      'TO-220 clip-on heatsink',
    ]);
  });

  it('matches unit-equivalent attribute values (0.1uF == 100nF)', () => {
    expect(names(searchSnapshot(snapshot, 'capacitance:0.1uF'))).toEqual([
      '100nF 0603 X7R capacitor',
    ]);
    expect(names(searchSnapshot(snapshot, 'capacitance:100nF'))).toEqual([
      '100nF 0603 X7R capacitor',
    ]);
  });

  it('applies attribute comparison and range ops on normalized values', () => {
    expect(names(searchSnapshot(snapshot, 'resistance:>=10k'))).toEqual([
      '10k 0603 1% resistor',
      '22k 0603 1% resistor',
      '47k 0603 1% resistor',
    ]);
    expect(names(searchSnapshot(snapshot, 'capacitance:10nF..1uF'))).toEqual([
      '100nF 0603 X7R capacitor',
      '1uF 0805 X5R capacitor',
    ]);
    expect(names(searchSnapshot(snapshot, 'voltage_rating:>=25V'))).toEqual([
      '100nF 0603 X7R capacitor',
      '1nF 0603 C0G capacitor',
      '22pF 0603 C0G capacitor',
    ]);
  });

  it('matches unit-less attributes by case-insensitive original text', () => {
    expect(names(searchSnapshot(snapshot, 'led_color:blue'))).toEqual(['5mm blue LED']);
    expect(searchSnapshot(snapshot, 'mounting_style:THT').parts.length).toBeGreaterThan(0);
  });

  it('supports has:dimensions and has:datasheet', () => {
    expect(names(searchSnapshot(snapshot, 'has:dimensions'))).toEqual([
      'ABS project enclosure',
      'Uno-style development board',
    ]);
    // The fixture's variants all have datasheet_url: null.
    expect(searchSnapshot(snapshot, 'has:datasheet').parts).toHaveLength(0);
    const withSheet = tinySnapshot([
      makePart({
        id: 'IDX00000000000000000000001',
        displayName: 'With datasheet',
        variants: [
          {
            id: 'IDX00000000000000000000002',
            manufacturer: 'Acme',
            mpn: 'ACME-1',
            package: null,
            lifecycle: null,
            datasheetUrl: 'https://example.com/acme-1.pdf',
            productUrl: null,
            isPreferred: true,
            listings: [],
          },
        ],
      }),
      makePart({ id: 'IDX00000000000000000000003', displayName: 'Without datasheet' }),
    ]);
    expect(names(searchSnapshot(withSheet, 'has:datasheet'))).toEqual(['With datasheet']);
  });

  it('applies the low-stock flag', () => {
    expect(names(searchSnapshot(snapshot, 'low stock'))).toEqual([
      '32.768kHz watch crystal',
      'IRF9540 P-channel power MOSFET',
    ]);
    expect(names(searchSnapshot(snapshot, 'is:low'))).toEqual([
      '32.768kHz watch crystal',
      'IRF9540 P-channel power MOSFET',
    ]);
  });

  it('surfaces unsupported filter keys instead of silently dropping them', () => {
    const result = searchSnapshot(snapshot, 'resistor project:amp');
    expect(result.unsupported).toEqual(['project:amp']);
    // The unsupported filter is ignored -- the rest of the query still runs.
    expect(result.parts.length).toBeGreaterThan(0);
    expect(names(result).every((n) => n.toLowerCase().includes('resistor'))).toBe(true);

    expect(searchSnapshot(snapshot, 'is:archived').unsupported).toEqual(['is:archived']);
    expect(searchSnapshot(snapshot, 'foo:bar').unsupported).toEqual(['foo:bar']);
    expect(searchSnapshot(snapshot, 'has:footprint').unsupported).toEqual(['has:footprint']);
  });

  it('reconstructs the operator in unsupported chips', () => {
    expect(searchSnapshot(snapshot, 'weight:>=5g').unsupported).toEqual(['weight:>=5g']);
  });

  it('surfaces filters whose value cannot be parsed for the attribute kind', () => {
    const result = searchSnapshot(snapshot, 'resistance:blue');
    expect(result.unsupported).toEqual(['resistance:blue']);
    expect(result.parts).toHaveLength(snapshot.parts.length);
  });

  it('treats is:active as a no-op (the snapshot only contains active parts)', () => {
    const result = searchSnapshot(snapshot, 'is:active');
    expect(result.parts).toHaveLength(snapshot.parts.length);
    expect(result.unsupported).toEqual([]);
  });
});
