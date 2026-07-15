/** Header fields of the published snapshot. Phase 6 extends the full schema;
 * these field names are final. */
export interface SnapshotHeader {
  formatVersion: number;
  publishedAt: string; // ISO-8601 UTC
  partCount: number;
}

export function parseSnapshotHeader(json: unknown): SnapshotHeader | null {
  if (typeof json !== 'object' || json === null) return null;
  const o = json as Record<string, unknown>;
  if (
    typeof o.formatVersion === 'number' &&
    typeof o.publishedAt === 'string' &&
    typeof o.partCount === 'number'
  ) {
    return { formatVersion: o.formatVersion, publishedAt: o.publishedAt, partCount: o.partCount };
  }
  return null;
}
