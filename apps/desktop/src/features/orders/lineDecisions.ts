/**
 * Pure `LineDecision` construction for the import review screen (Phase 5d
 * Task 3, `docs/superpowers/plans/2026-07-19-phase-5d-import-enrichment-ui.md`):
 * mapping the backend's derived `ProposedAction` (`ImportReviewLine.
 * proposed`) to the `decisions: Map<ImportLineId, LineDecision>` state
 * `ImportReview.tsx` owns, plus the prefilled `VariantDraft`/`ListingDraft`/
 * `PartDraft` shapes `LineActionEditor`'s "add as variant"/"create new part"
 * actions build from a line's known fields. Kept separate from the
 * components so the mapping — and its unmappable-proposal fallback — is
 * unit-testable without mounting React.
 */

import type {
  ImportLineId,
  ImportReviewLine,
  LineDecision,
  ListingDraft,
  PartDraft,
  ProposedAction,
  VariantDraft,
} from '../../bindings.gen';

/** Context every prefill needs beyond the line itself — the import's
 * currency/order date, neither carried on `ImportReviewLine`. */
export interface LineDecisionContext {
  currency: string;
  orderDate: string | null;
}

/** A brand-new manufacturer variant prefilled from the line's known
 * identity fields — shared by "add as variant to…" and "create new part"
 * (both add a variant, either to a new or an existing part). Fields the
 * line never carries (package/datasheet/product URL/lifecycle) stay
 * `null`/empty rather than fabricated — the same no-fabrication rule 5a's
 * description parser follows. */
export function prefillVariant(line: ImportReviewLine): VariantDraft {
  return {
    manufacturer: line.manufacturer ?? '',
    mpn: line.mpn ?? '',
    description: line.description ?? '',
    package: null,
    datasheet_url: null,
    product_url: null,
    lifecycle: null,
    notes: '',
  };
}

/** A new DigiKey supplier listing prefilled from the line — shared the same
 * way `prefillVariant` is. */
export function prefillListing(line: ImportReviewLine, context: LineDecisionContext): ListingDraft {
  return {
    supplier: 'DigiKey',
    supplier_sku: line.supplier_sku ?? '',
    product_url: null,
    packaging: null,
    typical_order: null,
    last_unit_price_micros: line.unit_price_micros,
    currency: context.currency,
    last_purchase_date: context.orderDate,
  };
}

/** A placeholder `PartDraft` for "create new part" — deliberately
 * incomplete (`category_id: ''`, no category chosen yet): Task 4's
 * create-from-line dialog is what actually completes this before commit.
 * `isCreateNewIncomplete` below is how the UI flags a row that still needs
 * that dialog. */
export function placeholderPartDraft(line: ImportReviewLine): PartDraft {
  return {
    display_name: line.description ?? line.mpn ?? line.supplier_sku ?? 'New part',
    category_id: '',
    description: line.description ?? '',
    bin_label: null,
    usage_behavior: 'usually_consumed',
    quantity_unit: 'each',
    low_stock_threshold: null,
    public_notes: '',
    private_notes: '',
  };
}

/** True for a `create_new` decision whose draft still needs Task 4's
 * dialog (no category chosen yet) — never true for any other decision
 * type. */
export function isCreateNewIncomplete(decision: LineDecision): boolean {
  return decision.type === 'create_new' && decision.draft.category_id === '';
}

/** The commit bar's "what will happen" counts (Task 4) — deliberately a pure
 * function over the SAME `decisions` map `ImportReview.tsx` sends to
 * `useCommitImport` (`Array.from(decisions.entries())`), so the numbers the
 * commit bar displays and the payload the backend receives can never drift
 * apart: both are read from one Map, never two independently-maintained
 * counters.
 *
 *  - `receives` — part lines whose decision actually creates a receive
 *    (`add_stock`/`add_as_variant`/`create_new`, `skip` excluded) AND whose
 *    `receive_qty_milli` is a positive shipped quantity; a matched/created
 *    part line shipped `0` (or `null`, nothing shipped yet) contributes no
 *    receive even though its decision isn't `skip` — spec's "zero-shipped
 *    line excluded from receives".
 *  - `newParts`/`newVariants` — `create_new`/`add_as_variant` decision counts.
 *  - `skipped` — part lines explicitly (or defaulted-to) `skip`.
 *  - `nonInventoryLines` — every non-`part` line (fee/tariff/no_charge/
 *    unknown); these never get a `decisions` entry (`ReviewLineTable` never
 *    renders an editor for them), so they're counted straight off `lines`.
 *  - `hasIncompleteDraft` — true while ANY part line's decision is a
 *    `create_new` whose draft hasn't been through Task 4's dialog
 *    (`isCreateNewIncomplete`) — the one condition that disables Commit;
 *    every other decision (including the untouched defaults `proposed`
 *    seeded) is a valid, ready-to-send decision.
 */
