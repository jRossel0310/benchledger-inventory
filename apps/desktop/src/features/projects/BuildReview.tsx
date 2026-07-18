/**
 * The Build-from-BOM review screen (Phase 4 Task 7, spec §"Reserve / build
 * from BOM"): a dry-run confirmation over `plan_build`'s `BuildPlanLine[]`.
 * Every line renders what will happen right now — `reserved_qty` consumed
 * unconditionally, `available_needed` drawn from free stock ONLY if the
 * caller checks that line's box (this is what fills `approvedAvailableLines`
 * on confirm), `checkout_qty` for reusable parts, and `missing` flagged
 * (amber) but never gating — "build what CAN be built, flag the rest" is the
 * documented policy (`build.rs`'s module doc comment), so a required line
 * that's still short does not disable Confirm.
 *
 * Confirming calls `useBuildFromBom`, which commits every line's ops as ONE
 * atomic group: an approved line that turns out short fails the WHOLE group
 * (the `part_stock` CHECK constraint is the real gate on an unclamped
 * `available_needed` — see `build.rs`), so nothing is partially applied and
 * the footer note says so.
 *
 * Mounted only while open (`ProjectDetail`'s `building` state) — the same
 * "dialog owns its own query" pattern `BomTable.tsx`'s add/edit dialogs use.
 * `usePlanBuild` fires on mount, and this component follows up with an
 * explicit `refetch()` (per `usePlanBuild`'s doc comment in
 * `hooks/projects.ts`) so a plan cached from before the dialog opened never
 * gets presented as current — stock and reservations can move fast.
 */

import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useState } from 'react';

import type { BomItemId, BuildPlanLine, ProjectId } from '../../bindings.gen';
import { useToast } from '../../components/Toast';
import { useBuildFromBom, usePlanBuild } from '../../hooks/projects';
import { errorHint, errorMessage, formatQuantity, type CommandError } from '../../lib/format';
import './BuildReview.css';

// `BuildPlanLine` carries no unit field, the same documented gap
// `BomTable.tsx` already lives with for `BomItemRecord`.
const PLAN_UNIT = 'each';

export interface BuildReviewProps {
  projectId: ProjectId;
  onClose: () => void;
}

