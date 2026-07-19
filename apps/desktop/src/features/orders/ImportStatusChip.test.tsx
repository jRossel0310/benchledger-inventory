import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { ImportStatusChip } from './ImportStatusChip';

afterEach(cleanup);

describe('ImportStatusChip', () => {
  it.each([
    ['parsed', 'Parsed'],
    ['committed', 'Committed'],
    ['reversed', 'Reversed'],
  ])('renders the known status %s with its label and modifier class', (status, label) => {
    render(<ImportStatusChip status={status} />);
    const chip = screen.getByText(label);
    expect(chip.className).toContain(`import-status-chip-${status}`);
  });

  it('falls back to a title-cased raw status and the "unknown" modifier class for an unrecognized status', () => {
    render(<ImportStatusChip status="pending_review" />);
    const chip = screen.getByText('Pending Review');
    expect(chip.className).toContain('import-status-chip-unknown');
    expect(chip.className).not.toContain('import-status-chip-pending_review');
  });
});
