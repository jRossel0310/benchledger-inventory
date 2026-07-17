/**
 * The Ctrl+K command palette — the keyboard-first spine of the desktop app
 * (design direction §"Ctrl+K command palette is the keyboard-first spine:
 * fuzzy over the quick actions ... and over parts/bins. This is the 'one
 * keystroke' promise made real."). Bound globally (works from any route —
 * mounted once in `AppShell`, which wraps every routed screen): fuzzy-
 * matches the quick actions by typed text, fuzzy-matches parts via the same
 * `useSearch` the top command-bar search and the Inventory browser use, and
 * surfaces bins (distinct `bin_label`s among those part results) as a
 * shortcut into a `bin:`-filtered Inventory view. Selecting an action opens
 * the shared `QuickAction` dialog (via `useQuickAction()`); selecting a part
 * navigates to its detail route; selecting a bin navigates to a filtered
 * Inventory view.
 *
 * Built on `cmdk`'s `Command.Dialog` (a Radix Dialog under the hood, so Esc/
 * focus-trap/overlay all come for free) with `shouldFilter={false}`: parts
 * are already fuzzy-ranked server-side by `search`, and the action list is
 * short enough that a plain case-insensitive substring filter is both
 * predictable and simple to keep correct — cmdk still owns all the
 * keyboard navigation (arrow keys, Enter, Home/End) over whatever `Item`s
 * this component chooses to render.
 */

import { useNavigate } from '@tanstack/react-router';
import { Command } from 'cmdk';
import { useEffect, useMemo, useState } from 'react';

import type { SearchHit } from '../../bindings.gen';
import { useSearch } from '../../hooks/inventory';
import { QUICK_ACTIONS } from './quickActionConfig';
import { useQuickAction } from './QuickActionContext';
import './CommandPalette.css';

interface PaletteAction {
  id: string;
  label: string;
  run: () => void;
}

const MAX_PARTS = 8;
const MAX_BINS = 5;

/** Distinct `bin_label`s among `hits` whose label itself contains `query` —
 * not merely "some hit that matched happens to have a bin" — so the Bins
 * group reads as a real bin-code lookup, not noise off unrelated part
 * matches. */
function matchingBins(hits: SearchHit[], query: string): string[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const seen = new Set<string>();
  const out: string[] = [];
  for (const hit of hits) {
    const bin = hit.bin_label;
    if (!bin || seen.has(bin) || !bin.toLowerCase().includes(q)) continue;
    seen.add(bin);
    out.push(bin);
    if (out.length >= MAX_BINS) break;
  }
  return out;
}

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const navigate = useNavigate();
  const quickAction = useQuickAction();
  const partsQuery = useSearch(query);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setOpen((current) => !current);
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  function handleOpenChange(next: boolean) {
    setOpen(next);
    if (next) setQuery('');
  }

  const actions = useMemo<PaletteAction[]>(
    () => [
      ...QUICK_ACTIONS.map((action) => ({
        id: action.kind,
        label: action.label,
        run: () => quickAction.open({ kind: action.kind }),
      })),
      {
        id: 'create_part',
        label: 'Create part',
        run: () => void navigate({ to: '/inventory/new' }),
      },
      {
        id: 'import_order',
        label: 'Import order',
        run: () => void navigate({ to: '/orders' }),
      },
    ],
    [quickAction, navigate],
  );

  const filteredActions = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return actions;
    return actions.filter((action) => action.label.toLowerCase().includes(q));
  }, [actions, query]);

  const parts = (partsQuery.data ?? []).slice(0, MAX_PARTS);
  const bins = useMemo(() => matchingBins(partsQuery.data ?? [], query), [partsQuery.data, query]);

  function selectAction(action: PaletteAction) {
    setOpen(false);
    action.run();
  }

  function selectPart(hit: SearchHit) {
    setOpen(false);
    void navigate({ to: '/inventory/$partId', params: { partId: hit.part_id } });
  }

  function selectBin(bin: string) {
    setOpen(false);
    void navigate({ to: '/inventory', search: { q: `bin:${bin}` } });
  }

  return (
    <Command.Dialog
      open={open}
      onOpenChange={handleOpenChange}
      label="Command palette"
      shouldFilter={false}
      loop
      overlayClassName="command-palette-overlay"
      contentClassName="command-palette-content"
    >
      <Command.Input
        autoFocus
        value={query}
        onValueChange={setQuery}
        placeholder="Search actions, parts, bins…"
        className="command-palette-input"
      />
      <Command.List className="command-palette-list">
        <Command.Empty className="command-palette-empty">No matches.</Command.Empty>

        {filteredActions.length > 0 ? (
          <Command.Group heading="Actions" className="command-palette-group">
            {filteredActions.map((action) => (
              <Command.Item
                key={action.id}
                value={`action-${action.id}`}
                onSelect={() => selectAction(action)}
                className="command-palette-item"
              >
                {action.label}
              </Command.Item>
            ))}
          </Command.Group>
        ) : null}

        {parts.length > 0 ? (
          <Command.Group heading="Parts" className="command-palette-group">
            {parts.map((hit) => (
              <Command.Item
                key={hit.part_id}
                value={`part-${hit.part_id}`}
                onSelect={() => selectPart(hit)}
                className="command-palette-item"
              >
                <span className="command-palette-item-name">{hit.display_name}</span>
                <span className="command-palette-item-meta">
                  {hit.category_name}
                  {hit.bin_label ? ` · ${hit.bin_label}` : ''}
                </span>
              </Command.Item>
            ))}
          </Command.Group>
        ) : null}

        {bins.length > 0 ? (
          <Command.Group heading="Bins" className="command-palette-group">
            {bins.map((bin) => (
              <Command.Item
                key={bin}
                value={`bin-${bin}`}
                onSelect={() => selectBin(bin)}
                className="command-palette-item"
              >
                Bin {bin}
              </Command.Item>
            ))}
          </Command.Group>
        ) : null}
      </Command.List>
    </Command.Dialog>
  );
}
