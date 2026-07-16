import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { DataTable, type DataTableColumn } from './DataTable';

interface Row {
  id: string;
  name: string;
}

const columns: DataTableColumn<Row>[] = [
  { key: 'name', header: 'Name', width: 200, render: (row) => row.name },
];

function makeRows(count: number): Row[] {
  return Array.from({ length: count }, (_, i) => ({ id: `r${i}`, name: `Row ${i}` }));
}

// @tanstack/react-virtual measures the scroll container via
// element.offsetHeight/offsetWidth (falling back to ResizeObserver, which
// jsdom doesn't implement). jsdom's layout engine always reports 0 for
// these, so we pin them to a realistic viewport size for the duration of
// this suite — otherwise every row would appear "out of view" and none
// would virtualize in.
let offsetHeightSpy: ReturnType<typeof vi.spyOn>;
let offsetWidthSpy: ReturnType<typeof vi.spyOn>;

beforeAll(() => {
  offsetHeightSpy = vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(300);
  offsetWidthSpy = vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(800);
});

afterAll(() => {
  offsetHeightSpy.mockRestore();
  offsetWidthSpy.mockRestore();
});

afterEach(cleanup);

describe('DataTable', () => {
  it('virtualizes: a 1000-row table only mounts a small window of rows', () => {
    const rows = makeRows(1000);
    render(<DataTable rows={rows} columns={columns} getRowId={(row) => row.id} />);

    const mountedRows = screen.getAllByRole('row').filter((el) => el.hasAttribute('data-index'));
    expect(mountedRows.length).toBeGreaterThan(0);
    expect(mountedRows.length).toBeLessThan(100);
  });

  it('renders the given rows and columns', () => {
    render(<DataTable rows={makeRows(3)} columns={columns} getRowId={(row) => row.id} />);
    expect(screen.getByText('Row 0')).toBeTruthy();
    expect(screen.getByText('Row 1')).toBeTruthy();
    expect(screen.getByText('Row 2')).toBeTruthy();
  });

  it('moves the active row with ArrowDown/ArrowUp and fires onActivate on Enter', () => {
    const rows = makeRows(5);
    const onActivate = vi.fn();
    render(
      <DataTable
        rows={rows}
        columns={columns}
        getRowId={(row) => row.id}
        onActivate={onActivate}
      />,
    );

    const grid = screen.getByRole('grid');
    fireEvent.keyDown(grid, { key: 'ArrowDown' });
    fireEvent.keyDown(grid, { key: 'ArrowDown' });
    fireEvent.keyDown(grid, { key: 'Enter' });

    expect(onActivate).toHaveBeenCalledTimes(1);
    expect(onActivate).toHaveBeenCalledWith(rows[2]);
  });

  it('fires onActivate when a row is clicked', () => {
    const rows = makeRows(5);
    const onActivate = vi.fn();
    render(
      <DataTable
        rows={rows}
        columns={columns}
        getRowId={(row) => row.id}
        onActivate={onActivate}
      />,
    );

    fireEvent.click(screen.getByText('Row 3'));
    expect(onActivate).toHaveBeenCalledWith(rows[3]);
  });

  it('renders an empty-state message when there are no rows', () => {
    render(
      <DataTable
        rows={[]}
        columns={columns}
        getRowId={(row: Row) => row.id}
        emptyMessage="No parts yet."
      />,
    );
    expect(screen.getByText('No parts yet.')).toBeTruthy();
  });

  it('renders mono cells with the data-table-cell-mono class', () => {
    const monoColumns: DataTableColumn<Row>[] = [
      { key: 'name', header: 'Name', width: 200, mono: true, render: (row) => row.name },
    ];
    render(<DataTable rows={makeRows(1)} columns={monoColumns} getRowId={(row) => row.id} />);
    const cell = screen.getByText('Row 0');
    expect(cell.className).toContain('data-table-cell-mono');
  });

  it('renders the row-actions slot only when rowActions is supplied', () => {
    const { rerender } = render(
      <DataTable rows={makeRows(1)} columns={columns} getRowId={(row) => row.id} />,
    );
    expect(screen.queryByText('Act')).toBeNull();

    rerender(
      <DataTable
        rows={makeRows(1)}
        columns={columns}
        getRowId={(row) => row.id}
        rowActions={() => <button type="button">Act</button>}
      />,
    );
    expect(screen.getByText('Act')).toBeTruthy();
  });
});
