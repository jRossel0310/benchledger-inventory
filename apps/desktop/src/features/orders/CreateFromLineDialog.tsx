/**
 * Complete a `create_new` import line's draft (Phase 5d Task 4, spec §10):
 * the modal `ReviewLineTable` opens for a line whose current decision is
 * `create_new` — whether that decision came from the backend's own
 * `proposed` default (`lineDecisions.ts`'s `placeholderPartDraft`, always
 * flagged incomplete — `category_id: ''`) or from `LineActionEditor`'s
 * "Create new part" action (the same placeholder). Saving replaces the
 * line's decision in `ImportReview.tsx`'s `decisions` map with the completed
 * `{type: 'create_new', draft, variant, listing}` — once `category_id` is
 * non-empty, `isCreateNewIncomplete` clears and the row's "Draft incomplete"
 * flag disappears. Cancel discards every edit and leaves the map untouched
 * (the caller only ever calls `onSave` on submit).
 *
 * Reuses `PartForm.tsx`'s primitives rather than forking a second part form:
 * `TextField`/`SelectField`/`NumberField` (`components/Field.tsx`) for every
 * input, and `useCategories` for the category picker — the same source
 * `PartForm` uses. This dialog is deliberately narrower than `PartForm`,
 * though: no attributes/dimensions/duplicate-detection/tags, because an
 * import line's draft only ever needs the fields spec §10 calls out (basic
 * part info + the one variant/listing the line itself carries) — a part
 * created here can always be finished off later from its own detail page,
 * same as any other part.
 *
 * The bin field mirrors `RowActions.tsx`'s `AssignBinDialog` bin-occupancy
 * check (`useBins`, case-insensitive label match) but as an inline WARNING
 * rather than a second confirm step: `AssignBinDialog` blocks-then-confirms
 * because assigning a bin is its *entire* job, but here it's one field among
 * many in a bigger save action, so an occupied-bin message renders under the
 * field and Save still proceeds immediately — warn-not-block, never a second
 * screen to click through.
 *
 * Fields not exposed here (`VariantDraft.package`/`datasheet_url`/
 * `product_url`/`lifecycle`/`notes`, `ListingDraft.product_url`/
 * `typical_order`) stay exactly as `lineDecisions.ts`'s `prefillVariant`/
 * `prefillListing` set them (null/empty — never fabricated) and pass through
 * unedited on save; a documented gap, the same shape `PartForm.tsx`'s module
 * doc comment already accepts for its own two omitted fields.
 */

import * as Dialog from '@radix-ui/react-dialog';
import { useState } from 'react';

import type {
  ImportReviewLine,
  LineDecision,
  ListingDraft,
  PartDraft,
  QuantityUnit,
  VariantDraft,
} from '../../bindings.gen';
import {
  NumberField,
  SelectField,
  TextField,
  type SelectFieldOption,
} from '../../components/Field';
import { useBins, useCategories } from '../../hooks/inventory';
import './CreateFromLineDialog.css';

const USAGE_OPTIONS: SelectFieldOption[] = [
  { value: 'usually_consumed', label: 'Usually consumed' },
  { value: 'usually_checked_out', label: 'Usually checked out' },
  { value: 'ask', label: 'Ask each time' },
];

const UNIT_OPTIONS: SelectFieldOption[] = [
  { value: 'each', label: 'Each' },
  { value: 'meter', label: 'Meter' },
  { value: 'foot', label: 'Foot' },
];

export interface CreateFromLineDialogProps {
  line: ImportReviewLine;
  /** The line's current `create_new` decision — whatever the placeholder or
   * a prior visit to this dialog left it as. Every field prefills from this,
   * never from `line` directly, so re-opening the dialog on a
   * previously-completed draft shows what the user actually entered rather
   * than resetting to the raw line data. */
  decision: Extract<LineDecision, { type: 'create_new' }>;
  onSave: (decision: LineDecision) => void;
  onCancel: () => void;
}

