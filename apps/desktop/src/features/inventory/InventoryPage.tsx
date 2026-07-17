/**
 * The Inventory browser screen (Phase 3 Task 4): composes the saved-view
 * chips, filter chips, an on-screen search box, and the dense virtualized
 * table, all sharing one query string — the route's `?q=` search param
 * (`InventorySearch`, `app/routes.tsx`). The top command-bar search
 * (`AppShell.tsx`) also drives this same param, so typing there lands here
 * pre-filtered; this screen's own search box is the same control restated
 * inline, for editing the composed query (free text + filter fragments)
 * without leaving the table.
 *
 * The search box's route-param write is debounced (~200ms, see
 * `useDebouncedCallback`): each keystroke mints a new `keys.search(query)`
 * cache key (`hooks/inventory.ts`), so writing `q` on every keystroke was
 * spamming both browser history (via `replace`) and the query cache. The
 * box itself (`searchValue`) stays controlled/immediate — only the route
 * write lands after the delay — and clearing it flushes immediately rather
 * than leaving a stale query live for the debounce window. Saved views and
 * filter chips are discrete, already-debounced-by-nature actions (a click,
 * not a keystream), so they keep writing `q` immediately via `setQuery`.
 */

import { useNavigate, useSearch } from '@tanstack/react-router';
import { useEffect, useState } from 'react';

import { TextField } from '../../components/Field';
import { useCategories } from '../../hooks/inventory';
import { useDebouncedCallback } from '../../hooks/useDebouncedCallback';
import { Filters } from './Filters';
import { InventoryTable } from './InventoryTable';
import './InventoryPage.css';
import { SavedViews } from './SavedViews';

const SEARCH_DEBOUNCE_MS = 200;

export function InventoryPage() {
  const { q } = useSearch({ from: '/inventory' });
  const navigate = useNavigate();
  const categoriesQuery = useCategories();
  const [searchValue, setSearchValue] = useState(q);

  // `q` can change from outside the box itself (a filter chip, a saved
  // view, a Dashboard card link landing here with `?q=...`) — keep the box
  // in sync with those the same way `AppShell`'s top-bar search does.
  useEffect(() => {
    setSearchValue(q);
  }, [q]);

  function setQuery(next: string) {
    void navigate({ to: '/inventory', search: { q: next }, replace: true });
  }

  const debouncedSetQuery = useDebouncedCallback(setQuery, SEARCH_DEBOUNCE_MS);

  function handleSearchChange(value: string) {
    setSearchValue(value);
    // A cleared box should read as cleared immediately, not stay filtered
    // for the debounce window.
    if (value.trim().length === 0) {
      debouncedSetQuery.flush(value);
    } else {
      debouncedSetQuery.run(value);
    }
  }

  return (
    <section className="inventory-page">
      <header className="inventory-page-header">
        <p className="inventory-page-eyebrow">Inventory</p>
        <h1 className="inventory-page-title">The parts library</h1>
      </header>

      <SavedViews query={q} onSelect={setQuery} />

      <div className="inventory-page-toolbar">
        <TextField
          label="Search"
          value={searchValue}
          onChange={handleSearchChange}
          placeholder="10k 0603, category:Resistor, low stock…"
        />
        <Filters query={q} categories={categoriesQuery.data ?? []} onChange={setQuery} />
      </div>

      <div className="inventory-page-table">
        <InventoryTable query={q} />
      </div>
    </section>
  );
}
