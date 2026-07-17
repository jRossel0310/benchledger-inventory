import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { CategoryRecord } from '../../bindings.gen';
import { Filters } from './Filters';

const CATEGORIES: CategoryRecord[] = [
  { id: 'c1', name: 'Resistor', group_name: 'Passives', built_in: true },
  { id: 'c2', name: 'Voltage regulator', group_name: 'Power', built_in: true },
];

afterEach(cleanup);

describe('Filters', () => {
  it('renders a chip per boolean filter and a category select', () => {
    render(<Filters query="" categories={CATEGORIES} onChange={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Low stock' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Archived' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Has datasheet' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Has dimensions' })).toBeTruthy();
    expect(screen.getByLabelText('Category')).toBeTruthy();
  });

  it('clicking Low stock appends the "low stock" fragment', () => {
    const onChange = vi.fn();
    render(<Filters query="10k" categories={CATEGORIES} onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Low stock' }));

    expect(onChange).toHaveBeenCalledWith('10k low stock');
  });

  it('clicking an already-active Low stock chip removes the fragment', () => {
    const onChange = vi.fn();
    render(<Filters query="10k low stock" categories={CATEGORIES} onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Low stock' }));

    expect(onChange).toHaveBeenCalledWith('10k');
  });

  it('marks the chip active (aria-pressed) when its fragment is present in the query', () => {
    render(<Filters query="is:archived" categories={CATEGORIES} onChange={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Archived' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    expect(screen.getByRole('button', { name: 'Low stock' }).getAttribute('aria-pressed')).toBe(
      'false',
    );
  });

  it('clicking Has datasheet appends has:datasheet', () => {
    const onChange = vi.fn();
    render(<Filters query="" categories={CATEGORIES} onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Has datasheet' }));

    expect(onChange).toHaveBeenCalledWith('has:datasheet');
  });

  it('clicking Has dimensions appends has:dimensions', () => {
    const onChange = vi.fn();
    render(<Filters query="" categories={CATEGORIES} onChange={onChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Has dimensions' }));

    expect(onChange).toHaveBeenCalledWith('has:dimensions');
  });

  it('selecting a category appends a quoted category: fragment when the name has whitespace', () => {
    const onChange = vi.fn();
    render(<Filters query="" categories={CATEGORIES} onChange={onChange} />);

    fireEvent.change(screen.getByLabelText('Category'), {
      target: { value: 'Voltage regulator' },
    });

    expect(onChange).toHaveBeenCalledWith('category:"Voltage regulator"');
  });

  it('selecting "All categories" clears an existing category filter', () => {
    const onChange = vi.fn();
    render(<Filters query="category:Resistor" categories={CATEGORIES} onChange={onChange} />);

    fireEvent.change(screen.getByLabelText('Category'), { target: { value: '' } });

    expect(onChange).toHaveBeenCalledWith('');
  });

  it('reflects the current category selection from the query', () => {
    render(<Filters query="category:Resistor" categories={CATEGORIES} onChange={vi.fn()} />);

    expect((screen.getByLabelText('Category') as HTMLSelectElement).value).toBe('Resistor');
  });
});
