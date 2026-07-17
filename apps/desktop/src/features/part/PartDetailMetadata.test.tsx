import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import type { PartRecord } from '../../bindings.gen';
import { PartDetailMetadata } from './PartDetailMetadata';

function part(overrides: Partial<PartRecord> = {}): PartRecord {
  return {
    id: 'p1',
    display_name: '10k 0603 1% resistor',
    category_id: 'cat-resistor',
    description: '',
    bin_label: 'A12',
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each',
    low_stock_threshold: null,
    public_notes: '',
    private_notes: '',
    metadata_complete: true,
    archived: false,
    created_at: '2026-01-01 09:00:00',
    modified_at: '2026-02-10 15:30:00',
    ...overrides,
  };
}

/** The metadata-complete/archived value sits in the `<dd>` immediately
 * following its `<dt>` label — scoping through the label avoids colliding
 * with the *other* Yes/No row (`Archived`) rendered on the same screen. */
function valueFor(label: string): string | null {
  return screen.getByText(label).nextElementSibling?.textContent ?? null;
}

afterEach(cleanup);

describe('PartDetailMetadata', () => {
  it('shows metadata_complete as Yes, and created/modified timestamps', () => {
    render(<PartDetailMetadata part={part({ metadata_complete: true })} />);

    expect(valueFor('Metadata complete')).toBe('Yes');
    // Both timestamps render distinctly formatted, not just the raw string.
    expect(screen.queryByText('2026-01-01 09:00:00')).toBeNull();
  });

  it('shows metadata_complete as No when the part has gaps', () => {
    render(<PartDetailMetadata part={part({ metadata_complete: false })} />);

    expect(valueFor('Metadata complete')).toBe('No');
  });

  it('never fabricates per-field provenance the backend does not provide', () => {
    render(<PartDetailMetadata part={part()} />);

    expect(screen.queryByText(/verified/i)).toBeNull();
    expect(screen.queryByText(/confidence/i)).toBeNull();
  });
});
