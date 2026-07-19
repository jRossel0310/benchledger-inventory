/**
 * The Match -> Review -> Confirm screen (Phase 5d Task 3, extended in Task 4
 * with the real commit/reverse flow, spec §10): `useImportReview`'s summary
 * header (order metadata, financial block, line counts, backorder count, and
 * a prominent-but-non-blocking duplicate-import warning), the per-line table
 * (`ReviewLineTable`), and the commit bar.
 *
 * Owns the `decisions: Map<ImportLineId, LineDecision>` state the plan's
 * Task 3 interface calls for: initialized once per import from each line's
 * backend `proposed` (`lineDecisions.ts`'s `decisionFromProposed`), then
 * updated per-line via `updateDecision` as `ReviewLineTable`/
 * `LineActionEditor`/`CreateFromLineDialog` report changes — never rebuilt
 * wholesale on a change, so editing one line's decision can never clobber
 * another's. Re-initializes only when the import actually changes (tracked
 * via a ref, not every `useImportReview` refetch) so a background refetch
 * (e.g. after Task 4's commit invalidates this query) doesn't silently
 * discard in-progress edits out from under the user.
 *
 * Commit bar (Task 4): `summarizeDecisions(lines, decisions)` — the SAME
 * `lineDecisions.ts` helper, over the SAME `decisions` map this component
 * sends to `useCommitImport` — drives every number the bar displays (N
 * receives / M new parts / K new variants) AND the Commit button's disabled
 * state (`hasIncompleteDraft`), so the count shown can never drift from what
 * commit actually does; there is exactly one source, never two independently
 * maintained tallies. The payload itself is `Array.from(decisions.entries())`
 * — `commit_import`'s wire shape is `[ImportLineId, LineDecision][]`, which a
 * `Map` doesn't serialize to on its own.
 *
 * Once `record.status` is no longer `parsed` (committed or reversed),
 * `disabled` freezes `ReviewLineTable`'s editors — a decision against an
 * already-committed import can't actually apply, so nothing here pretends
 * it's still editable. `committed` additionally reveals "Reverse import"
 * (confirmed via a dialog mirroring `GroupRow.tsx`'s reverse-group
 * confirmation): a full-group undo, parts created by the commit staying on
 * file at zero stock (5b's documented behavior, never deleted).
 */

import { Link } from '@tanstack/react-router';
import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useRef, useState } from 'react';

import type { ImportId, ImportLineId, ImportRecord, LineDecision } from '../../bindings.gen';
import { useToast } from '../../components/Toast';
import { useCommitImport, useImportReview, useReverseImport } from '../../hooks/imports';
import { errorHint, errorMessage, formatPrice } from '../../lib/format';
import { ImportStatusChip } from './ImportStatusChip';
import { orderNumberFor } from './OrdersList';
import { ReviewLineTable } from './ReviewLineTable';
import {
  decisionFromProposed,
  summarizeDecisions,
  type LineDecisionContext,
} from './lineDecisions';
import './ImportReview.css';

/** The fixed reversal note every "Reverse import" confirmation sends — the
 * same "one fixed, honest string, no free-text box" convention
 * `History.tsx`'s `REVERSE_NOTE` uses for group/transaction reversals. */
const REVERSE_NOTE = 'Reversed from import review';

export interface ImportReviewProps {
  importId: ImportId;
}

