/**
 * The enrichment compare-and-apply diff dialog (Phase 5d Task 5, spec §11):
 * the human side of the backend-enforced trust rule in
 * `inventory_db::enrichment` (module doc's `requires_review` rule). Opened
 * from `PartDetail`'s "Refresh product data" button, which — per this
 * module's own contract — mounts this component only once clicked; there is
 * no other entry point, so `useEnrichmentPreview(partId, {enabled: true})`
 * below never fires before the user actually asks for a refresh (the hook's
 * own lazy default is `false`, but this dialog only ever exists once the
 * caller has decided to enable it).
 *
 * # The acknowledgement rule (do not weaken this)
 *
 * Every `FieldDiff` gets an `include` checkbox. For a `requires_review`
 * (protected) row, that checkbox is NOT enough to arm it — the backend
 * itself will reject `apply_enrichment` with `EnrichmentReviewRequired` for
 * a protected field whose `AppliedField.acknowledge_review` isn't `true`,
 * and this dialog mirrors that at the UI layer so the rejection is
 * vanishingly rare (only a genuine race, e.g. someone else editing the part
 * concurrently) rather than the normal path. A protected row additionally
 * needs its own explicit confirmation checkbox ticked — "Overwrite
 * manually-set value" when the field's CURRENT provenance is `'manual'`, or
 * "Accept inferred over existing" when the low-confidence description
 * parser is proposing to overwrite a value that already has some other
 * source — before it is armed. A row that is `include`d but not (yet)
 * confirmed is simply left out of the apply payload, with a small inline
 * hint explaining why, rather than either silently arming it or blocking
 * every OTHER row's apply — see `armedDiffs` below. Unchecking `include`
 * also resets that row's confirmation, so re-checking it later requires a
 * fresh, deliberate re-confirmation rather than a stale one carrying over.
 *
 * "Select all" (only rendered when at least one unprotected row exists)
 * arms every UNPROTECTED row's `include` checkbox — it never touches a
 * protected row's `include` OR its confirmation, because a bulk control
 * setting `acknowledge_review: true` on a human's behalf is exactly what
 * the trust rule exists to prevent (spec's Global Constraints: "never by
 * bulk select-all").
 */

import * as Dialog from '@radix-ui/react-dialog';
import { Link } from '@tanstack/react-router';
import { useMemo, useRef, useState } from 'react';

import type { AppliedField, FieldDiff, PartId } from '../../bindings.gen';
import { useToast } from '../../components/Toast';
import { useApplyEnrichment, useDigiKeyStatus, useEnrichmentPreview } from '../../hooks/enrichment';
import { errorHint, errorMessage } from '../../lib/format';
import './EnrichmentDiffDialog.css';

export interface EnrichmentDiffDialogProps {
  partId: PartId;
  onClose: () => void;
}

const VARIANT_FIELD_LABELS: Record<string, string> = {
  'variant.datasheet_url': 'Datasheet URL',
  'variant.product_url': 'Product URL',
  'variant.lifecycle': 'Lifecycle',
  'variant.package': 'Package',
};

/** Title-case a snake_case/dotted fragment into readable words — used for
 * both `attr.<key>` suffixes and any field key not in the fixed label
 * tables above, so a future field key never renders as raw
 * `variant.foo_bar`. */
function humanize(fragment: string): string {
  return fragment
    .split(/[._]/)
    .filter((word) => word.length > 0)
    .map((word) => `${word[0]?.toUpperCase() ?? ''}${word.slice(1)}`)
    .join(' ');
}

/** A `FieldDiff.key` as a readable row label: the fixed `variant.*` labels,
 * `description`/`category` as-is (title-cased), an `attr.<key>` humanized
 * from its suffix, or a humanized fallback for anything else. */
function fieldLabel(key: string): string {
  if (key in VARIANT_FIELD_LABELS) return VARIANT_FIELD_LABELS[key] ?? humanize(key);
  if (key === 'description') return 'Description';
  if (key === 'category') return 'Category';
  if (key.startsWith('attr.')) return humanize(key.slice('attr.'.length));
  return humanize(key);
}

const SOURCE_LABELS: Record<string, string> = {
  digikey: 'DigiKey',
  inferred: 'Inferred',
  invoice: 'Invoice',
  manufacturer: 'Manufacturer',
  datasheet: 'Datasheet',
  manual: 'Manual',
  measured: 'Measured',
};

function sourceLabel(source: string): string {
  return SOURCE_LABELS[source] ?? humanize(source);
}

/** The source badge's token-color class — `digikey` (a trusted API) reads
 * as the action-primary blue, `inferred` (the low-confidence description
 * parser) reads as the same warning amber a protected row's accent uses,
 * and everything else (a source this preview can't currently produce, but
 * `field_provenance`'s CHECK set allows) falls back to a neutral muted
 * badge rather than guessing a color for it. */