export function BuildReview({ projectId, onClose }: BuildReviewProps) {
  const planQuery = usePlanBuild(projectId);
  const { toast } = useToast();
  const [approved, setApproved] = useState<Set<BomItemId>>(new Set());
  const [submitError, setSubmitError] = useState<CommandError | null>(null);

  // Refetch once up front rather than trusting whatever's already cached —
  // see the module doc comment. Deliberately an empty dependency array:
  // `planQuery` is a fresh object every render, and this should run once
  // when the dialog opens, not on every render.
  useEffect(() => {
    void planQuery.refetch();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const buildFromBom = useBuildFromBom({
    onDone: (error) => {
      if (error) {
        setSubmitError(error);
        toast({
          title: 'Could not build from BOM',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Build committed', kind: 'success' });
      onClose();
    },
  });

  function toggleApproved(bomItemId: BomItemId, checked: boolean) {
    setApproved((current) => {
      const next = new Set(current);
      if (checked) {
        next.add(bomItemId);
      } else {
        next.delete(bomItemId);
      }
      return next;
    });
  }

  function handleConfirm() {
    setSubmitError(null);
    buildFromBom.mutate({ projectId, approvedAvailableLines: Array.from(approved) });
  }

  const lines = planQuery.data?.lines ?? [];
  const summary = {
    total: lines.length,
    consumingReserved: lines.filter((line) => line.reserved_qty > 0).length,
    drawingAvailable: lines.filter(
      (line) => line.available_needed > 0 && approved.has(line.bom_item_id),
    ).length,
    checkouts: lines.filter((line) => line.checkout_qty > 0).length,
    unmetRequired: lines.filter((line) => line.required && line.missing > 0).length,
  };

  const pending = buildFromBom.isPending;
  const planReady = !planQuery.isPending && !planQuery.isError;

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open && !pending) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="build-review-overlay" />
        <Dialog.Content className="build-review-content">
          <Dialog.Title className="build-review-title">Build from BOM</Dialog.Title>
          <Dialog.Description className="build-review-description">
            A dry run of exactly what confirming will do right now — every line and the transactions
            it produces.
          </Dialog.Description>

          {planQuery.isPending ? (
            <p className="build-review-status">Planning the build…</p>
          ) : planQuery.isError ? (
            <p className="build-review-status build-review-status-error">
              Could not plan the build: {errorMessage(planQuery.error)}
            </p>
          ) : (
            <>
              <ul className="build-review-summary" aria-label="Build summary">
                <li>
                  {summary.total} line{summary.total === 1 ? '' : 's'}
                </li>
                <li>{summary.consumingReserved} consuming reserved</li>
                <li>{summary.drawingAvailable} drawing available (approved)</li>
                <li>
                  {summary.checkouts} checkout{summary.checkouts === 1 ? '' : 's'}
                </li>
                <li
                  className={
                    summary.unmetRequired > 0 ? 'build-review-summary-item-warning' : undefined
                  }
                >
                  {summary.unmetRequired} unmet required
                </li>
              </ul>

              <ul className="build-review-lines">
                {lines.map((line) => (
                  <BuildReviewLine
                    key={line.bom_item_id}
                    line={line}
                    approved={approved.has(line.bom_item_id)}
                    onToggle={(checked) => toggleApproved(line.bom_item_id, checked)}
                    disabled={pending}
                  />
                ))}
              </ul>
            </>
          )}

          <p className="build-review-atomic-note">
            Committed as one transaction — reversible from History.
          </p>

          {submitError ? (
            <p className="build-review-error">
              {errorHint(submitError.code) ?? submitError.message}
            </p>
          ) : null}

          <div className="build-review-buttons">
            <button
              type="button"
              className="build-review-cancel"
              onClick={onClose}
              disabled={pending}
            >
              Cancel
            </button>
            <button
              type="button"
              className="build-review-confirm"
              onClick={handleConfirm}
              disabled={pending || !planReady || lines.length === 0}
            >
              {pending ? 'Building…' : 'Confirm build'}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface BuildReviewLineProps {
  line: BuildPlanLine;
  approved: boolean;
  onToggle: (checked: boolean) => void;
  disabled: boolean;
}

/** One BOM line's dry-run detail: badges (required/optional, reusable, and
 * — only when it applies — unmet required), then every op this line will
 * produce. `available_needed`'s checkbox is the only gated control on the
 * whole screen (per the module doc comment); everything else here just
 * states what will happen, unconditionally. */
function BuildReviewLine({ line, approved, onToggle, disabled }: BuildReviewLineProps) {
  const unmetRequired = line.required && line.missing > 0;
  return (
    <li className="build-review-line">
      <div className="build-review-line-header">
        <span className="build-review-line-name">{line.part_display_name}</span>
        <span
          className={`build-review-badge ${
            line.required ? 'build-review-badge-required' : 'build-review-badge-optional'
          }`}
        >
          {line.required ? 'Required' : 'Optional'}
        </span>
        {line.is_reusable ? (
          <span className="build-review-badge build-review-badge-reusable">Reusable</span>
        ) : null}
        {unmetRequired ? (
          <span className="build-review-badge build-review-badge-unmet">Unmet required</span>
        ) : null}
      </div>

      <div className="build-review-line-ops">
        {line.reserved_qty > 0 ? (
          <span className="build-review-op">
            Consume reserved {formatQuantity(line.reserved_qty, PLAN_UNIT)}
          </span>
        ) : null}

        {line.available_needed > 0 ? (
          <label className="build-review-op build-review-op-checkbox">
            <input
              type="checkbox"
              checked={approved}
              disabled={disabled}
              onChange={(event) => onToggle(event.target.checked)}
            />
            Draw {formatQuantity(line.available_needed, PLAN_UNIT)} from available
          </label>
        ) : null}

        {line.checkout_qty > 0 ? (
          <span className="build-review-op">
            Check out {formatQuantity(line.checkout_qty, PLAN_UNIT)}
          </span>
        ) : null}

        {line.missing > 0 ? (
          <span className="build-review-op build-review-op-missing">
            Short by {formatQuantity(line.missing, PLAN_UNIT)}
          </span>
        ) : null}
      </div>

      <p className="build-review-line-preview">{line.planned_ops_preview}</p>
    </li>
  );
}
