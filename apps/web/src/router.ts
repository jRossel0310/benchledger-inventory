/**
 * Tiny hash router for the read-only companion site. Hash-based routes
 * (`#/`, `#/part/<id>`, `#/bins`, `#/projects`) sidestep the static-hosting
 * deep-link problem entirely: the server always serves the one index.html
 * and the fragment never reaches it, so no rewrite config is needed.
 *
 * `parseRoute` is total -- any hash it doesn't recognize (including a
 * `#/part/` with an empty id or extra path segments) maps to `notFound`,
 * which the app renders as an honest not-found panel rather than falling
 * back to home silently.
 */

import { useEffect, useState } from 'react';

export type Route =
  | { kind: 'home' }
  | { kind: 'part'; id: string }
  | { kind: 'bins' }
  | { kind: 'projects' }
  | { kind: 'notFound' };

/** Parse a `window.location.hash` value (with or without the leading `#`)
 * into a `Route`. Empty hash and `#/` are home; unknown shapes are
 * `notFound`, never a silent redirect. */
export function parseRoute(hash: string): Route {
  const path = hash.startsWith('#') ? hash.slice(1) : hash;
  if (path === '' || path === '/') return { kind: 'home' };
  if (path === '/bins') return { kind: 'bins' };
  if (path === '/projects') return { kind: 'projects' };
  if (path.startsWith('/part/')) {
    const id = path.slice('/part/'.length);
    if (id !== '' && !id.includes('/')) return { kind: 'part', id };
  }
  return { kind: 'notFound' };
}

/** The canonical hash for a route -- `parseRoute(formatRoute(r))` round-trips
 * for every addressable kind. `notFound` has no address of its own; it
 * formats as home (the only sensible link target for it). */
export function formatRoute(route: Route): string {
  switch (route.kind) {
    case 'home':
    case 'notFound':
      return '#/';
    case 'part':
      return `#/part/${route.id}`;
    case 'bins':
      return '#/bins';
    case 'projects':
      return '#/projects';
  }
}

/** The current route, kept in sync with the address bar via `hashchange`.
 * Listener is removed on unmount. */
export function useRoute(): Route {
  const [route, setRoute] = useState<Route>(() => parseRoute(window.location.hash));
  useEffect(() => {
    const onHashChange = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, []);
  return route;
}