function sourceClass(source: string): string {
  if (source === 'digikey') return 'enrichment-diff-source--digikey';
  if (source === 'inferred') return 'enrichment-diff-source--inferred';
  return 'enrichment-diff-source--other';
}

/** Which explicit confirmation copy a protected row needs, matching
 * `is_protected_field`'s two triggers: a `current_source` of `'manual'`
 * takes priority (a human's own value is the more serious thing to
 * overwrite) over the inferred-over-existing case. */
function confirmLabel(diff: FieldDiff): string {
  if (diff.current_source === 'manual') return 'Overwrite manually-set value';
  return 'Accept inferred over existing';
}

interface RowState {
  include: boolean;
  confirm: boolean;
}

function initialRowState(diffs: FieldDiff[]): Record<string, RowState> {
  const state: Record<string, RowState> = {};
  for (const d of diffs) {
    state[d.key] = { include: false, confirm: false };
  }
  return state;
}

export function EnrichmentDiffDialog({ partId, onClose }: EnrichmentDiffDialogProps) {
  const { toast } = useToast();
  const previewQuery = useEnrichmentPreview(partId, { enabled: true });
  const statusQuery = useDigiKeyStatus();

  const [rows, setRows] = useState<Record<string, RowState>>({});
  const [brokenImages, setBrokenImages] = useState<Set<string>>(new Set());
  const [applyError, setApplyError] = useState<string | null>(null);
  const initializedForRef = useRef<string | null>(null);
  const appliedCountRef = useRef(0);

  const diffs = previewQuery.data?.diffs ?? [];

  // Initialize row state exactly once for this preview fetch (a fresh dialog
  // mount always starts with a clean ref, since closing unmounts it) — this
  // guard exists so a background refetch while the dialog stays open (e.g.
  // TanStack Query revalidating a stale preview) can't silently blow away
  // row state the user has already started arming.
  if (previewQuery.data && initializedForRef.current !== previewQuery.data.part_id) {
    initializedForRef.current = previewQuery.data.part_id;
    setRows(initialRowState(previewQuery.data.diffs));
  }

  const applyMutation = useApplyEnrichment({
    onDone: (error) => {
      if (error) {
        setApplyError(errorHint(error.code) ?? error.message);
        return;
      }
      const count = appliedCountRef.current;
      toast({
        title: `Applied ${count} field${count === 1 ? '' : 's'}`,
        kind: 'success',
      });
      onClose();
    },
  });

  const unprotectedKeys = useMemo(
    () => diffs.filter((d) => !d.requires_review).map((d) => d.key),
    [diffs],
  );
  const allUnprotectedSelected =
    unprotectedKeys.length > 0 && unprotectedKeys.every((k) => rows[k]?.include);

  const armedDiffs = diffs.filter((d) => {
    const r = rows[d.key];
    if (!r?.include) return false;
    if (d.requires_review) return r.confirm;
    return true;
  });

  function setInclude(key: string, include: boolean) {
    setRows((current) => ({
      ...current,
      // Unchecking include also clears any confirmation for this row — see
      // the module doc: re-checking later must re-earn the confirmation
      // rather than silently resurrecting a stale one.
      [key]: { include, confirm: include ? (current[key]?.confirm ?? false) : false },
    }));
  }

  function setConfirm(key: string, confirm: boolean) {
    setRows((current) => ({
      ...current,
      [key]: { include: current[key]?.include ?? false, confirm },
    }));
  }

  function toggleSelectAll() {
    const nextValue = !allUnprotectedSelected;
    setRows((current) => {
      const next = { ...current };
      for (const key of unprotectedKeys) {
        next[key] = { include: nextValue, confirm: next[key]?.confirm ?? false };
      }
      return next;
    });
  }

  function handleApply() {
    setApplyError(null);
    const applied: AppliedField[] = armedDiffs.map((d) => ({
      key: d.key,
      value: d.proposed,
      source: d.source,
      acknowledge_review: d.requires_review,
    }));
    appliedCountRef.current = applied.length;
    applyMutation.mutate({ partId, applied });
  }

  const images = (previewQuery.data?.images ?? []).filter((url) => !brokenImages.has(url));

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="enrichment-diff-overlay" />
        <Dialog.Content className="enrichment-diff-content">
          <Dialog.Title className="enrichment-diff-title">Refresh product data</Dialog.Title>
          <Dialog.Description className="enrichment-diff-description">
            Compare and approve the fields DigiKey and the description parser propose.
          </Dialog.Description>

          {previewQuery.isPending ? (
            <p className="enrichment-diff-status">Loading product data…</p>
          ) : previewQuery.isError ? (
            <p className="enrichment-diff-status enrichment-diff-status-error">
              Could not load product data: {errorMessage(previewQuery.error)}
            </p>
          ) : (
            <>
              {statusQuery.data && !statusQuery.data.configured ? (
                <p className="enrichment-diff-not-configured">
                  DigiKey isn&apos;t configured — description-based suggestions only.{' '}
                  <Link to="/settings" className="enrichment-diff-settings-link">
                    Configure in Settings
                  </Link>
                  .
                </p>
              ) : null}

              {images.length > 0 ? (
                <div className="enrichment-diff-images">
                  {images.map((url) => (
                    <img
                      key={url}
                      src={url}
                      alt="Product image"
                      className="enrichment-diff-image"
                      onError={() =>
                        setBrokenImages((current) => {
                          const next = new Set(current);
                          next.add(url);
                          return next;
                        })
                      }
                    />
                  ))}
                </div>
              ) : null}

              {diffs.length === 0 ? (
                <p className="enrichment-diff-status">
                  No differences — every field DigiKey and the description parser could find already
                  matches this part.
                </p>
              ) : (
                <>
                  {unprotectedKeys.length > 0 ? (
                    <label className="enrichment-diff-select-all">
                      <input
                        type="checkbox"
                        aria-label="Select all"
                        checked={allUnprotectedSelected}
                        onChange={toggleSelectAll}
                      />
                      Select all (unprotected fields only)
                    </label>
                  ) : null}

                  <ul className="enrichment-diff-list">
                    {diffs.map((d) => {
                      const state = rows[d.key] ?? { include: false, confirm: false };
                      const hint = d.requires_review && state.include && !state.confirm;
                      return (
                        <li
                          key={d.key}
                          className={
                            d.requires_review
                              ? 'enrichment-diff-row enrichment-diff-row--protected'
                              : 'enrichment-diff-row'
                          }
                        >
                          <input
                            type="checkbox"
                            aria-label={`Include ${fieldLabel(d.key)}`}
                            checked={state.include}
                            onChange={(e) => setInclude(d.key, e.target.checked)}
                            className="enrichment-diff-row-checkbox"
                          />
                          <div className="enrichment-diff-row-body">
                            <div className="enrichment-diff-row-header">
                              <span className="enrichment-diff-row-label">{fieldLabel(d.key)}</span>
                              <span className={`enrichment-diff-source ${sourceClass(d.source)}`}>
                                {sourceLabel(d.source)}
                              </span>
                            </div>
                            <div className="enrichment-diff-row-values">
                              <span className="enrichment-diff-row-current">
                                {d.current ?? '—'}
                              </span>
                              <span className="enrichment-diff-row-arrow" aria-hidden>
                                →
                              </span>
                              <span className="enrichment-diff-row-proposed">{d.proposed}</span>
                            </div>
                            {d.requires_review ? (
                              <label className="enrichment-diff-row-confirm">
                                <input
                                  type="checkbox"
                                  checked={state.confirm}
                                  disabled={!state.include}
                                  onChange={(e) => setConfirm(d.key, e.target.checked)}
                                />
                                {confirmLabel(d)}
                              </label>
                            ) : null}
                            {hint ? (
                              <p className="enrichment-diff-row-hint">
                                Confirm to include this change.
                              </p>
                            ) : null}
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                </>
              )}

              {(previewQuery.data?.notes.length ?? 0) > 0 ||
              (previewQuery.data?.provider_summary.length ?? 0) > 0 ? (
                <details className="enrichment-diff-footer">
                  <summary>Details</summary>
                  {(previewQuery.data?.provider_summary ?? []).map((line, i) => (
                    <p key={`summary-${i}`} className="enrichment-diff-footer-line">
                      {line}
                    </p>
                  ))}
                  {(previewQuery.data?.notes ?? []).map((line, i) => (
                    <p key={`note-${i}`} className="enrichment-diff-footer-line">
                      {line}
                    </p>
                  ))}
                </details>
              ) : null}

              {applyError ? (
                <p className="enrichment-diff-status enrichment-diff-status-error">{applyError}</p>
              ) : null}

              <div className="enrichment-diff-buttons">
                <button
                  type="button"
                  className="enrichment-diff-cancel"
                  onClick={onClose}
                  disabled={applyMutation.isPending}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  className="enrichment-diff-apply"
                  onClick={handleApply}
                  disabled={armedDiffs.length === 0 || applyMutation.isPending}
                >
                  {applyMutation.isPending ? 'Applying…' : 'Apply'}
                </button>
              </div>
            </>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
