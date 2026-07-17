import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { TagInput } from './TagInput';

afterEach(cleanup);

describe('TagInput', () => {
  it('adds a tag on Enter and clears the input', () => {
    const onChange = vi.fn();
    render(<TagInput tags={[]} onChange={onChange} />);
    const input = screen.getByLabelText('Tags') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'smd' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onChange).toHaveBeenCalledWith(['smd']);
  });

  it('adds a tag on comma', () => {
    const onChange = vi.fn();
    render(<TagInput tags={['smd']} onChange={onChange} />);
    const input = screen.getByLabelText('Tags');
    fireEvent.change(input, { target: { value: 'e96' } });
    fireEvent.keyDown(input, { key: ',' });
    expect(onChange).toHaveBeenCalledWith(['smd', 'e96']);
  });

  it('rejects a duplicate tag', () => {
    const onChange = vi.fn();
    render(<TagInput tags={['smd']} onChange={onChange} />);
    const input = screen.getByLabelText('Tags');
    fireEvent.change(input, { target: { value: 'smd' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onChange).not.toHaveBeenCalled();
  });

  it('renders a chip per tag and removes one via its × button', () => {
    const onChange = vi.fn();
    render(<TagInput tags={['smd', 'e96']} onChange={onChange} />);
    expect(screen.getByText('smd')).toBeTruthy();
    fireEvent.click(screen.getByLabelText('Remove tag smd'));
    expect(onChange).toHaveBeenCalledWith(['e96']);
  });

  it('removes the last chip on Backspace in an empty input', () => {
    const onChange = vi.fn();
    render(<TagInput tags={['smd', 'e96']} onChange={onChange} />);
    fireEvent.keyDown(screen.getByLabelText('Tags'), { key: 'Backspace' });
    expect(onChange).toHaveBeenCalledWith(['smd']);
  });
});
