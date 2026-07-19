/**
 * BenchLedger's public companion site: header with the read-only banner and
 * the section nav, hash-routed views (home = the shared-grammar search box
 * (debounced ~150ms) + unsupported-filter chips (the honesty rule -- a
 * filter the snapshot can't evaluate is shown as ignored, never silently
 * dropped) + the inventory table; part detail; bins; projects), and an
 * honest not-found panel for unknown hashes.
 *
 * The search input lives on the home view only -- it drives the inventory
 * table and nothing else, so showing it elsewhere would be a lie.
 *
 * `load` is injectable for tests; production uses `loadSnapshot` (fetches
 * `/inventory.snapshot.json`, published by the desktop app).
 */

import { useEffect, useMemo, useState } from 'react';
import { Bins } from './Bins';
import { Inventory } from './Inventory';
import { PartDetail } from './PartDetail';
import { Projects } from './Projects';
import { useRoute } from './router';
import { searchSnapshot } from './searchSnapshot';
import { loadSnapshot, type SnapshotState } from './snapshot';

const SEARCH_DEBOUNCE_MS = 150;

export function App({ load = loadSnapshot }: { load?: () => Promise<SnapshotState> }) {
  const route = useRoute();
  const [state, setState] = useState<SnapshotState | null>(null);
  const [input, setInput] = useState('');
  const [query, setQuery] = useState('');

  useEffect(() => {
    let cancelled = false;
    load().then((s) => {
      if (!cancelled) setState(s);
    });
    return () => {
      cancelled = true;
    };
  }, [load]);

  useEffect(() => {
    const timer = setTimeout(() => setQuery(input), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [input]);

  const result = useMemo(
    () => (state?.kind === 'loaded' ? searchSnapshot(state.snapshot, query) : null),
    [state, query],
  );

  return (
    <>
      <header className="header">
        <div className="header-row">
          <h1 className="title">BenchLedger — Inventory</h1>
          <nav className="nav" aria-label="Sections">
            <a className="nav-link" href="#/">
              Inventory
            </a>
            <a className="nav-link" href="#/bins">
              Bins
            </a>
            <a className="nav-link" href="#/projects">
              Projects
            </a>
          </nav>
        </div>
        <p className="banner">
          Read-only inventory snapshot
          {state?.kind === 'loaded' && <> — last published {state.snapshot.publishedAt}</>}
        </p>
      </header>
      <main className="main">
        {state === null && <p className="loading">Loading…</p>}
        {state?.kind === 'none' && (
          <div className="empty">
            <h2>No snapshot published yet</h2>
            <p>The desktop application has not published an inventory snapshot to this site.</p>
          </div>
        )}
        {state?.kind === 'invalid' && (
          <div className="empty">
            <h2>Snapshot could not be read</h2>
            <p>The published snapshot file is not in a recognized format.</p>
          </div>
        )}
        {state?.kind === 'loaded' && route.kind === 'home' && result !== null && (
          <>
            <div className="search-row">
              <input
                type="search"
                className="search-input"
                placeholder="Search — e.g. resistor 0603, bin:A4, capacitance:0.1uF, low stock"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                aria-label="Search inventory"
              />
              <span className="result-count">
                {result.parts.length === 1 ? '1 part' : `${result.parts.length} parts`}
              </span>
            </div>
            {result.unsupported.length > 0 && (
              <div className="chip-row">
                <span className="chip-row-label">Unsupported here, ignored:</span>
                {result.unsupported.map((chip) => (
                  <span key={chip} className="chip">
                    {chip}
                  </span>
                ))}
              </div>
            )}
            <Inventory parts={result.parts} />
          </>
        )}
        {state?.kind === 'loaded' && route.kind === 'part' && (
          <PartDetail snapshot={state.snapshot} partId={route.id} />
        )}
        {state?.kind === 'loaded' && route.kind === 'bins' && <Bins snapshot={state.snapshot} />}
        {state?.kind === 'loaded' && route.kind === 'projects' && (
          <Projects snapshot={state.snapshot} />
        )}
        {state?.kind === 'loaded' && route.kind === 'notFound' && (
          <div className="empty">
            <h2>Page not found</h2>
            <p>This address does not match anything in the snapshot viewer.</p>
            <a className="empty-link" href="#/">
              ← Back to inventory
            </a>
          </div>
        )}
      </main>
    </>
  );
}
