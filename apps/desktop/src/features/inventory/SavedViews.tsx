/**
 * Named saved search views for the Inventory browser (Phase 3 Task 4):
 * built-in presets that need no persistence, plus user-saved queries
 * persisted as a JSON blob under the `saved_views` settings key (see
 * `Database::get_setting`/`set_setting`, `crates/inventory-db/src/
 * settings.rs`, and the `getSetting`/`setSetting` commands).
 *
 * Presets are limited to queries the backend search grammar can actually
 * answer (verified against `crates/inventory-db/src/search.rs`'s
 * `apply_filter`): "All parts" (no filter), "Low stock" (`low stock`),
 * "Archived" (`is:archived`). "Unassigned bin" and "Recently used" — both
 * suggested by the design spec — have no corresponding search key (`bin:`
 * only supports an *exact* match, and there's no recency-of-use signal in
 * the schema), so they're deliberately omitted rather than linking to a
 * query that would 400 with `UnknownSearchKey`/return nothing — the same
 * "no dead controls" call the Task 3 dashboard made for its own unfilterable
 * cards.
 */

import { useState, type FormEvent } from 'react';

import { useSetSetting, useSetting } from '../../hooks/inventory';
import { useToast } from '../../components/Toast';
import { errorHint } from '../../lib/format';
import './SavedViews.css';

const SAVED_VIEWS_SETTING_KEY = 'saved_views';

interface BuiltInView {
  name: string;
  query: string;
}

const BUILT_IN_VIEWS: BuiltInView[] = [
  { name: 'All parts', query: '' },
  { name: 'Low stock', query: 'low stock' },
  { name: 'Archived', query: 'is:archived' },
];

interface SavedView {
  id: string;
  name: string;
  query: string;
}

function isSavedView(value: unknown): value is SavedView {
  return (
    !!value &&
    typeof value === 'object' &&
    typeof (value as SavedView).id === 'string' &&
    typeof (value as SavedView).name === 'string' &&
    typeof (value as SavedView).query === 'string'
  );
}

/** Parse the `saved_views` setting's raw JSON, degrading to an empty list
 * for anything that isn't a JSON array of `SavedView`-shaped objects
 * (missing key, corrupt/hand-edited value, a future format change) — a
 * saved view is a convenience, not data that should ever block the screen
 * from rendering. */
function parseSavedViews(raw: string | null | undefined): SavedView[] {
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isSavedView) : [];
  } catch {
    return [];
  }
}

export interface SavedViewsProps {
  /** The current composed query — what "save current view" persists. */
  query: string;
  onSelect: (query: string) => void;
}

export function SavedViews({ query, onSelect }: SavedViewsProps) {
  const settingQuery = useSetting(SAVED_VIEWS_SETTING_KEY);
  const savedViews = parseSavedViews(settingQuery.data);
  const { toast } = useToast();
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState('');

  const setSetting = useSetSetting({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not save the view',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      setNaming(false);
      setName('');
    },
  });

  function persist(next: SavedView[]) {
    setSetting.mutate({ key: SAVED_VIEWS_SETTING_KEY, value: JSON.stringify(next) });
  }

  function handleSave(event: FormEvent) {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    persist([
      ...savedViews,
      { id: `view-${Date.now()}-${Math.random().toString(36).slice(2)}`, name: trimmed, query },
    ]);
  }

  function handleRemove(id: string) {
    persist(savedViews.filter((view) => view.id !== id));
  }

  return (
    <div className="saved-views">
      {BUILT_IN_VIEWS.map((view) => (
        <button
          key={view.name}
          type="button"
          className={`saved-view-chip${query === view.query ? ' saved-view-chip-active' : ''}`}
          onClick={() => onSelect(view.query)}
        >
          {view.name}
        </button>
      ))}
      {savedViews.map((view) => (
        <span key={view.id} className="saved-view-chip-group">
          <button
            type="button"
            className={`saved-view-chip${query === view.query ? ' saved-view-chip-active' : ''}`}
            onClick={() => onSelect(view.query)}
          >
            {view.name}
          </button>
          <button
            type="button"
            className="saved-view-remove"
            aria-label={`Remove saved view ${view.name}`}
            onClick={() => handleRemove(view.id)}
          >
            ×
          </button>
        </span>
      ))}
      {naming ? (
        <form className="saved-view-form" onSubmit={handleSave}>
          <input
            className="saved-view-name-input"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="View name"
            autoFocus
          />
          <button
            type="submit"
            className="saved-view-form-save"
            disabled={setSetting.isPending || !name.trim()}
          >
            Save
          </button>
          <button
            type="button"
            className="saved-view-form-cancel"
            onClick={() => {
              setNaming(false);
              setName('');
            }}
          >
            Cancel
          </button>
        </form>
      ) : (
        <button type="button" className="saved-view-save-button" onClick={() => setNaming(true)}>
          + Save current view
        </button>
      )}
    </div>
  );
}
