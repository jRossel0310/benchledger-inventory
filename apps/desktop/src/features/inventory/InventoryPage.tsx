/**
 * The Inventory browser screen (Phase 3 Task 4): composes the saved-view
 * chips, filter chips, an on-screen search box, and the dense virtualized
 * table, all sharing one query string — the route's `?q=` search param
 * (`InventorySearch`, `app/routes.tsx`). The top command-bar search
 * (`AppShell.tsx`) also drives this same param, so typing there lands here
 * pre-filtered; this screen's own search box is the same control restated
 * inline, for editing the composed query (free text + filter fragments)
 * without leaving the table.
 */

import { useNavigate, useSearch } from '@tanstack/react-router';

import { TextField } from '../../components/Field';
import { useCategories } from '../../hooks/inventory';
import { Filters } from './Filters';
import { InventoryTable } from './InventoryTable';
import './InventoryPage.css';
import { SavedViews } from './SavedViews';

export function InventoryPage() {
  const { q } = useSearch({ from: '/inventory' });
  const navigate = useNavigate();
  const categoriesQuery = useCategories();

  function setQuery(next: string) {
    void navigate({ to: '/inventory', search: { q: next }, replace: true });
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
          value={q}
          onChange={setQuery}
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
