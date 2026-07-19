import { describe, expect, it } from 'vitest';
import { loadSnapshot } from './snapshot';

// Phase 6 Task 2: `loaded` carries the full `Snapshot` (full schema,
// @ei/shared's snapshot.ts), not a header summary -- empty parts/bins/
// projects arrays are a minimal-but-fully-valid snapshot body.
const ok = {
  published_at: '2026-07-14T00:00:00Z',
  schema_version: '1',
  parts: [],
  bins: [],
  projects: [],
};

function fakeFetch(status: number, body?: unknown) {
  return async () => ({ ok: status === 200, status, json: async () => body }) as Response;
}

describe('loadSnapshot', () => {
  it('returns none when the snapshot is missing (404)', async () => {
    expect(await loadSnapshot(fakeFetch(404))).toEqual({ kind: 'none' });
  });

  it('returns loaded with the fully parsed snapshot', async () => {
    const state = await loadSnapshot(fakeFetch(200, ok));
    expect(state).toEqual({
      kind: 'loaded',
      snapshot: {
        publishedAt: '2026-07-14T00:00:00Z',
        schemaVersion: '1',
        parts: [],
        bins: [],
        projects: [],
      },
    });
  });

  it('returns invalid for malformed JSON shape', async () => {
    expect(await loadSnapshot(fakeFetch(200, { nope: true }))).toEqual({ kind: 'invalid' });
  });

  it('returns invalid when a nested part fails structural validation', async () => {
    const broken = { ...ok, parts: [{ id: 'x' }] };
    expect(await loadSnapshot(fakeFetch(200, broken))).toEqual({ kind: 'invalid' });
  });
});