export function CreateFromLineDialog({
  line,
  decision,
  onSave,
  onCancel,
}: CreateFromLineDialogProps) {
  const categoriesQuery = useCategories();
  const binsQuery = useBins();

  const [displayName, setDisplayName] = useState(decision.draft.display_name);
  const [categoryId, setCategoryId] = useState(decision.draft.category_id);
  const [description, setDescription] = useState(decision.draft.description);
  const [quantityUnit, setQuantityUnit] = useState<QuantityUnit>(decision.draft.quantity_unit);
  const [usageBehavior, setUsageBehavior] = useState(decision.draft.usage_behavior);
  const [bin, setBin] = useState(decision.draft.bin_label ?? '');
  const [lowStockThreshold, setLowStockThreshold] = useState<number | ''>(
    decision.draft.low_stock_threshold === null ? '' : decision.draft.low_stock_threshold / 1000,
  );
  const [publicNotes, setPublicNotes] = useState(decision.draft.public_notes);
  const [privateNotes, setPrivateNotes] = useState(decision.draft.private_notes);

  const [manufacturer, setManufacturer] = useState(decision.variant.manufacturer);
  const [mpn, setMpn] = useState(decision.variant.mpn);

  const [supplierSku, setSupplierSku] = useState(decision.listing.supplier_sku);
  const [unitPrice, setUnitPrice] = useState<number | ''>(
    decision.listing.last_unit_price_micros === null
      ? ''
      : decision.listing.last_unit_price_micros / 1_000_000,
  );
  const [packaging, setPackaging] = useState(decision.listing.packaging ?? '');

  const categoryOptions: SelectFieldOption[] = [
    { value: '', label: 'Select a category…' },
    ...(categoriesQuery.data ?? []).map((c) => ({ value: c.id, label: c.name })),
  ];

  const trimmedBin = bin.trim();
  const occupant = (binsQuery.data ?? []).find(
    (b) => b.bin_label !== null && b.bin_label.toLowerCase() === trimmedBin.toLowerCase(),
  );
  const binWarning =
    trimmedBin !== '' && occupant && occupant.part_count > 0
      ? `Bin ${trimmedBin} already holds ${occupant.part_count} part${
          occupant.part_count === 1 ? '' : 's'
        } — you can still assign it.`
      : null;

  const canSave = displayName.trim() !== '' && categoryId !== '';

  function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    if (!canSave) return;

    const draft: PartDraft = {
      display_name: displayName.trim(),
      category_id: categoryId,
      description: description.trim(),
      bin_label: trimmedBin === '' ? null : trimmedBin,
      usage_behavior: usageBehavior,
      quantity_unit: quantityUnit,
      low_stock_threshold:
        lowStockThreshold === '' ? null : Math.round(Number(lowStockThreshold) * 1000),
      public_notes: publicNotes.trim(),
      private_notes: privateNotes.trim(),
    };
    const variant: VariantDraft = {
      ...decision.variant,
      manufacturer: manufacturer.trim(),
      mpn: mpn.trim(),
    };
    const listing: ListingDraft = {
      ...decision.listing,
      supplier_sku: supplierSku.trim(),
      last_unit_price_micros: unitPrice === '' ? null : Math.round(Number(unitPrice) * 1_000_000),
      packaging: packaging.trim() === '' ? null : packaging.trim(),
    };

    onSave({ type: 'create_new', draft, variant, listing });
  }

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="create-from-line-overlay" />
        <Dialog.Content className="create-from-line-content">
          <Dialog.Title className="create-from-line-title">Create new part</Dialog.Title>
          <Dialog.Description className="create-from-line-description">
            From line {line.line_number ?? ''}: {line.description ?? line.mpn ?? line.supplier_sku}
          </Dialog.Description>

          <form className="create-from-line-form" onSubmit={handleSubmit}>
            <fieldset className="create-from-line-section">
              <legend className="create-from-line-legend">Part</legend>
              <TextField
                label="Display name"
                value={displayName}
                onChange={setDisplayName}
                required
                autoFocus
              />
              <SelectField
                label="Category"
                value={categoryId}
                onChange={setCategoryId}
                options={categoryOptions}
                required
              />
              <TextField label="Description" value={description} onChange={setDescription} />
              <div className="create-from-line-grid">
                <SelectField
                  label="Quantity unit"
                  value={quantityUnit}
                  onChange={(v) => setQuantityUnit(v as QuantityUnit)}
                  options={UNIT_OPTIONS}
                />
                <SelectField
                  label="Usage behavior"
                  value={usageBehavior}
                  onChange={setUsageBehavior}
                  options={USAGE_OPTIONS}
                />
              </div>
              <TextField
                label="Bin"
                value={bin}
                onChange={setBin}
                placeholder="e.g. A12 — leave blank for unassigned"
                hint={binWarning ?? undefined}
              />
              <NumberField
                label="Low-stock threshold"
                value={lowStockThreshold}
                onChange={setLowStockThreshold}
                min={0}
              />
              <div className="create-from-line-grid">
                <TextField label="Public notes" value={publicNotes} onChange={setPublicNotes} />
                <TextField label="Private notes" value={privateNotes} onChange={setPrivateNotes} />
              </div>
            </fieldset>

            <fieldset className="create-from-line-section">
              <legend className="create-from-line-legend">Manufacturer variant</legend>
              <div className="create-from-line-grid">
                <TextField label="Manufacturer" value={manufacturer} onChange={setManufacturer} />
                <TextField label="MPN" value={mpn} onChange={setMpn} />
              </div>
            </fieldset>

            <fieldset className="create-from-line-section">
              <legend className="create-from-line-legend">Supplier listing</legend>
              <div className="create-from-line-grid">
                <TextField label="Supplier SKU" value={supplierSku} onChange={setSupplierSku} />
                <NumberField
                  label="Unit price"
                  value={unitPrice}
                  onChange={setUnitPrice}
                  min={0}
                  step={0.01}
                />
              </div>
              <TextField label="Packaging" value={packaging} onChange={setPackaging} />
            </fieldset>

            <div className="create-from-line-buttons">
              <button type="button" className="create-from-line-cancel" onClick={onCancel}>
                Cancel
              </button>
              <button type="submit" className="create-from-line-submit" disabled={!canSave}>
                Save draft
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
