import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  DimensionFields,
  emptyDimensionEntry,
  isDimensionEntryFilled,
  toDimensionDraft,
  type DimensionEntry,
} from './DimensionFields';

afterEach(cleanup);

describe('DimensionFields', () => {
  it('renders no rows and an "Add dimension" button when the list is empty', () => {
    renderRows([]);
    expect(screen.queryByLabelText('Name')).toBeNull();
    expect(screen.getByText('+ Add dimension')).toBeTruthy();
  });

  it('appends a fresh empty row when "Add dimension" is clicked', () => {
    const onChange = vi.fn();
    render(<DimensionFields dimensions={[]} onChange={onChange} />);
    fireEvent.click(screen.getByText('+ Add dimension'));
    expect(onChange).toHaveBeenCalledWith([emptyDimensionEntry()]);
  });

  it('renders a field set per row: name, value, group, source, measured date, notes', () => {
    renderRows([row({ name: 'Length', rawValue: '5mm' })]);
    expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe('Length');
    expect((screen.getByLabelText('Value') as HTMLInputElement).value).toBe('5mm');
    expect(screen.getByLabelText('Group')).toBeTruthy();
    expect(screen.getByLabelText('Source')).toBeTruthy();
    expect(screen.getByLabelText('Measured date')).toBeTruthy();
    expect(screen.getByLabelText('Notes')).toBeTruthy();
  });

  it('reports an edited field through onChange without touching the other rows', () => {
    const onChange = vi.fn();
    const rows = [row({ name: 'Length' }), row({ name: 'Width' })];
    render(<DimensionFields dimensions={rows} onChange={onChange} />);

    fireEvent.change(screen.getAllByLabelText('Value')[0]!, { target: { value: '5mm' } });

    expect(onChange).toHaveBeenCalledWith([{ ...rows[0], rawValue: '5mm' }, rows[1]]);
  });

  it('removes only the targeted row', () => {
    const onChange = vi.fn();
    const rows = [row({ name: 'Length' }), row({ name: 'Width' })];
    render(<DimensionFields dimensions={rows} onChange={onChange} />);

    fireEvent.click(screen.getByLabelText('Remove Length'));

    expect(onChange).toHaveBeenCalledWith([rows[1]]);
  });

  it('changes the group and source selects', () => {
    const onChange = vi.fn();
    render(<DimensionFields dimensions={[row()]} onChange={onChange} />);

    fireEvent.change(screen.getByLabelText('Group'), { target: { value: 'body' } });
    expect((onChange.mock.calls[0]![0] as DimensionEntry[])[0]!.group).toBe('body');

    fireEvent.change(screen.getByLabelText('Source'), { target: { value: 'datasheet' } });
    expect((onChange.mock.calls[1]![0] as DimensionEntry[])[0]!.source).toBe('datasheet');
  });

  it('disables every field and button when disabled', () => {
    render(<DimensionFields dimensions={[row()]} onChange={vi.fn()} disabled />);
    expect((screen.getByLabelText('Name') as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByText('+ Add dimension') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByLabelText(/^Remove/) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe('toDimensionDraft', () => {
  it('trims text fields and converts a blank measured date to null', () => {
    expect(
      toDimensionDraft({
        group: 'body',
        name: '  Length  ',
        rawValue: ' 5mm ',
        source: 'datasheet',
        notes: '  ',
        measuredDate: '',
      }),
    ).toEqual({
      group: 'body',
      name: 'Length',
      raw_value: '5mm',
      source: 'datasheet',
      notes: '',
      measured_date: null,
    });
  });

  it('keeps a filled-in measured date', () => {
    expect(toDimensionDraft(row({ measuredDate: '2026-01-01' })).measured_date).toBe('2026-01-01');
  });
});

describe('isDimensionEntryFilled', () => {
  it('is false for a fresh empty row and true once name and value are both entered', () => {
    expect(isDimensionEntryFilled(emptyDimensionEntry())).toBe(false);
    expect(isDimensionEntryFilled(row({ name: 'Length' }))).toBe(false);
    expect(isDimensionEntryFilled(row({ name: 'Length', rawValue: '5mm' }))).toBe(true);
  });
});

function row(overrides: Partial<DimensionEntry> = {}): DimensionEntry {
  return { ...emptyDimensionEntry(), ...overrides };
}

function renderRows(dimensions: DimensionEntry[]) {
  return render(<DimensionFields dimensions={dimensions} onChange={vi.fn()} />);
}