export function ImportReview({ importId }: ImportReviewProps) {
  const reviewQuery = useImportReview(importId);
  const { toast } = useToast();
  const [decisions, setDecisions] = useState<Map<ImportLineId, LineDecision>>(new Map());
  const [decisionWarnings, setDecisionWarnings] = useState<Map<ImportLineId, string>>(new Map());
  const [confirmingReverse, setConfirmingReverse] = useState(false);
  const initializedFor = useRef<ImportId | null>(null);

  useEffect(() => {
    const review = reviewQuery.data;
    if (!review) return;
    if (initializedFor.current === review.import.id) return;
    initializedFor.current = review.import.id;

    const context: LineDecisionContext = {
      currency: review.import.currency,
      orderDate: review.import.order_date,
    };
    const nextDecisions = new Map<ImportLineId, LineDecision>();
    const nextWarnings = new Map<ImportLineId, string>();
    for (const line of review.lines) {
      const { decision, warning } = decisionFromProposed(line, context);
      nextDecisions.set(line.line_id, decision);
      if (warning) nextWarnings.set(line.line_id, warning);
    }
    setDecisions(nextDecisions);
    setDecisionWarnings(nextWarnings);
  }, [reviewQuery.data]);

  const commitImport = useCommitImport({
    onDone: (error, data) => {
      if (error) {
        toast({
          title: 'Could not commit this import',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      if (data) {
        const receives = data.transactions.length;
        toast({ title: `Received ${receives} line${receives === 1 ? '' : 's'}`, kind: 'success' });
      }
    },
  });

  const reverseImport = useReverseImport({
    onDone: (error) => {
      setConfirmingReverse(false);
      if (error) {
        toast({
          title: 'Could not reverse this import',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Import reversed', kind: 'success' });
    },
  });

  function updateDecision(lineId: ImportLineId, decision: LineDecision) {
    setDecisions((current) => {
      const next = new Map(current);
      next.set(lineId, decision);
      return next;
    });
    // A line whose decision has since been set explicitly no longer needs
    // its unmappable-proposal fallback warning — this only ever removes
    // this one line's entry, never touches any other line's.
    setDecisionWarnings((current) => {
      if (!current.has(lineId)) return current;
      const next = new Map(current);
      next.delete(lineId);
      return next;
    });
  }

  if (reviewQuery.isPending) {
    return <p className="import-review-status">Loading import…</p>;
  }
  if (reviewQuery.isError) {
    return (
      <p className="import-review-status import-review-status-error">
        Could not load this import: {errorMessage(reviewQuery.error)}
      </p>
    );
  }

  const review = reviewQuery.data;
  const record = review.import;
  const lines = review.lines;
  const duplicates = review.duplicate_of;
  const backorderedCount = lines.filter((line) => (line.backordered_milli ?? 0) > 0).length;
  const context: LineDecisionContext = { currency: record.currency, orderDate: record.order_date };

  const isParsed = record.status === 'parsed';
  const isCommitted = record.status === 'committed';
  const summary = summarizeDecisions(lines, decisions);

  function handleCommit() {
    commitImport.mutate({ importId, decisions: Array.from(decisions.entries()) });
  }

  return (
    <section className="import-review">
      <header className="import-review-header">
        <p className="import-review-eyebrow">Orders</p>
        <h1 className="import-review-title">{record.supplier} order</h1>
        <p className="import-review-subtitle">
          <span className="import-review-subtitle-order">{orderNumberFor(record)}</span>
          {record.order_date ? (
            <span className="import-review-subtitle-date">{record.order_date}</span>
          ) : null}
          <ImportStatusChip status={record.status} />
        </p>
      </header>

      {duplicates.length > 0 ? <DuplicateWarning duplicates={duplicates} /> : null}

      <div className="import-review-summary">
        <dl className="import-review-financials">
          <FinancialRow
            label="Subtotal"
            micros={record.subtotal_micros}
            currency={record.currency}
          />
          <FinancialRow
            label="Shipping"
            micros={record.shipping_micros}
            currency={record.currency}
          />
          <FinancialRow label="Tax" micros={record.tax_micros} currency={record.currency} />
          <FinancialRow label="Tariff" micros={record.tariff_micros} currency={record.currency} />
          <div className="import-review-financials-row import-review-financials-total">
            <dt>Total</dt>
            <dd>{formatPrice(record.total_micros, record.currency)}</dd>
          </div>
        </dl>
        <ul className="import-review-counts" aria-label="Line counts">
          <li>
            {lines.length} line{lines.length === 1 ? '' : 's'}
          </li>
          <li>{review.total_receive_lines} will receive stock</li>
          {backorderedCount > 0 ? (
            <li className="import-review-counts-warning">
              {backorderedCount} line{backorderedCount === 1 ? '' : 's'} backordered
            </li>
          ) : null}
        </ul>
      </div>

      <ReviewLineTable
        lines={lines}
        decisions={decisions}
        decisionWarnings={decisionWarnings}
        onChangeDecision={updateDecision}
        context={context}
        disabled={!isParsed}
      />

      <footer className="import-review-commit-bar">
        {isParsed ? (
          <>
            <ul className="import-review-commit-summary" aria-label="What committing will do">
              <li>
                {summary.receives} receive{summary.receives === 1 ? '' : 's'}
              </li>
              {summary.newParts > 0 ? (
                <li>
                  {summary.newParts} new part{summary.newParts === 1 ? '' : 's'}
                </li>
              ) : null}
              {summary.newVariants > 0 ? (
                <li>
                  {summary.newVariants} new variant{summary.newVariants === 1 ? '' : 's'}
                </li>
              ) : null}
              {summary.skipped > 0 ? <li>{summary.skipped} skipped</li> : null}
              {summary.nonInventoryLines > 0 ? (
                <li>{summary.nonInventoryLines} non-inventory</li>
              ) : null}
            </ul>
            <p className="import-review-commit-bar-note">
              Committed as one transaction group — reversible from History.
            </p>
            <button
              type="button"
              className="import-review-commit-button"
              disabled={summary.hasIncompleteDraft || commitImport.isPending}
              onClick={handleCommit}
              title={
                summary.hasIncompleteDraft
                  ? 'Complete every "Create new part" draft before committing.'
                  : undefined
              }
            >
              {commitImport.isPending ? 'Committing…' : 'Commit import'}
            </button>
          </>
        ) : isCommitted ? (
          <>
            <p className="import-review-commit-bar-note">
              This import was committed — inventory has been received. Reversing undoes the whole
              group as one transaction; any parts it created stay on file at zero stock.
            </p>
            <button
              type="button"
              className="import-review-reverse-button"
              onClick={() => setConfirmingReverse(true)}
              disabled={reverseImport.isPending}
            >
              {reverseImport.isPending ? 'Reversing…' : 'Reverse import'}
            </button>
          </>
        ) : (
          <p className="import-review-commit-bar-note">
            This import was reversed — inventory it received has been undone.
          </p>
        )}
      </footer>

      {confirmingReverse ? (
        <Dialog.Root open onOpenChange={(open) => !open && setConfirmingReverse(false)}>
          <Dialog.Portal>
            <Dialog.Overlay className="import-review-dialog-overlay" />
            <Dialog.Content className="import-review-dialog-content">
              <Dialog.Title className="import-review-dialog-title">
                Reverse this import?
              </Dialog.Title>
              <Dialog.Description className="import-review-dialog-description">
                This undoes the entire commit as one transaction group — every receive it created
                moves back out. Parts this import created stay on file at zero stock; they are never
                deleted.
              </Dialog.Description>
              <div className="import-review-dialog-buttons">
                <button
                  type="button"
                  className="import-review-dialog-cancel"
                  onClick={() => setConfirmingReverse(false)}
                  disabled={reverseImport.isPending}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  className="import-review-dialog-submit"
                  onClick={() => reverseImport.mutate({ importId, note: REVERSE_NOTE })}
                  disabled={reverseImport.isPending}
                >
                  {reverseImport.isPending ? 'Reversing…' : 'Reverse import'}
                </button>
              </div>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      ) : null}
    </section>
  );
}

interface DuplicateWarningProps {
  duplicates: ImportRecord[];
}

/** A prominent but non-blocking warning: `duplicate_of` non-empty means
 * this import looks like an order/file already on file, but the backend
 * never refuses to let the user proceed (spec §10, warn-not-block) — so
 * this renders as an alert with links to the prior imports, not a gate. */
function DuplicateWarning({ duplicates }: DuplicateWarningProps) {
  return (
    <div className="import-review-duplicate-warning" role="alert">
      <p className="import-review-duplicate-warning-title">
        Possible duplicate — this looks like{' '}
        {duplicates.length === 1
          ? 'an import already on file'
          : `${duplicates.length} imports already on file`}
      </p>
      <ul className="import-review-duplicate-warning-list">
        {duplicates.map((dup) => (
          <li key={dup.id}>
            <Link to="/orders/$importId" params={{ importId: dup.id }}>
              {orderNumberFor(dup)} — {dup.created_at}
            </Link>
          </li>
        ))}
      </ul>
      <p className="import-review-duplicate-warning-note">
        You can still review and commit this import — this is only a warning.
      </p>
    </div>
  );
}

interface FinancialRowProps {
  label: string;
  micros: number | null;
  currency: string;
}

function FinancialRow({ label, micros, currency }: FinancialRowProps) {
  return (
    <div className="import-review-financials-row">
      <dt>{label}</dt>
      <dd>{formatPrice(micros, currency)}</dd>
    </div>
  );
}
