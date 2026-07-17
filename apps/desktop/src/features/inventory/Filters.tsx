/**
 * Filter chips/controls for the Inventory browser (Phase 3 Task 4):
 * translate to search-query fragments appended to the shared `q` string via
 * the pure `queryFilters` helpers, so filtering composes with free text and
 * reuses the tested backend query engine rather than a client-side
 * re-filter. Only exposes filters the backend search grammar
 * (`crates/inventory-core/src/search.rs` + `crates/inventory-db/src/
 * search.rs`'s `apply_filter`) actually supports — category, low stock,
 * archived, has:datasheet, has:dimensions. No filter is added here for a
 * concept the grammar doesn't have a key for (e.g. "unassigned bin" — see
 * `SavedViews.tsx`'s docstring for the same rule applied to presets).
 */

import type { CategoryRecord } from '../../bindings.gen';
import {
  parseActiveFilters,
  withArchived,
  withCategory,
  withHasDatasheet,
  withHasDimensions,
  withLowStock,
} from './queryFilters';
import './Filters.css';

export interface FiltersProps {
  query: string;
  categories: CategoryRecord[];
  onChange: (nextQuery: string) => void;
}

export function Filters({ query, categories, onChange }: FiltersProps) {
  const active = parseActiveFilters(query);

  return (
    <div className="inventory-filters">
      <div className="inventory-filters-field">
        <label className="inventory-filters-label" htmlFor="inventory-category-filter">
          Category
        </label>
        <select
          id="inventory-category-filter"
          className="inventory-filters-select"
          value={active.category ?? ''}
          onChange={(event) => onChange(withCategory(query, event.target.value || null))}
        >
          <option value="">All categories</option>
          {categories.map((category) => (
            <option key={category.id} value={category.name}>
              {category.name}
            </option>
          ))}
        </select>
      </div>
      <FilterChip
        label="Low stock"
        active={active.lowStock}
        onToggle={() => onChange(withLowStock(query, !active.lowStock))}
      />
      <FilterChip
        label="Archived"
        active={active.archived}
        onToggle={() => onChange(withArchived(query, !active.archived))}
      />
      <FilterChip
        label="Has datasheet"
        active={active.hasDatasheet}
        onToggle={() => onChange(withHasDatasheet(query, !active.hasDatasheet))}
      />
      <FilterChip
        label="Has dimensions"
        active={active.hasDimensions}
        onToggle={() => onChange(withHasDimensions(query, !active.hasDimensions))}
      />
    </div>
  );
}

interface FilterChipProps {
  label: string;
  active: boolean;
  onToggle: () => void;
}

function FilterChip({ label, active, onToggle }: FilterChipProps) {
  return (
    <button
      type="button"
      className={`inventory-filter-chip${active ? ' inventory-filter-chip-active' : ''}`}
      aria-pressed={active}
      onClick={onToggle}
    >
      {label}
    </button>
  );
}
