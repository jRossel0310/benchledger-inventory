/**
 * The Orders & Imports list (Phase 5d Task 2, spec §9/§10): every persisted
 * import (`useImports`), newest first (the hook's own order — `list_
 * imports` returns newest-first server-side), each row showing its
 * source-format badge, line count, formatted total, and lifecycle status
 * chip. A row click/Enter navigates to `/orders/$importId` (`ImportReview`,
 * landing on Task 3's placeholder shell until that task replaces it) — the
 * same `DataTable` + row-activate pattern `ProjectsList.tsx` uses.
 *
 * The upload zone (`UploadImport`) is always rendered above the table,
 * never behind a click-to-reveal toggle: an imports screen with zero rows
 * is exactly the moment a first-time user needs the upload control most, so
 * hiding it behind another click would be the wrong default (mirrors why
 * `ProjectsList`'s create form is the plan's *inline*, not modal-first,
 * exception).
 */

import { useNavigate } from '@tanstack/react-router';

import type { ImportRecord } from '../../bindings.gen';
import { DataTable, type DataTableColumn } from '../../components/DataTable';
import { useImports } from '../../hooks/imports';
import { errorMessage, formatPrice, formatTimestamp } from '../../lib/format';
import { ImportStatusChip } from './ImportStatusChip';
import { UploadImport } from './UploadImport';
import './OrdersList.css';

const SOURCE_FORMAT_LABELS: Record<string, string> = {
  pdf: 'PDF',
  csv: 'CSV',
  xlsx: 'XLSX',
};

/** The order-identifying number to show in the list: `order_number`, falling
 * back through `invoice_number` -> `shipment_number` -> `web_order_id` (the
 * DigiKey parsers don't always populate every field, but at least one
 * usually survives) -> an em dash when none do. Exported so `ImportReview`
 * (Phase 5d Task 3) uses the exact same fallback for its header and
 * duplicate-import links rather than a second copy. */
export function orderNumberFor(row: ImportRecord): string {
  return row.order_number ?? row.invoice_number ?? row.shipment_number ?? row.web_order_id ?? '—';
}

const COLUMNS: DataTableColumn<ImportRecord>[] = [
  {
    key: 'created_at',
    header: 'Imported',
    width: 150,
    mono: true,
    render: (row) => formatTimestamp(row.created_at),
  },
  {
    key: 'supplier',
    header: 'Supplier',
    width: 140,
    render: (row) => row.supplier,
  },
  {
    key: 'order_number',
    header: 'Order #',
    width: 160,
    mono: true,
    render: (row) => orderNumberFor(row),
  },
  {
    key: 'source_format',
    header: 'Format',
    width: 80,
    render: (row) => (
      <span className="orders-list-format-badge">
        {SOURCE_FORMAT_LABELS[row.source_format] ?? row.source_format.toUpperCase()}
      </span>
    ),
  },
  {
    key: 'line_count',
    header: 'Lines',
    width: 70,
    mono: true,
    render: (row) => String(row.line_count),
  },
  {
    key: 'total',
    header: 'Total',
    width: 110,
    mono: true,
    render: (row) => formatPrice(row.total_micros, row.currency),
  },
  {
    key: 'status',
    header: 'Status',
    width: 110,
    render: (row) => <ImportStatusChip status={row.status} />,
  },
];

export function OrdersList() {
  const navigate = useNavigate();
  const importsQuery = useImports();

  function handleActivate(row: ImportRecord) {
    void navigate({ to: '/orders/$importId', params: { importId: row.id } });
  }

  return (
    <section className="orders-list">
      <header className="orders-list-header">
        <div>
          <p className="orders-list-eyebrow">Orders</p>
          <h1 className="orders-list-title">Orders &amp; imports</h1>
        </div>
      </header>

      <UploadImport />

      <div className="orders-list-table">
        {importsQuery.isPending ? (
          <p className="orders-list-status">Loading orders…</p>
        ) : importsQuery.isError ? (
          <p className="orders-list-status orders-list-status-error">
            Could not load orders: {errorMessage(importsQuery.error)}
          </p>
        ) : (
          <DataTable
            rows={importsQuery.data}
            columns={COLUMNS}
            getRowId={(row) => row.id}
            onActivate={handleActivate}
            emptyMessage="No orders imported yet — upload a DigiKey order confirmation or invoice above to get started."
            aria-label="Orders"
          />
        )}
      </div>
    </section>
  );
}
