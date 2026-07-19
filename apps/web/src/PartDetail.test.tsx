import { parseSnapshot, type Snapshot } from '@ei/shared';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import sample from '../../../packages/shared/fixtures/sample-snapshot.json';
import { PartDetail } from './PartDetail';

const parsed = parseSnapshot(sample);
if (parsed === null) throw new Error('sample-snapshot.json failed to parse');

// The committed sample has no datasheet/product URLs (all null); inject one
// on the 10k resistor's variant so the external-link rendering is covered.
const DATASHEET = 'https://example.com/rc0603.pdf';
const snapshot: Snapshot = {
  ...parsed,
  parts: parsed.parts.map((part) =>
    part.id === 'ID000000000000000000000005'
      ? {
          ...part,
          variants: part.variants.map((v) => ({ ...v, datasheetUrl: DATASHEET })),
        }
      : part,
  ),
};

// "10k 0603 1% resistor": attributes, a variant with a listing, and a
// Blinky Board project association.
const RESISTOR_ID = 'ID000000000000000000000005';
// "ABS project enclosure": the sample part with dimensions.
const ENCLOSURE_ID = 'ID000000000000000000000032';

afterEach(cleanup);

describe('PartDetail', () => {
  it('renders the header: name, category, bin, and the three stock figures', () => {
    render(<PartDetail snapshot={snapshot} partId={RESISTOR_ID} />);
    expect(screen.getByRole('heading', { name: '10k 0603 1% resistor' })).toBeTruthy();
    expect(screen.getByText('Resistor')).toBeTruthy();
    expect(screen.getByText('Bin: A10')).toBeTruthy();
    expect(screen.getByText('Available')).toBeTruthy();
    expect(screen.getByText('450')).toBeTruthy(); // 450000 milli available
    expect(screen.getByText('Reserved')).toBeTruthy();
    expect(screen.getByText('50')).toBeTruthy(); // 50000 milli reserved
    expect(screen.getByText('Checked out')).toBeTruthy();
  });

  it('renders specifications with the display value and the subtle normalized form', () => {
    render(<PartDetail snapshot={snapshot} partId={RESISTOR_ID} />);
    expect(screen.getByText('Resistance')).toBeTruthy();
    expect(screen.getByText('10k')).toBeTruthy();
    expect(screen.getByText('10000 Ω')).toBeTruthy();
    expect(screen.getByText('Power rating')).toBeTruthy();
    expect(screen.getByText('1/4W')).toBeTruthy();
  });

  it('renders dimensions with value, unit, and group', () => {
    render(<PartDetail snapshot={snapshot} partId={ENCLOSURE_ID} />);
    expect(screen.getByText('Dimensions')).toBeTruthy();
    expect(screen.getByText('Length')).toBeTruthy();
    expect(screen.getByText('100 mm')).toBeTruthy();
    expect(screen.getAllByText('overall').length).toBeGreaterThan(0);
  });

  it('renders variants: manufacturer, MPN, lifecycle, and the datasheet link', () => {
    render(<PartDetail snapshot={snapshot} partId={RESISTOR_ID} />);
    expect(screen.getByText('Yageo')).toBeTruthy();
    expect(screen.getByText('RC0603FR-0710KL')).toBeTruthy();
    expect(screen.getByText('active')).toBeTruthy();
    const datasheet = screen.getByRole('link', { name: 'Datasheet' });
    expect(datasheet.getAttribute('href')).toBe(DATASHEET);
    expect(datasheet.getAttribute('rel')).toContain('noopener');
    expect(datasheet.getAttribute('target')).toBe('_blank');
  });

  it('renders supplier part numbers without any price', () => {
    const { container } = render(<PartDetail snapshot={snapshot} partId={RESISTOR_ID} />);
    expect(screen.getByText('DigiKey')).toBeTruthy();
    expect(screen.getByText('311-10.0KLRCT-ND')).toBeTruthy();
    expect(screen.getByText('Cut Tape')).toBeTruthy();
    expect(container.textContent).not.toMatch(/price|\$/i);
  });

  it('cross-links project associations to the projects view', () => {
    render(<PartDetail snapshot={snapshot} partId={RESISTOR_ID} />);
    const link = screen.getByRole('link', { name: 'Blinky Board' });
    expect(link.getAttribute('href')).toBe('#/projects');
  });

  it('omits sections the part has no data for', () => {
    // The M3 hex nut has no attributes, dimensions, variants, or projects.
    render(<PartDetail snapshot={snapshot} partId="ID000000000000000000000041" />);
    expect(screen.queryByText('Specifications')).toBeNull();
    expect(screen.queryByText('Dimensions')).toBeNull();
    expect(screen.queryByText('Variants')).toBeNull();
    expect(screen.queryByText('Supplier part numbers')).toBeNull();
    expect(screen.queryByText('Used in projects')).toBeNull();
  });

  it('shows a not-found panel with a back link for an unknown part id', () => {
    render(<PartDetail snapshot={snapshot} partId="ID999999999999999999999999" />);
    expect(screen.getByText('Part not found')).toBeTruthy();
    const back = screen.getByRole('link', { name: /Back to inventory/ });
    expect(back.getAttribute('href')).toBe('#/');
  });

  it('has a back link to the inventory on the found path too', () => {
    render(<PartDetail snapshot={snapshot} partId={RESISTOR_ID} />);
    const back = screen.getByRole('link', { name: /All parts/ });
    expect(back.getAttribute('href')).toBe('#/');
  });
});
