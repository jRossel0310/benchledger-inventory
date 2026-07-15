import { useEffect, useState } from 'react';
import { loadSnapshot, type SnapshotState } from './snapshot';

export function App() {
  const [state, setState] = useState<SnapshotState | null>(null);

  useEffect(() => {
    loadSnapshot().then(setState);
  }, []);

  return (
    <>
      <header className="banner">
        Read-only inventory snapshot
        {state?.kind === 'loaded' && <> — last published {state.header.publishedAt}</>}
      </header>
      <main className="main">
        {state === null && <p>Loading…</p>}
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
        {state?.kind === 'loaded' && (
          <div className="empty">
            <h2>{state.header.partCount} parts published</h2>
            <p>Inventory browsing arrives with the Phase 6 snapshot schema.</p>
          </div>
        )}
      </main>
    </>
  );
}
