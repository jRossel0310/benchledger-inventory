/**
 * Part detail's Specifications tab (Phase 3 Task 7): every attribute value
 * on file for the part — key, the exact text the user/import entered
 * (`original_text`), and the normalized numeric value the backend parsed it
 * into (`value_num`, `null` for non-numeric attribute types) — rendered in
 * `--font-data` per the design direction (identifiers/measurements read like
 * an instrument, not prose). `category_attribute_defs` supplies a friendly
 * `label` per key when the part's category still has that attribute
 * attached; a key with no matching def (e.g. the category changed since the
 * value was set) falls back to the raw key rather than hiding the row.
 */

import type { CategoryId, PartId } from '../../bindings.gen';
import { useAttributes, useCategoryAttributeDefs } from '../../hooks/inventory';
import { errorMessage } from '../../lib/format';
import './PartDetail.css';

export interface PartDetailSpecificationsProps {
  partId: PartId;
  categoryId: CategoryId;
}

export function PartDetailSpecifications({ partId, categoryId }: PartDetailSpecificationsProps) {
  const attributesQuery = useAttributes(partId);
  const defsQuery = useCategoryAttributeDefs(categoryId);

  if (attributesQuery.isPending || defsQuery.isPending) {
    return <p className="part-detail-status">Loading specifications…</p>;
  }
  if (attributesQuery.isError) {
    return (
      <p className="part-detail-status part-detail-status-error">
        Could not load specifications: {errorMessage(attributesQuery.error)}
      </p>
    );
  }

  const defByKey = new Map((defsQuery.data ?? []).map((def) => [def.key, def]));
  const rows = attributesQuery.data ?? [];

  if (rows.length === 0) {
    return <p className="part-detail-status">No specifications set on this part yet.</p>;
  }

  return (
    <table className="part-detail-table">
      <thead>
        <tr>
          <th>Attribute</th>
          <th>Original text</th>
          <th>Normalized</th>
        </tr>
      </thead>
      <tbody>
        {rows.map(([key, originalText, valueNum]) => (
          <tr key={key}>
            <td>
              <span className="part-detail-mono">{defByKey.get(key)?.label ?? key}</span>
            </td>
            <td className="part-detail-mono">{originalText}</td>
            <td className="part-detail-mono">{valueNum ?? '—'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
