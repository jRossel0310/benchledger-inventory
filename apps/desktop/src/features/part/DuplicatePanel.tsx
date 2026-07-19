/**
 * Live duplicate detection for the part create/edit form (spec §9, §7, §10).
 * As identity-defining fields are entered, a debounced `find_matches`
 * (`useFindMatches`) scores the current form state — built by
 * `buildMatchCandidate` — against parts already on file, and this panel
 * renders each result with the backend's verbatim verdict + explanation and
 * the actions the brief names:
 *
 *  - **Add stock to this part** — opens the shared `QuickAction` add-stock
 *    dialog preselected to the matched part (`useQuickAction`), the "this is
 *    the same part I already have, just receive more" path.
 *  - **Add as a variant of this part** — `useAddVariant` onto the matched
 *    part using the form's primary variant (only offered when the form has
 *    one to add).
 *  - **Create separate part anyway** — dismisses this match; the new part is
 *    still created on save. (`record_equivalence`-rejected is only meaningful
 *    between two *existing* parts — a candidate being typed has no id yet —
 *    so for a new-part candidate "create anyway" is the honest path, not a
 *    rejected-equivalence write. When both ids exist, i.e. edit mode's own
 *    part vs. a match, "Not equivalent" instead records the rejection via
 *    `useRecordEquivalence`.)
 *
 * Explanations are shown exactly as the backend returns them (the
 * per-level explanation builders in `crates/inventory-db/src/matching.rs`
 * are deliberately specific — never re-summarized or reworded here).
 */

import { useEffect, useState } from 'react';

import type { MatchCandidate, MatchResult, PartId, VariantDraft } from '../../bindings.gen';
import { useAddVariant, useFindMatches, useRecordEquivalence } from '../../hooks/inventory';
import { useDebouncedCallback } from '../../hooks/useDebouncedCallback';
import { useToast } from '../../components/Toast';
import { errorHint } from '../../lib/format';
import { useQuickAction } from '../quick/QuickActionContext';
import './DuplicatePanel.css';

const FIND_MATCHES_DEBOUNCE_MS = 350;

/** Human labels for the backend's `verdict_kind` snake_case strings — the
 * verdict *name* is ours to phrase for display; the *explanation* below it
 * is always the backend's verbatim. Any unknown/future kind falls back to a
 * title-cased version of the raw string rather than being dropped. */
const VERDICT_LABELS: Record<string, string> = {
  exact_sku: 'Same supplier SKU',
  exact_mpn: 'Same manufacturer part number',
  known_alias: 'Known alias',
  exact_identity: 'Identical specs',
  probable_equivalent: 'Probably the same part',
  similar: 'Similar part',
};

/** Exported so other verdict-rendering surfaces (the import review screen's
 * match badges, Phase 5d Task 3) use the exact same label vocabulary rather
 * than a second, drifting copy of this map. */
export function verdictLabel(kind: string): string {
  return (
    VERDICT_LABELS[kind] ??
    kind
      .split('_')
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(' ')
  );
}

export interface DuplicatePanelProps {
  /** The candidate built from current form state, or `null` when there is
   * not enough identity signal yet (`buildMatchCandidate` returns `null`).
   * Debounced here before it reaches `find_matches` so typing doesn't fire a
   * request per keystroke. */
  candidate: MatchCandidate | null;
  /** The form's primary variant as a ready `VariantDraft`, when the form has
   * one — enables "Add as a variant of this part". `null` hides that action
   * (there's nothing to add as a variant yet). */
  primaryVariant: VariantDraft | null;
  /** In edit mode, the id of the part being edited — so a match against a
   * *different* existing part can be recorded as a rejected equivalence
   * ("Not equivalent"). Absent in create mode (the candidate has no id). */
  currentPartId?: PartId;
}

