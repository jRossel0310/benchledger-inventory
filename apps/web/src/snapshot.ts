import { parseSnapshotHeader, type SnapshotHeader } from '@ei/shared';

export const SNAPSHOT_URL = '/inventory.snapshot.json';

export type SnapshotState =
  | { kind: 'none' }
  | { kind: 'loaded'; header: SnapshotHeader }
  | { kind: 'invalid' };

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
  const header = parseSnapshotHeader(body);
  return header ? { kind: 'loaded', header } : { kind: 'invalid' };
}
