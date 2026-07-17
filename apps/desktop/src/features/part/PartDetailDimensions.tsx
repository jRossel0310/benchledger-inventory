/**
 * Part detail's Dimensions tab (Phase 3 Task 7): every physical measurement
 * on file for the part (`list_dimensions`) — name, value + its own display
 * unit (dimensions don't share the part's `quantity_unit`; each one carries
 * its own, e.g. body length in `mm` on an `each`-unit part), group,
 * provenance source, and any notes.
 */

import type { DimensionGroup, DimensionSource, PartId } from '../../bindings.gen';
import { useDimensions } from '../../hooks/inventory';
import { errorMessage, formatTimestamp } from '../../lib/format';
import './PartDetail.css';

const GROUP_LABELS: Record<DimensionGroup, string> = {
  overall: 'Overall',
  body: 'Body',
  mounting: 'Mounting',
  custom: 'Custom',
};

const SOURCE_LABELS: Record<DimensionSource, string> = {
  manufacturer: 'Manufacturer',
  datasheet: 'Datasheet',
  supplier: 'Supplier',
  measured: 'Measured',
  estimated: 'Estimated',
};

export interface PartDetailDimensionsProps {
  partId: PartId;
}

export function PartDetailDimensions({ partId }: PartDetailDimensionsProps) {
  const dimensionsQuery = useDimensions(partId);

  if (dimensionsQuery.isPending) {
    return <p className="part-detail-status">Loading dimensions…</p>;
  }
  if (dimensionsQuery.isError) {
    return (
      <p className="part-detail-status part-detail-status-error">
        Could not load dimensions: {errorMessage(dimensionsQuery.error)}
      </p>
    );
  }

  const rows = dimensionsQuery.data ?? [];
  if (rows.length === 0) {
    return <p className="part-detail-status">No dimensions recorded for this part yet.</p>;
  }

  return (
    <table className="part-detail-table">
      <thead>
        <tr>
          <th>Name</th>
          <th>Value</th>
          <th>Group</th>
          <th>Source</th>
          <th>Measured</th>
          <th>Notes</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((dim) => (
          <tr key={dim.id}>
            <td>{dim.name}</td>
            <td className="part-detail-mono">
              {dim.value_num !== null ? `${dim.value_num} ${dim.display_unit}` : '—'}
            </td>
            <td>{GROUP_LABELS[dim.group]}</td>
            <td>{SOURCE_LABELS[dim.source]}</td>
            <td className="part-detail-mono">
              {dim.measured_date ? formatTimestamp(dim.measured_date) : '—'}
            </td>
            <td>{dim.notes || '—'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
