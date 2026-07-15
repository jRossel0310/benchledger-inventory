import { describe, expect, it } from 'vitest';
import { loadSnapshot } from './snapshot';

const ok = { formatVersion: 1, publishedAt: '2026-07-14T00:00:00Z', partCount: 42 };

function fakeFetch(status: number, body?: unknown) {
  return async () =>
    ({ ok: status === 200, status, json: async () => body }) as Response;
}

describe('loadSnapshot', () => {
  it('returns none when the snapshot is missing (404)', async () => {
    expect(await loadSnapshot(fakeFetch(404))).toEqual({ kind: 'none' });
  });

  it('returns loaded with the parsed header', async () => {
    const state = await loadSnapshot(fakeFetch(200, ok));
    expect(state).toEqual({ kind: 'loaded', header: ok });
  });

  it('returns invalid for malformed JSON shape', async () => {
    expect(await loadSnapshot(fakeFetch(200, { nope: true }))).toEqual({ kind: 'invalid' });
  });
});
