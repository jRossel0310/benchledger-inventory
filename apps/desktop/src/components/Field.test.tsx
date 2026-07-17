import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { CheckboxField, MultiSelectField, NumberField, SelectField, TextField } from './Field';

afterEach(cleanup);

describe('TextField', () => {
  it('renders a labeled text input showing the current value', () => {
    render(<TextField label="Display name" value="Resistor" onChange={() => {}} />);
    const input = screen.getByLabelText('Display name') as HTMLInputElement;
    expect(input.value).toBe('Resistor');
    expect(input.type).toBe('text');
  });

  it('reports edits through onChange', () => {
    const onChange = vi.fn();
    render(<TextField label="Display name" value="Resistor" onChange={onChange} />);
    fireEvent.change(screen.getByLabelText('Display name'), { target: { value: 'Capacitor' } });
    expect(onChange).toHaveBeenCalledWith('Capacitor');
  });

  it('renders a hint or an error, preferring the error when both are given', () => {
    const { rerender } = render(
      <TextField label="Bin" value="" onChange={() => {}} hint="e.g. A12" />,
    );
    expect(screen.getByText('e.g. A12')).toBeTruthy();

    rerender(
      <TextField label="Bin" value="" onChange={() => {}} hint="e.g. A12" error="Required" />,
    );
    expect(screen.getByText('Required')).toBeTruthy();
    expect(screen.queryByText('e.g. A12')).toBeNull();
  });
});

describe('NumberField', () => {
  it('parses a typed value into a number', () => {
    const onChange = vi.fn();
    render(<NumberField label="Quantity" value={5} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '12' } });
    expect(onChange).toHaveBeenCalledWith(12);
  });

  it('reports an emptied field as "" rather than 0', () => {
    const onChange = vi.fn();
    render(<NumberField label="Quantity" value={5} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText('Quantity'), { target: { value: '' } });
    expect(onChange).toHaveBeenCalledWith('');
  });
});

describe('SelectField', () => {
  it('renders every option and reports the selected value', () => {
    const onChange = vi.fn();
    render(
      <SelectField
        label="Unit"
        value="each"
        onChange={onChange}
        options={[
          { value: 'each', label: 'Each' },
          { value: 'meter', label: 'Meter' },
        ]}
      />,
    );
    const select = screen.getByLabelText('Unit') as HTMLSelectElement;
    expect(select.value).toBe('each');
    fireEvent.change(select, { target: { value: 'meter' } });
    expect(onChange).toHaveBeenCalledWith('meter');
  });
});

describe('CheckboxField', () => {
  it('renders a labeled checkbox reflecting the current checked state', () => {
    render(<CheckboxField label="Polarized" checked onChange={() => {}} />);
    const checkbox = screen.getByLabelText('Polarized') as HTMLInputElement;
    expect(checkbox.type).toBe('checkbox');
    expect(checkbox.checked).toBe(true);
  });

  it('reports toggles through onChange', () => {
    const onChange = vi.fn();
    render(<CheckboxField label="Polarized" checked={false} onChange={onChange} />);
    fireEvent.click(screen.getByLabelText('Polarized'));
    expect(onChange).toHaveBeenCalledWith(true);
  });
});

describe('MultiSelectField', () => {
  const OPTIONS = [
    { value: 'kicad_symbol', label: 'KiCad symbol' },
    { value: 'datasheet', label: 'Datasheet' },
    { value: 'step', label: 'STEP model' },
  ];

  it('renders every option as its own checkbox, checked per the current values', () => {
    render(
      <MultiSelectField
        label="Kinds"
        values={['datasheet']}
        onChange={() => {}}
        options={OPTIONS}
      />,
    );
    expect((screen.getByLabelText('KiCad symbol') as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText('Datasheet') as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText('STEP model') as HTMLInputElement).checked).toBe(false);
  });

  it('adds a value when its checkbox is checked', () => {
    const onChange = vi.fn();
    render(
      <MultiSelectField
        label="Kinds"
        values={['datasheet']}
        onChange={onChange}
        options={OPTIONS}
      />,
    );
    fireEvent.click(screen.getByLabelText('STEP model'));
    expect(onChange).toHaveBeenCalledWith(['datasheet', 'step']);
  });

  it('removes a value when its checkbox is unchecked', () => {
    const onChange = vi.fn();
    render(
      <MultiSelectField
        label="Kinds"
        values={['datasheet', 'step']}
        onChange={onChange}
        options={OPTIONS}
      />,
    );
    fireEvent.click(screen.getByLabelText('Datasheet'));
    expect(onChange).toHaveBeenCalledWith(['step']);
  });
});
