/**
 * Read-only bins view: every bin the snapshot publishes (in its stable
 * label order), each sectioned with the parts it holds (name linked to the
 * part detail route, available stock alongside), plus an "Unassigned"
 * section for parts with no bin. Bin membership is derived from the parts
 * themselves; the snapshot's `bins[].partCount` is the same number by
 * construction and is shown as the section count.
 */

import type { Snapshot, SnapshotPart } from '@ei/shared';
import './Bins.css';
import { formatQuantity } from './format';

function partCountLabel(count: number): string {
  return count === 1 ? '1 part' : `${count} parts`;
}

function BinSection({ label, parts }: { label: string; parts: SnapshotPart[] }) {
  return (
    <section className="bin-section">
      <h3 className="bin-title">
        <span className="bin-label">{label}</span>
        <span className="bin-count">{partCountLabel(parts.length)}</span>
      </h3>
      {parts.length === 0 ? (
        <p className="bin-empty">Empty</p>
      ) : (
        <ul className="bin-parts">
          {parts.map((part) => (
            <li key={part.id} className="bin-part">
              <a className="bin-part-name" href={`#/part/${part.id}`}>
                {part.displayName}
              </a>
              <span className="bin-part-stock">
                {formatQuantity(part.stock.availableMilli, part.quantityUnit)} available
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function Bins({ snapshot }: { snapshot: Snapshot }) {
  const byBin = new Map<string, SnapshotPart[]>();
  const unassigned: SnapshotPart[] = [];
  for (const part of snapshot.parts) {
    if (part.bin === null) {
      unassigned.push(part);
    } else {
      const list = byBin.get(part.bin);
      if (list === undefined) {
        byBin.set(part.bin, [part]);
      } else {
        list.push(part);
      }
    }
  }

  const labels = snapshot.bins
    .map((bin) => bin.label)
    .filter((label): label is string => label !== null);
  return (
    <div className="bins">
      <h2 className="bins-heading">Bins</h2>
      {labels.length === 0 && unassigned.length === 0 && (
        <p className="bin-empty">No bins in the published snapshot.</p>
      )}
      {labels.map((label) => (
        <BinSection key={label} label={label} parts={byBin.get(label) ?? []} />
      ))}
      {unassigned.length > 0 && <BinSection label="Unassigned" parts={unassigned} />}
    </div>
  );
}
