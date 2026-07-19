import { parseSnapshot, type Snapshot } from '@ei/shared';

export const SNAPSHOT_URL = '/inventory.snapshot.json';

/** Phase 6 Task 2: `loaded` now carries the FULL parsed `Snapshot` (every
 * part/bin/project, full schema -- see `@ei/shared`'s `snapshot.ts`), not
 * just the header summary. Task 7 builds the inventory table/search UI on
 * top of `snapshot` directly. */
export type SnapshotState =
  { kind: 'none' } | { kind: 'loaded'; snapshot: Snapshot } | { kind: 'invalid' };

export async function loadSnapshot(
  fetchImpl: (url: string) => Promise<Response> = (url) => fetch(url),
): Promise<SnapshotState> {
  let res: Response;
  try {
    res = await fetchImpl(SNAPSHOT_URL);
  } catch {
    return { kind: 'none' };
  }
  if (!res.ok) return { kind: 'none' };
  let body: unknown;
  try {
    body = await res.json();
  } catch {
    return { kind: 'invalid' };
  }
  const snapshot = parseSnapshot(body);
  return snapshot ? { kind: 'loaded', snapshot } : { kind: 'invalid' };
}
