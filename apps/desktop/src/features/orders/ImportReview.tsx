/**
 * The Match -> Review screen (Phase 5d Task 3, spec §10): `useImportReview`'s
 * summary header (order metadata, financial block, line counts, backorder
 * count, and a prominent-but-non-blocking duplicate-import warning), the
 * per-line table (`ReviewLineTable`), and a disabled Commit placeholder bar
 * — Task 4 replaces the bar with the real commit/reverse flow.
 *
 * Owns the `decisions: Map<ImportLineId, LineDecision>` state the plan's
 * Task 3 interface calls for: initialized once per import from each line's
 * backend `proposed` (`lineDecisions.ts`'s `decisionFromProposed`), then
 * updated per-line via `updateDecision` as `ReviewLineTable`/
 * `LineActionEditor` report changes — never rebuilt wholesale on a change,
 * so editing one line's decision can never clobber another's. Re-
 * initializes only when the import actually changes (tracked via a ref, not
 * every `useImportReview` refetch) so a background refetch (e.g. after
 * Task 4's commit invalidates this query) doesn't silently discard in-
 * progress edits out from under the user.
 */

import { Link } from '@tanstack/react-router';
import { useEffect, useRef, useState } from 'react';

import type { ImportId, ImportLineId, ImportRecord, LineDecision } from '../../bindings.gen';
import { useImportReview } from '../../hooks/imports';
import { errorMessage, formatPrice } from '../../lib/format';
import { ImportStatusChip } from './ImportStatusChip';
import { orderNumberFor } from './OrdersList';
import { ReviewLineTable } from './ReviewLineTable';
import { decisionFromProposed, type LineDecisionContext } from './lineDecisions';
import './ImportReview.css';

export interface ImportReviewProps {
  importId: ImportId;
}

export function ImportReview({ importId }: ImportReviewProps) {
  const reviewQuery = useImportReview(importId);
  const [decisions, setDecisions] = useState<Map<ImportLineId, LineDecision>>(new Map());
  const [decisionWarnings, setDecisionWarnings] = useState<Map<ImportLineId, string>>(new Map());
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
      />

      <footer className="import-review-commit-bar">
        <p className="import-review-commit-bar-note">
          Commit lands in the next task — this bar is a placeholder until then. Nothing on this
          screen touches inventory yet.
        </p>
        <button type="button" className="import-review-commit-button" disabled>
          Commit (coming soon)
        </button>
      </footer>
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