export interface DecisionSummary {
  receives: number;
  newParts: number;
  newVariants: number;
  skipped: number;
  nonInventoryLines: number;
  hasIncompleteDraft: boolean;
}

export function summarizeDecisions(
  lines: ImportReviewLine[],
  decisions: Map<ImportLineId, LineDecision>,
): DecisionSummary {
  let receives = 0;
  let newParts = 0;
  let newVariants = 0;
  let skipped = 0;
  let hasIncompleteDraft = false;

  for (const line of lines) {
    if (line.kind !== 'part') continue;
    const decision = decisions.get(line.line_id);
    if (!decision) continue;

    if (decision.type === 'skip') {
      skipped += 1;
      continue;
    }
    if (decision.type === 'create_new') {
      newParts += 1;
      if (isCreateNewIncomplete(decision)) hasIncompleteDraft = true;
    } else if (decision.type === 'add_as_variant') {
      newVariants += 1;
    }
    if ((line.receive_qty_milli ?? 0) > 0) receives += 1;
  }

  const nonInventoryLines = lines.filter((line) => line.kind !== 'part').length;

  return { receives, newParts, newVariants, skipped, nonInventoryLines, hasIncompleteDraft };
}

export interface DecisionInit {
  decision: LineDecision;
  /** A human-readable reason this line's decision needed the unmappable-
   * proposal fallback rather than a clean mapping from `proposed` — `null`
   * for every clean mapping (today, that's all four known `ProposedAction`
   * variants). Surfaced as an inline row warning by `ReviewLineTable`. */
  warning: string | null;
}

/** The default `LineDecision` for one review line, derived from the
 * backend's `proposed` field. Every known `ProposedAction` variant maps
 * cleanly:
 *
 *  - `add_stock_to_existing` -> `add_stock` against the same part.
 *  - `create_new` -> `create_new` with a placeholder draft flagged
 *    incomplete (see `isCreateNewIncomplete`).
 *  - `non_inventory` (fee/tariff/no_charge) and `ignore` (unclassified) ->
 *    `skip`, exactly matching their "do nothing" intent: `LineDecision` has
 *    no non-inventory/ignore variant of its own because skip already means
 *    the same thing (no part, no variant, no listing, no receive, no price
 *    history, no alias).
 *
 * Any *other* `proposed.type` this UI doesn't recognize (a future backend
 * addition this version predates) defaults to `skip` with a visible
 * warning instead of guessing — never silently mis-acts on an unrecognized
 * proposal. */
export function decisionFromProposed(
  line: ImportReviewLine,
  context: LineDecisionContext,
): DecisionInit {
  const proposed: ProposedAction = line.proposed;
  switch (proposed.type) {
    case 'add_stock_to_existing':
      return { decision: { type: 'add_stock', part_id: proposed.part_id }, warning: null };
    case 'create_new':
      return {
        decision: {
          type: 'create_new',
          draft: placeholderPartDraft(line),
          variant: prefillVariant(line),
          listing: prefillListing(line, context),
        },
        warning: null,
      };
    case 'non_inventory':
    case 'ignore':
      return { decision: { type: 'skip' }, warning: null };
    default: {
      // Exhaustive today (every known `ProposedAction` variant is handled
      // above), but a future backend addition this version predates would
      // otherwise silently fall through — defaulting to `skip` with a
      // visible warning is the safe failure mode, never a guess. The cast
      // is only to read `.type` off a value TS has narrowed to `never`.
      const unrecognized = proposed as unknown as { type: string };
      return {
        decision: { type: 'skip' },
        warning: `Unrecognized proposed action "${unrecognized.type}" — defaulted to skip.`,
      };
    }
  }
}
