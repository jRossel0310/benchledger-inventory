/**
 * Per-line decision editor for the import review screen (Phase 5d Task 3):
 * a Radix `Popover` triggered from the review table's "Change…" action,
 * offering every way to resolve a `part`-kind line into a `LineDecision` —
 * pick one of the backend's `matches` (each showing its verdict badge +
 * explanation), "Match other part…"/"Add as variant to…" (the same
 * search-and-pick pattern `BomTable.tsx`'s `AddBomItemDialog`/
 * `EditBomItemDialog` use via `cmdk`'s `Command` over `useSearch`), "Create
 * new part" (a placeholder draft — Task 4's dialog completes it), and
 * "Skip". Every choice calls `onChange` with the exact `LineDecision` shape
 * and closes the popover; nothing here mutates anything itself — the caller
 * (`ImportReview.tsx`) owns the `decisions` map this only ever writes into.
 */

import * as Popover from '@radix-ui/react-popover';
import { Command } from 'cmdk';
import { useState } from 'react';

import type { ImportReviewLine, LineDecision, SearchHit } from '../../bindings.gen';
import { verdictLabel } from '../part/DuplicatePanel';
import { useSearch } from '../../hooks/inventory';
import {
  placeholderPartDraft,
  prefillListing,
  prefillVariant,
  type LineDecisionContext,
} from './lineDecisions';
import './LineActionEditor.css';

type PickerMode = 'add_stock' | 'add_as_variant' | null;

export interface LineActionEditorProps {
  line: ImportReviewLine;
  decision: LineDecision;
  onChange: (decision: LineDecision) => void;
  context: LineDecisionContext;
}

export function LineActionEditor({ line, decision, onChange, context }: LineActionEditorProps) {
  const [open, setOpen] = useState(false);
  const [pickerMode, setPickerMode] = useState<PickerMode>(null);
  const [query, setQuery] = useState('');

  const searchQuery = useSearch(query);
  const results = searchQuery.data ?? [];

  function closePopover(nextOpen: boolean) {
    setOpen(nextOpen);
    if (!nextOpen) {
      setPickerMode(null);
      setQuery('');
    }
  }

  function choose(next: LineDecision) {
    onChange(next);
    closePopover(false);
  }

  function pickPart(hit: SearchHit) {
    if (pickerMode === 'add_stock') {
      choose({ type: 'add_stock', part_id: hit.part_id });
    } else if (pickerMode === 'add_as_variant') {
      choose({
        type: 'add_as_variant',
        part_id: hit.part_id,
        variant: prefillVariant(line),
        listing: prefillListing(line, context),
      });
    }
  }

  return (
    <Popover.Root open={open} onOpenChange={closePopover}>
      <Popover.Trigger asChild>
        <button
          type="button"
          className="line-action-editor-trigger"
          aria-label={`Change decision for line ${line.line_number ?? ''}`}
        >
          Change…
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content className="line-action-editor-content" align="end" sideOffset={4}>
          {line.matches.length > 0 && pickerMode === null ? (
            <div className="line-action-editor-section">
              <p className="line-action-editor-section-label">Matches</p>
              <ul className="line-action-editor-matches">
                {line.matches.map((match) => (
                  <li key={match.part_id}>
                    <button
                      type="button"
                      className={`line-action-editor-match${
                        decision.type === 'add_stock' && decision.part_id === match.part_id
                          ? ' line-action-editor-match--selected'
                          : ''
                      }`}
                      onClick={() => choose({ type: 'add_stock', part_id: match.part_id })}
                    >
                      <span className="line-action-editor-match-verdict">
                        {verdictLabel(match.verdict_kind)}
                      </span>
                      <span className="line-action-editor-match-name">{match.display_name}</span>
                      <span className="line-action-editor-match-explanation">
                        {match.explanation}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}

          {pickerMode !== null ? (
            <div className="line-action-editor-section">
              <p className="line-action-editor-section-label">
                {pickerMode === 'add_stock' ? 'Match other part' : 'Add as variant to…'}
              </p>
              <Command
                shouldFilter={false}
                label="Search parts"
                className="line-action-editor-search"
              >
                <Command.Input
                  autoFocus
                  value={query}
                  onValueChange={setQuery}
                  placeholder="Search parts…"
                  className="line-action-editor-search-input"
                />
                <Command.List className="line-action-editor-search-list">
                  <Command.Empty className="line-action-editor-search-empty">
                    {query.trim() ? 'No parts match.' : 'Type to search a part…'}
                  </Command.Empty>
                  {results.map((hit) => (
                    <Command.Item
                      key={hit.part_id}
                      value={hit.part_id}
                      onSelect={() => pickPart(hit)}
                      className="line-action-editor-search-item"
                    >
                      <span className="line-action-editor-search-item-name">
                        {hit.display_name}
                      </span>
                      <span className="line-action-editor-search-item-meta">
                        {hit.category_name}
                      </span>
                    </Command.Item>
                  ))}
                </Command.List>
              </Command>
              <button
                type="button"
                className="line-action-editor-back"
                onClick={() => {
                  setPickerMode(null);
                  setQuery('');
                }}
              >
                Back
              </button>
            </div>
          ) : (
            <div className="line-action-editor-actions">
              <button
                type="button"
                className="line-action-editor-action"
                onClick={() => setPickerMode('add_stock')}
              >
                Match other part…
              </button>
              <button
                type="button"
                className="line-action-editor-action"
                onClick={() => setPickerMode('add_as_variant')}
              >
                Add as variant to…
              </button>
              <button
                type="button"
                className="line-action-editor-action"
                onClick={() =>
                  choose({
                    type: 'create_new',
                    draft: placeholderPartDraft(line),
                    variant: prefillVariant(line),
                    listing: prefillListing(line, context),
                  })
                }
              >
                Create new part
              </button>
              <button
                type="button"
                className="line-action-editor-action line-action-editor-action--skip"
                onClick={() => choose({ type: 'skip' })}
              >
                Skip
              </button>
            </div>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
