import { describe, expect, it } from 'vitest';
import { parseSnapshot, parseSnapshotHeader } from './snapshot';
import sample from '../fixtures/sample-snapshot.json';

describe('parseSnapshot (fixture-driven, mirrors inventory-sync::snapshot::Snapshot)', () => {
  it('parses the generated sample snapshot in full', () => {
    const parsed = parseSnapshot(sample);
    expect(parsed).not.toBeNull();
    expect(parsed!.schemaVersion).toBe('1');
    expect(parsed!.publishedAt).toBe('2026-01-01T00:00:00Z');
  });

  it('has a non-trivial number of parts/bins (fixture truncation guard)', () => {
    const parsed = parseSnapshot(sample)!;
    expect(parsed.parts.length).toBeGreaterThanOrEqual(20);
    expect(parsed.bins.length).toBeGreaterThanOrEqual(10);
    expect(parsed.projects.length).toBeGreaterThanOrEqual(1);
  });

  it('maps every part field snake_case -> camelCase', () => {
    const parsed = parseSnapshot(sample)!;
    const raw = (sample as { parts: Record<string, unknown>[] }).parts[0]!;
    const part = parsed.parts[0]!;
    expect(part.id).toBe(raw.id);
    expect(part.displayName).toBe(raw.display_name);
    expect(part.usageBehavior).toBe(raw.usage_behavior);
    expect(part.quantityUnit).toBe(raw.quantity_unit);
    expect(part.lowStockThresholdMilli).toBe(raw.low_stock_threshold_milli);
    expect(part.publicNotes).toBe(raw.public_notes);
    const rawStock = raw.stock as Record<string, unknown>;
    expect(part.stock.availableMilli).toBe(rawStock.available_milli);
    expect(part.stock.reservedMilli).toBe(rawStock.reserved_milli);
    expect(part.stock.checkedOutMilli).toBe(rawStock.checked_out_milli);
  });

  it('finds at least one part with attributes, dimensions, and a variant with listings', () => {
    const parsed = parseSnapshot(sample)!;
    const withAttrs = parsed.parts.find((p) => p.attributes.length > 0);
    expect(withAttrs).toBeDefined();
    expect(withAttrs!.attributes[0]).toEqual(
      expect.objectContaining({ key: expect.any(String), label: expect.any(String) }),
    );

    const withVariant = parsed.parts.find((p) => p.variants.length > 0);
    expect(withVariant).toBeDefined();
    expect(withVariant!.variants[0]).toEqual(
      expect.objectContaining({ manufacturer: expect.any(String), mpn: expect.any(String) }),
    );
  });

  it("cross-references a project's partIds against real part ids", () => {
    const parsed = parseSnapshot(sample)!;
    const allPartIds = new Set(parsed.parts.map((p) => p.id));
    const withParts = parsed.projects.find((p) => p.partIds.length > 0);
    expect(withParts).toBeDefined();
    for (const id of withParts!.partIds) {
      expect(allPartIds.has(id)).toBe(true);
    }
  });

  it('tolerates unknown extra fields at every nesting level (forward-compat)', () => {
    const withExtras = {
      ...sample,
      an_unknown_future_field: 'ignored',
      parts: (sample as { parts: Record<string, unknown>[] }).parts.map((p) => ({
        ...p,
        another_unknown_field: 42,
      })),
    };
    const parsed = parseSnapshot(withExtras);
    expect(parsed).not.toBeNull();
    expect(parsed!.parts.length).toBe((sample as { parts: unknown[] }).parts.length);
  });

  it('rejects non-object bodies', () => {
    expect(parseSnapshot(null)).toBeNull();
    expect(parseSnapshot(undefined)).toBeNull();
    expect(parseSnapshot('a string')).toBeNull();
    expect(parseSnapshot(42)).toBeNull();
    expect(parseSnapshot([])).toBeNull();
  });

  it('rejects a body missing required top-level fields', () => {
    expect(parseSnapshot({})).toBeNull();
    expect(parseSnapshot({ published_at: '2026-01-01T00:00:00Z' })).toBeNull();
    const { parts: _parts, ...withoutParts } = sample as Record<string, unknown>;
    expect(parseSnapshot(withoutParts)).toBeNull();
  });

  it('rejects a body whose parts array is not actually an array', () => {
    expect(parseSnapshot({ ...sample, parts: 'not an array' })).toBeNull();
  });

  it('rejects a body with one malformed element in an otherwise-valid array', () => {
    const raw = sample as { parts: Record<string, unknown>[] };
    const broken = {
      ...sample,
      parts: [...raw.parts.slice(1), { ...raw.parts[0], display_name: 42 }],
    };
    expect(parseSnapshot(broken)).toBeNull();
  });
});

describe('parseSnapshotHeader (thin wrapper over parseSnapshot)', () => {
  it('derives formatVersion/publishedAt/partCount from a full snapshot', () => {
    const header = parseSnapshotHeader(sample);
    const parsed = parseSnapshot(sample)!;
    expect(header).toEqual({
      formatVersion: Number(parsed.schemaVersion),
      publishedAt: parsed.publishedAt,
      partCount: parsed.parts.length,
    });
  });

  it('returns null when the full snapshot fails to parse', () => {
    expect(parseSnapshotHeader({ formatVersion: 1, publishedAt: 'x', partCount: 1 })).toBeNull();
    expect(parseSnapshotHeader(null)).toBeNull();
    expect(parseSnapshotHeader({})).toBeNull();
  });
});
