/**
 * A virtualized table primitive for the dense, 10k-part-scale inventory
 * table (see the Phase 3 design direction doc's "primary surface" note).
 * Generic over the row type; callers supply column defs and a `getRowId`.
 * Only the rows scrolled into view (plus overscan) are ever mounted, via
 * `@tanstack/react-virtual`.
 */

import { useVirtualizer } from '@tanstack/react-virtual';
import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from 'react';

import './DataTable.css';

export interface DataTableColumn<T> {
  key: string;
  header: string;
  /** Column width in pixels. */
  width: number;
  /** Render this column with the monospaced `--font-data` face + tabular
   * numerals — for identifiers and quantities, per the design direction. */
  mono?: boolean;
  render: (row: T) => ReactNode;
}

export interface DataTableProps<T> {
  rows: T[];
  columns: DataTableColumn<T>[];
  getRowId: (row: T) => string;
  /** Fired when a row is activated: `Enter` on the keyboard-active row, or a
   * click anywhere in the row outside the row-actions slot. */
  onActivate?: (row: T) => void;
  /** Rendered in a trailing cell that is transparent until the row is
   * hovered/focused — e.g. quick-action icon buttons. */
  rowActions?: (row: T) => ReactNode;
  rowHeight?: number;
  emptyMessage?: string;
  'aria-label'?: string;
}

export function DataTable<T>({
  rows,
  columns,
  getRowId,
  onActivate,
  rowActions,
  rowHeight = 30,
  emptyMessage = 'No rows to show.',
  'aria-label': ariaLabel,
}: DataTableProps<T>) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [activeIndex, setActiveIndex] = useState(0);

  // `rows` can shrink out from under the current selection (a filter/search
  // narrowing the result set) — reclamp so `activeIndex` never points past
  // the end (or, once rows are gone, always resolves to a valid row) and
  // keyboard/Enter activation keeps acting on a real row.
  useEffect(() => {
    setActiveIndex((current) => Math.min(current, Math.max(rows.length - 1, 0)));
  }, [rows.length]);

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 8,
  });

  function activate(index: number) {
    setActiveIndex(index);
    const row = rows[index];
    if (row) onActivate?.(row);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (rows.length === 0) return;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      const next = Math.min(activeIndex + 1, rows.length - 1);
      setActiveIndex(next);
      rowVirtualizer.scrollToIndex(next);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      const next = Math.max(activeIndex - 1, 0);
      setActiveIndex(next);
      rowVirtualizer.scrollToIndex(next);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      activate(activeIndex);
    }
  }

  return (
    <div className="data-table">
      {/* The `grid` role wraps both the header and body so assistive tech
       * sees one grid with column headers, not a header row that's an
       * unrelated sibling of an empty-looking grid. The scroll container
       * underneath keeps its own `overflow-y` for virtualization — only the
       * ARIA role and keyboard handling moved up here. */}
      <div
        className="data-table-grid"
        role="grid"
        tabIndex={0}
        aria-label={ariaLabel}
        onKeyDown={handleKeyDown}
      >
        <div className="data-table-header" role="row">
          {columns.map((col) => (
            <div
              key={col.key}
              role="columnheader"
              className="data-table-header-cell"
              style={{ width: col.width }}
            >
              {col.header}
            </div>
          ))}
          {rowActions ? (
            <div role="columnheader" className="data-table-header-cell data-table-actions-cell" />
          ) : null}
        </div>
        <div ref={scrollRef} className="data-table-scroll">
          {rows.length === 0 ? (
            <div className="data-table-empty">{emptyMessage}</div>
          ) : (
            <div
              className="data-table-virtual-spacer"
              style={{ height: rowVirtualizer.getTotalSize() }}
            >
              {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                const row = rows[virtualRow.index];
                // `virtualRow.index` is always within `rows` by construction
                // (the virtualizer is configured with `count: rows.length`);
                // this guard only satisfies `noUncheckedIndexedAccess`.
                if (!row) return null;
                const isActive = virtualRow.index === activeIndex;
                const rowStyle: CSSProperties = {
                  height: virtualRow.size,
                  transform: `translateY(${virtualRow.start}px)`,
                };
                return (
                  <div
                    key={getRowId(row)}
                    data-index={virtualRow.index}
                    role="row"
                    aria-selected={isActive}
                    className={`data-table-row${isActive ? ' data-table-row-active' : ''}`}
                    style={rowStyle}
                    onClick={() => activate(virtualRow.index)}
                  >
                    {columns.map((col) => (
                      <div
                        key={col.key}
                        role="cell"
                        className={`data-table-cell${col.mono ? ' data-table-cell-mono' : ''}`}
                        style={{ width: col.width }}
                      >
                        {col.render(row)}
                      </div>
                    ))}
                    {rowActions ? (
                      <div
                        className="data-table-cell data-table-actions-cell"
                        onClick={(event) => event.stopPropagation()}
                      >
                        {rowActions(row)}
                      </div>
                    ) : null}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
