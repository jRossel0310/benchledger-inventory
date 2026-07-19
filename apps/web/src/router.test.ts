import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { formatRoute, parseRoute, useRoute, type Route } from './router';

afterEach(() => {
  cleanup();
  window.location.hash = '';
});

describe('parseRoute', () => {
  it('maps the empty hash and #/ to home', () => {
    expect(parseRoute('')).toEqual({ kind: 'home' });
    expect(parseRoute('#')).toEqual({ kind: 'home' });
    expect(parseRoute('#/')).toEqual({ kind: 'home' });
  });

  it('parses the bins and projects routes', () => {
    expect(parseRoute('#/bins')).toEqual({ kind: 'bins' });
    expect(parseRoute('#/projects')).toEqual({ kind: 'projects' });
  });

  it('parses a part route with its id', () => {
    expect(parseRoute('#/part/ID000000000000000000000005')).toEqual({
      kind: 'part',
      id: 'ID000000000000000000000005',
    });
  });

  it('maps unknown hashes to notFound instead of falling back to home', () => {
    expect(parseRoute('#/nope')).toEqual({ kind: 'notFound' });
    expect(parseRoute('#/part/')).toEqual({ kind: 'notFound' });
    expect(parseRoute('#/part/abc/extra')).toEqual({ kind: 'notFound' });
    expect(parseRoute('#/bins/extra')).toEqual({ kind: 'notFound' });
  });

  it('round-trips every addressable route through formatRoute', () => {
    const routes: Route[] = [
      { kind: 'home' },
      { kind: 'part', id: 'ID000000000000000000000005' },
      { kind: 'bins' },
      { kind: 'projects' },
    ];
    for (const route of routes) {
      expect(parseRoute(formatRoute(route))).toEqual(route);
    }
  });

  it('formats notFound as home (its only sensible link target)', () => {
    expect(formatRoute({ kind: 'notFound' })).toBe('#/');
  });
});

describe('useRoute', () => {
  it('reflects the initial hash and follows hashchange events', () => {
    window.location.hash = '#/bins';
    const { result } = renderHook(() => useRoute());
    expect(result.current).toEqual({ kind: 'bins' });

    act(() => {
      window.location.hash = '#/part/abc';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    expect(result.current).toEqual({ kind: 'part', id: 'abc' });
  });

  it('removes its hashchange listener on unmount', () => {
    const { result, unmount } = renderHook(() => useRoute());
    unmount();
    act(() => {
      window.location.hash = '#/projects';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    // The unmounted hook must not have updated (and must not throw on the
    // event); the last rendered value is still the initial route.
    expect(result.current).toEqual({ kind: 'home' });
  });
});
