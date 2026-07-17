import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useDebouncedCallback } from './useDebouncedCallback';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('useDebouncedCallback', () => {
  it('fires the callback once, after the delay', () => {
    const callback = vi.fn();
    const { result } = renderHook(() => useDebouncedCallback(callback, 200));

    result.current.run('a');
    expect(callback).not.toHaveBeenCalled();

    vi.advanceTimersByTime(199);
    expect(callback).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(callback).toHaveBeenCalledTimes(1);
    expect(callback).toHaveBeenCalledWith('a');
  });

  it('only runs once for a burst of calls, with the latest value winning', () => {
    const callback = vi.fn();
    const { result } = renderHook(() => useDebouncedCallback(callback, 200));

    result.current.run('a');
    vi.advanceTimersByTime(100);
    result.current.run('b');
    vi.advanceTimersByTime(100);
    result.current.run('c');
    vi.advanceTimersByTime(199);
    expect(callback).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(callback).toHaveBeenCalledTimes(1);
    expect(callback).toHaveBeenCalledWith('c');
  });

  it('flush runs the callback immediately and cancels the pending run', () => {
    const callback = vi.fn();
    const { result } = renderHook(() => useDebouncedCallback(callback, 200));

    result.current.run('a');
    result.current.flush('b');
    expect(callback).toHaveBeenCalledTimes(1);
    expect(callback).toHaveBeenCalledWith('b');

    vi.advanceTimersByTime(200);
    // The 'a' run that flush pre-empted must not also fire later.
    expect(callback).toHaveBeenCalledTimes(1);
  });

  it('cancel discards a pending run without ever invoking the callback', () => {
    const callback = vi.fn();
    const { result } = renderHook(() => useDebouncedCallback(callback, 200));

    result.current.run('a');
    result.current.cancel();
    vi.advanceTimersByTime(200);

    expect(callback).not.toHaveBeenCalled();
  });

  it('always calls the latest callback passed in, even for an already-scheduled run', () => {
    const first = vi.fn();
    const second = vi.fn();
    const { result, rerender } = renderHook(({ callback }) => useDebouncedCallback(callback, 200), {
      initialProps: { callback: first },
    });

    result.current.run('a');
    rerender({ callback: second });
    vi.advanceTimersByTime(200);

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith('a');
  });

  it('cancels a pending run on unmount', () => {
    const callback = vi.fn();
    const { result, unmount } = renderHook(() => useDebouncedCallback(callback, 200));

    result.current.run('a');
    unmount();
    vi.advanceTimersByTime(200);

    expect(callback).not.toHaveBeenCalled();
  });
});
