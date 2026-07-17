/**
 * The part create/edit save sequence (spec §9). The backend has NO batched
 * "create a part with all its metadata" command, so a save is an ordered
 * series of single mutations: create (or update) the part, then set each
 * attribute, tags, each dimension, and each variant (and, per variant, each
 * supplier listing — which needs the `VariantId` `add_variant` returns, so
 * listings necessarily follow their variant).
 *
 * NON-ATOMIC, by construction (documented limitation to revisit if a batched
 * command lands): if a step after `create_part` fails, the part already
 * exists with whatever was written before the failure. `savePart` surfaces
 * that honestly rather than swallowing it — it throws a `SavePartError`
 * carrying the failed `step`, the underlying `CommandError`, and the
 * `partId` (non-null once the part itself was created/updated), so the form
 * can tell the user "the part was created but <step> failed — fix it and
 * retry the rest" instead of pretending the whole save succeeded or that
 * nothing happened.
 *
 * Framework-free (takes its mutations as async callbacks) so the ordering,
 * the variant→listing nesting, and the partial-failure contract are all
 * unit-testable without rendering the form or a real backend. `PartForm`
 * wires the callbacks to the TanStack Query mutation hooks' `mutateAsync`
 * (so cache invalidation still runs per mutation).
 */

import type {
  DimensionDraft,
  ListingDraft,
  PartDraft,
  PartId,
  PartRecord,
  VariantDraft,
  VariantRecord,
} from '../../bindings.gen';

export interface SavePartDeps {
  createPart: (draft: PartDraft) => Promise<PartRecord>;
  updatePart: (record: PartRecord) => Promise<void>;
  setAttribute: (partId: PartId, key: string, raw: string) => Promise<void>;
  setTags: (partId: PartId, tags: string[]) => Promise<void>;
  addDimension: (partId: PartId, draft: DimensionDraft) => Promise<void>;
  addVariant: (partId: PartId, draft: VariantDraft) => Promise<VariantRecord>;
  addSupplierListing: (variantId: string, draft: ListingDraft) => Promise<void>;
}

export interface SaveVariantInput {
  draft: VariantDraft;
  listings: ListingDraft[];
}

export type SavePartMode =
  { kind: 'create'; draft: PartDraft } | { kind: 'update'; record: PartRecord };

export interface SavePartInput {
  mode: SavePartMode;
  /** `(key, raw)` pairs, already filtered to entered values — passed
   * straight to `set_attribute`, which upserts (so re-setting a loaded value
   * in edit mode is safe/idempotent). */
  attributes: [string, string][];
  /** The complete tag set — `set_tags` replaces, so this is the final list,
   * not a delta. */
  tags: string[];
  /** New dimensions to append (loaded/persisted ones are excluded by the
   * caller — dimensions are append-only). */
  dimensions: DimensionDraft[];
  /** New variants to append, each with its own new listings. */
  variants: SaveVariantInput[];
}

/** A failed save step, carrying enough context for the form to message the
 * partial-write state honestly. `partId` is `null` only when `create_part`
 * itself failed (nothing was written); otherwise the part exists. */
export interface SavePartError {
  step: string;
  error: unknown;
  partId: PartId | null;
}

function isSavePartError(value: unknown): value is SavePartError {
  return typeof value === 'object' && value !== null && 'step' in value && 'partId' in value;
}

async function step<T>(label: string, partId: PartId | null, run: () => Promise<T>): Promise<T> {
  try {
    return await run();
  } catch (error) {
    // Don't re-wrap an already-structured SavePartError bubbling up from a
    // nested step (a listing failure inside a variant loop).
    if (isSavePartError(error)) throw error;
    const failure: SavePartError = { step: label, error, partId };
    throw failure;
  }
}

export async function savePart(deps: SavePartDeps, input: SavePartInput): Promise<PartId> {
  let partId: PartId;
  if (input.mode.kind === 'create') {
    const draft = input.mode.draft;
    const created = await step('create_part', null, () => deps.createPart(draft));
    partId = created.id;
  } else {
    const record = input.mode.record;
    await step('update_part', record.id, () => deps.updatePart(record));
    partId = record.id;
  }

  for (const [key, raw] of input.attributes) {
    await step(`set_attribute:${key}`, partId, () => deps.setAttribute(partId, key, raw));
  }

  await step('set_tags', partId, () => deps.setTags(partId, input.tags));

  for (const dimension of input.dimensions) {
    await step(`add_dimension:${dimension.name}`, partId, () =>
      deps.addDimension(partId, dimension),
    );
  }

  for (const variant of input.variants) {
    const created = await step(
      `add_variant:${variant.draft.mpn || variant.draft.manufacturer}`,
      partId,
      () => deps.addVariant(partId, variant.draft),
    );
    for (const listing of variant.listings) {
      await step(`add_supplier_listing:${listing.supplier}`, partId, () =>
        deps.addSupplierListing(created.id, listing),
      );
    }
  }

  return partId;
}
