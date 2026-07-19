/**
 * The read-only inventory table -- the web rendering of the desktop's
 * inventory browser, sharing its visual language (dense rows, `--font-data`
 * for identifiers and quantities, the segmented available/reserved/
 * checked-out stock bar in the shared stock-state token colors, the amber
 * low-stock chip) without sharing any code: the web app has no component
 * library, so the bar is a plain segmented div.
 *
 * Rows link to the `#/part/<id>` hash route (Task 8 adds the detail view
 * behind it). Plain rendering, no virtualization -- snapshot scale is
 * hundreds of parts; revisit above ~2k rows.
 */

import type { SnapshotPart } from '@ei/shared';
import './Inventory.css';
import { formatQuantity } from './format';

/** A short "key specs" line under the part name: the first few attribute
 * display values (skipping bare boolean flags, whose original text is just
 * "true"/"false"), falling back to the description for spec-less parts. */
export function specLine(part: SnapshotPart): string {
  const specs = part.attributes
    .map((attr) => attr.originalText)
    .filter((text) => text !== 'true' && text !== 'false')
    .slice(0, 3);
  return specs.length > 0 ? specs.join(' · ') : part.description;
}

function StockBar({ part }: { part: SnapshotPart }) {
  const { availableMilli, reservedMilli, checkedOutMilli } = part.stock;
  const total = availableMilli + reservedMilli + checkedOutMilli;
  const pct = (value: number) => (total > 0 ? (value / total) * 100 : 0);
  return (
    <div className="stock-bar" aria-hidden="true">
      <span className="stock-bar-available" style={{ width: `${pct(availableMilli)}%` }} />
      <span className="stock-bar-reserved" style={{ width: `${pct(reservedMilli)}%` }} />
      <span className="stock-bar-checked-out" style={{ width: `${pct(checkedOutMilli)}%` }} />
    </div>
  );
}

export function Inventory({ parts }: { parts: SnapshotPart[] }) {
  if (parts.length === 0) {
    return <p className="inventory-none">No parts match this search.</p>;
  }
  return (
    <table className="inventory-table">
      <thead>
        <tr>
          <th className="inventory-col-part">Part</th>
          <th>Category</th>
          <th className="inventory-col-num">Available</th>
          <th className="inventory-col-num">Reserved</th>
          <th className="inventory-col-num">Checked out</th>
          <th className="inventory-col-stock">Stock</th>
          <th>Bin</th>
        </tr>
      </thead>
      <tbody>
        {parts.map((part) => (
          <tr key={part.id}>
            <td className="inventory-col-part">
              <div className="inventory-name-line">
                <a className="inventory-name" href={`#/part/${part.id}`}>
                  {part.displayName}
                </a>
                {part.lowStock && <span className="inventory-low">Low</span>}
              </div>
              <div className="inventory-spec">{specLine(part)}</div>
            </td>
            <td className="inventory-category">{part.category}</td>
            <td className="inventory-col-num">
              {formatQuantity(part.stock.availableMilli, part.quantityUnit)}
            </td>
            <td className="inventory-col-num">
              {formatQuantity(part.stock.reservedMilli, part.quantityUnit)}
            </td>
            <td className="inventory-col-num">
              {formatQuantity(part.stock.checkedOutMilli, part.quantityUnit)}
            </td>
            <td className="inventory-col-stock">
              <StockBar part={part} />
            </td>
            <td className="inventory-bin">{part.bin ?? '—'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
