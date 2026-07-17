/**
 * The primary Inventory surface (Phase 3 Task 4, see
 * docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md §9): a
 * dense, virtualized table over `useInventorySearch(query)` — the same
 * `search` command and query grammar the Ctrl+K palette and dashboard card
 * links already use, so filtering, saved views, and free text all reuse the
 * tested backend query engine rather than a client-side re-filter. `query`
 * is owned by the caller (the route's `?q=` search param) so the table
 * itself stays a pure "query in, rows out" view.
 *
 * `SearchHit` (Phase 2c) doesn't carry the part's `quantity_unit`, so every
 * quantity here is formatted with `formatQuantity(value, 'each')` — correct
 * for the common case (`each`-unit parts, the vast majority of a typical
 * library) and never wrong (the `each` suffix is empty), but a continuous-
 * unit part (e.g. wire, sold by the meter) renders its milli-accurate
 * number without the `m`/`ft` suffix until the part detail view (Task 7)
 * fills in the rest of the record.
 *
 * A row click/Enter opens the part-detail inspector drawer (Task 7,
 * `usePartInspector`) over this table rather than navigating away — the
 * design direction's "inspect a part without losing your place in the
 * list." The full-page `/inventory/$partId` route still exists (deep link/
 * back-button friendly), reached from inside the drawer via its own "Open
 * full page" link, never from this row click.
 */

import type { SearchHit } from '../../bindings.gen';
import { DataTable, type DataTableColumn } from '../../components/DataTable';
import { isStockLow, StockGauge } from '../../components/StockGauge';
import { useInventorySearch } from '../../hooks/inventory';
import { errorMessage, formatQuantity } from '../../lib/format';
import { usePartInspector } from '../part/PartInspectorContext';
import { RowActions } from './RowActions';
import './InventoryTable.css';

const COLUMNS: DataTableColumn<SearchHit>[] = [
  {
    key: 'part',
    header: 'Part',
    width: 300,
    mono: true,
    render: (row) => row.display_name,
  },
  {
    key: 'category',
    header: 'Category',
    width: 130,
    render: (row) => row.category_name,
  },
  {
    key: 'stock',
    header: 'Stock',
    width: 140,
    render: (row) => (
      <StockGauge
        available={row.available}
        reserved={row.reserved}
        checkedOut={row.checked_out}
        unit="each"
        lowThreshold={row.low_stock_threshold ?? null}
      />
    ),
  },
  {
    key: 'available',
    header: 'Avail',
    width: 64,
    mono: true,
    render: (row) => formatQuantity(row.available, 'each'),
  },
  {
    key: 'reserved',
    header: 'Resv',
    width: 64,
    mono: true,
    render: (row) => formatQuantity(row.reserved, 'each'),
  },
  {
    key: 'checked_out',
    header: 'Out',
    width: 64,
    mono: true,
    render: (row) => formatQuantity(row.checked_out, 'each'),
  },
  {
    key: 'bin',
    header: 'Bin',
    width: 80,
    mono: true,
    render: (row) => row.bin_label ?? '—',
  },
  {
    key: 'status',
    header: 'Status',
    width: 60,
    render: (row) =>
      isStockLow(row.available, row.low_stock_threshold ?? null) ? (
        <span className="inventory-low-chip">Low</span>
      ) : null,
  },
];

export interface InventoryTableProps {
  /** The active search query — free text plus any `key:value`/flag
   * fragments the Filters/SavedViews/search box have composed into it. An
   * empty string means "no filter" (every non-archived part). */
  query: string;
}

export function InventoryTable({ query }: InventoryTableProps) {
  const partInspector = usePartInspector();
  const searchQuery = useInventorySearch(query);

  function handleActivate(row: SearchHit) {
    partInspector.open(row.part_id);
  }

  // `isLoading` (no data yet at all) rather than `isPending` (no *current*
  // data): with `useInventorySearch`'s `placeholderData: keepPreviousData`,
  // `isPending` alone would still be false while the *previous* query's rows
  // sit in `data` — but gating on it here would be relying on that
  // incidentally rather than by design. `isLoading` is the query-key-change
  // case actually meant to show the empty state; a query string changing
  // out from under an already-loaded table (typing, filtering) instead falls
  // through to the table below with its prior rows still in `data`, while
  // `isFetching` (checked below) flags that a fresher result is on the way.
  if (searchQuery.isLoading) {
    return <p className="inventory-table-status">Loading inventory…</p>;
  }

  if (searchQuery.isError) {
    return (
      <p className="inventory-table-status inventory-table-status-error">
        Could not load inventory: {errorMessage(searchQuery.error)}
      </p>
    );
  }

  const rows = searchQuery.data ?? [];
  // An unfiltered query (`''`) returns every non-archived part, so zero rows
  // there means the library itself is empty; a non-empty query returning
  // zero rows means the filter matched nothing.
  const emptyMessage =
    query.trim().length > 0
      ? 'No parts match — try a different search or clear a filter.'
      : 'No parts yet — press Ctrl+K to create one or import an order.';

  return (
    <div className="inventory-table-wrap">
      {/* A subtle "still current, but a fresher result is in flight" cue —
       * shown instead of unmounting the table — while a new `query` refetches
       * over the placeholder (previous-query) rows still on screen. */}
      {searchQuery.isFetching ? (
        <p className="inventory-table-pending" aria-live="polite">
          Updating…
        </p>
      ) : null}
      <DataTable
        rows={rows}
        columns={COLUMNS}
        getRowId={(row) => row.part_id}
        onActivate={handleActivate}
        rowActions={(row) => <RowActions row={row} />}
        emptyMessage={emptyMessage}
        aria-label="Inventory"
      />
    </div>
  );
}
