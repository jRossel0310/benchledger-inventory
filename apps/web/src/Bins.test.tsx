import { parseSnapshot, type Snapshot } from '@ei/shared';
import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import sample from '../../../packages/shared/fixtures/sample-snapshot.json';
import { Bins } from './Bins';

const parsed = parseSnapshot(sample);
if (parsed === null) throw new Error('sample-snapshot.json failed to parse');
const snapshot: Snapshot = parsed;

// A variant of the sample where the 10k resistor has no bin, exercising the
// Unassigned section (the committed sample assigns every part a bin).
const withUnassigned: Snapshot = {
  ...snapshot,
  parts: snapshot.parts.map((part) =>
    part.id === 'ID000000000000000000000005' ? { ...part, bin: null } : part,
  ),
};

afterEach(cleanup);

describe('Bins', () => {
  it('renders a section per bin with its part count', () => {
    render(<Bins snapshot={snapshot} />);
    const a10 = screen.getByText('A10').closest('.bin-section');
    expect(a10).not.toBeNull();
    expect(within(a10 as HTMLElement).getByText('1 part')).toBeTruthy();
    expect(screen.getAllByText('1 part').length).toBe(snapshot.bins.length);
  });

  it('groups parts under their bin with detail links and stock', () => {
    render(<Bins snapshot={snapshot} />);
    const a10 = screen.getByText('A10').closest('.bin-section') as HTMLElement;
    const link = within(a10).getByRole('link', { name: '10k 0603 1% resistor' });
    expect(link.getAttribute('href')).toBe('#/part/ID000000000000000000000005');
    expect(within(a10).getByText('450 available')).toBeTruthy();
  });

  it('shows no Unassigned section when every part has a bin', () => {
    render(<Bins snapshot={snapshot} />);
    expect(screen.queryByText('Unassigned')).toBeNull();
  });

  it('collects parts without a bin under an Unassigned section', () => {
    render(<Bins snapshot={withUnassigned} />);
    const section = screen.getByText('Unassigned').closest('.bin-section') as HTMLElement;
    expect(within(section).getByRole('link', { name: '10k 0603 1% resistor' })).toBeTruthy();
    // The part left its old bin's section too.
    const a10 = screen.getByText('A10').closest('.bin-section') as HTMLElement;
    expect(within(a10).queryByRole('link', { name: '10k 0603 1% resistor' })).toBeNull();
  });
});
