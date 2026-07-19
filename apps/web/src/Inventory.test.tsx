import { parseSnapshot } from '@ei/shared';
import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import sample from '../../../packages/shared/fixtures/sample-snapshot.json';
import { Inventory } from './Inventory';
import { formatQuantity } from './format';

const snapshot = parseSnapshot(sample);
if (snapshot === null) throw new Error('sample-snapshot.json failed to parse');

afterEach(cleanup);

function rowFor(name: string): HTMLElement {
  const cell = screen.getByText(name);
  const row = cell.closest('tr');
  if (row === null) throw new Error(`no table row contains "${name}"`);
  return row;
}

describe('formatQuantity', () => {
  it('renders each-quantities as bare whole numbers', () => {
    expect(formatQuantity(450000, 'each')).toBe('450');
    expect(formatQuantity(0, 'each')).toBe('0');
  });

  it('renders continuous units with a suffix and exact decimals', () => {
    expect(formatQuantity(25000, 'm')).toBe('25 m');
    expect(formatQuantity(1500, 'm')).toBe('1.5 m');
    expect(formatQuantity(2500, 'ft')).toBe('2.5 ft');
  });
});

describe('Inventory', () => {
  it('renders one row per part', () => {
    render(<Inventory parts={snapshot.parts} />);
    expect(screen.getAllByRole('row')).toHaveLength(snapshot.parts.length + 1); // + header
  });

  it('shows whole-number quantities derived from milli values', () => {
    render(<Inventory parts={snapshot.parts} />);
    const row = rowFor('10k 0603 1% resistor');
    const cells = within(row).getAllByRole('cell');
    const texts = cells.map((c) => c.textContent);
    expect(texts).toContain('450'); // available
    expect(texts).toContain('50'); // reserved
    expect(texts).toContain('A10'); // bin
  });

  it('respects the quantity unit for continuous quantities', () => {
    render(<Inventory parts={snapshot.parts} />);
    const row = rowFor('22AWG stranded hookup wire, red');
    expect(within(row).getByText('25 m')).toBeTruthy();
  });

  it('shows the low-stock badge only on flagged parts', () => {
    render(<Inventory parts={snapshot.parts} />);
    expect(within(rowFor('32.768kHz watch crystal')).getByText('Low')).toBeTruthy();
    expect(within(rowFor('10k 0603 1% resistor')).queryByText('Low')).toBeNull();
  });

  it('renders the three stock-bar segments per row', () => {
    const { container } = render(<Inventory parts={snapshot.parts} />);
    const firstBar = container.querySelector('.stock-bar');
    expect(firstBar).not.toBeNull();
    expect(firstBar?.querySelectorAll('.stock-bar-available')).toHaveLength(1);
    expect(firstBar?.querySelectorAll('.stock-bar-reserved')).toHaveLength(1);
    expect(firstBar?.querySelectorAll('.stock-bar-checked-out')).toHaveLength(1);
    expect(container.querySelectorAll('.stock-bar')).toHaveLength(snapshot.parts.length);
  });

  it('shows a key spec line from the part attributes', () => {
    render(<Inventory parts={snapshot.parts} />);
    const spec = rowFor('2N7000 N-channel small-signal MOSFET').querySelector('.inventory-spec');
    expect(spec?.textContent).toContain('N-channel');
    expect(spec?.textContent).toContain('200mA');
  });

  it('links each row to the part detail hash route', () => {
    render(<Inventory parts={snapshot.parts} />);
    const link = within(rowFor('10k 0603 1% resistor')).getByRole('link');
    expect(link.getAttribute('href')).toBe('#/part/ID000000000000000000000005');
  });

  it('shows an empty message when no parts match', () => {
    render(<Inventory parts={[]} />);
    expect(screen.getByText('No parts match this search.')).toBeTruthy();
  });
});