export function DuplicatePanel({ candidate, primaryVariant, currentPartId }: DuplicatePanelProps) {
  // Debounce the candidate the panel actually searches on: the parent
  // rebuilds `candidate` on every keystroke (so it always reflects live form
  // state), but the `find_matches` request waits for typing to settle. A
  // candidate going `null` (fields cleared below the "enough signal"
  // threshold) lands immediately — there's nothing to wait to settle, and a
  // stale panel should clear at once rather than after the debounce window.
  const [searchCandidate, setSearchCandidate] = useState<MatchCandidate | null>(candidate);
  const debounced = useDebouncedCallback(setSearchCandidate, FIND_MATCHES_DEBOUNCE_MS);
  useEffect(() => {
    if (candidate === null) {
      debounced.flush(null);
    } else {
      debounced.run(candidate);
    }
  }, [candidate, debounced]);

  const matches = useFindMatches(searchCandidate);
  const [dismissed, setDismissed] = useState<Set<PartId>>(new Set());

  const visible = (matches.data ?? []).filter((m) => !dismissed.has(m.part_id));

  if (searchCandidate === null || visible.length === 0) return null;

  function dismiss(partId: PartId) {
    setDismissed((current) => new Set(current).add(partId));
  }

  return (
    <section className="duplicate-panel" aria-label="Possible duplicates">
      <p className="duplicate-panel-heading">
        Possible {visible.length === 1 ? 'duplicate' : 'duplicates'} already in inventory
      </p>
      <ul className="duplicate-panel-list">
        {visible.map((match) => (
          <DuplicateMatch
            key={match.part_id}
            match={match}
            primaryVariant={primaryVariant}
            currentPartId={currentPartId}
            onDismiss={() => dismiss(match.part_id)}
          />
        ))}
      </ul>
    </section>
  );
}

interface DuplicateMatchProps {
  match: MatchResult;
  primaryVariant: VariantDraft | null;
  currentPartId?: PartId;
  onDismiss: () => void;
}

function DuplicateMatch({ match, primaryVariant, currentPartId, onDismiss }: DuplicateMatchProps) {
  const { open } = useQuickAction();
  const { toast } = useToast();

  const addVariant = useAddVariant({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not add variant',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: `Added variant to ${match.display_name}`, kind: 'success' });
      onDismiss();
    },
  });

  const recordEquivalence = useRecordEquivalence({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not record decision',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      onDismiss();
    },
  });

  function addStock() {
    open({ kind: 'receive', part: { id: match.part_id, displayName: match.display_name } });
  }

  function addAsVariant() {
    if (!primaryVariant) return;
    addVariant.mutate({ partId: match.part_id, draft: primaryVariant });
  }

  function notEquivalent() {
    // Only meaningful between two existing parts. In create mode the
    // candidate has no id, so "Not equivalent" isn't offered — "Create
    // separate part anyway" (a plain dismiss) is the path instead.
    if (!currentPartId) {
      onDismiss();
      return;
    }
    recordEquivalence.mutate({
      a: currentPartId,
      b: match.part_id,
      decision: 'rejected',
      note: '',
    });
  }

  return (
    <li className="duplicate-match">
      <div className="duplicate-match-header">
        <span className="duplicate-match-name">{match.display_name}</span>
        <span className="duplicate-match-verdict">{verdictLabel(match.verdict_kind)}</span>
      </div>
      <p className="duplicate-match-explanation">{match.explanation}</p>
      <div className="duplicate-match-actions">
        <button type="button" className="duplicate-match-action" onClick={addStock}>
          Add stock to this part
        </button>
        {primaryVariant ? (
          <button
            type="button"
            className="duplicate-match-action"
            onClick={addAsVariant}
            disabled={addVariant.isPending}
          >
            Add as a variant of this part
          </button>
        ) : null}
        <button type="button" className="duplicate-match-action" onClick={onDismiss}>
          Create separate part anyway
        </button>
        {currentPartId ? (
          <button
            type="button"
            className="duplicate-match-action duplicate-match-action--reject"
            onClick={notEquivalent}
            disabled={recordEquivalence.isPending}
          >
            Not equivalent
          </button>
        ) : null}
      </div>
    </li>
  );
}
