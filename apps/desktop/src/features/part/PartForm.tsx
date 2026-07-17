/**
 * The category-adaptive part create/edit form (Phase 3 Task 6, spec §9). One
 * component serves both `/inventory/new` (create) and
 * `/inventory/$partId/edit` (edit) — a route, not a modal, because the form
 * is large (basic info + per-category attributes + dimensions + variants +
 * listings) and deserves its own scrollable surface.
 *
 * Sections, top to bottom:
 *  - **Basic info** — display name (+ a one-click "Suggest" fill derived from
 *    the entered identity attributes, `suggestedName`), category, description,
 *    tags, bin, usage behavior, low-stock threshold, public/private notes.
 *  - **Category attributes** (`AttributeFields`) — one typed field per
 *    `category_attribute_defs` row, swapped live when the category changes.
 *  - **Dimensions** (`DimensionFields`) and **Manufacturer variants +
 *    supplier listings** (`VariantsEditor`).
 *  - **Duplicate detection** (`DuplicatePanel`) — a debounced `find_matches`
 *    fires from the identity fields as they're entered.
 *
 * Save is the non-atomic sequence in `savePart` (see its doc comment): no
 * batched command exists, so it's create/update → per-attribute → tags →
 * dimensions → variants+listings, and a mid-sequence failure leaves the part
 * partially written. This form surfaces exactly that — on a `SavePartError`
 * with a `partId`, the part exists; the error banner says so and the user can
 * fix the offending field and save again (the upsert-y steps re-run
 * harmlessly; already-persisted dimensions/variants are marked read-only so
 * they're never double-added).
 *
 * Two documented gaps where no command exists (never faked): the "preferred
 * reorder quantity" basic-info field the brief lists has no column in
 * `PartDraft`/`PartRecord` (the DB column exists but no command writes it),
 * and **CAD/doc links** (`part_cad_links`) have no command yet — both are
 * omitted rather than collected-and-silently-dropped. Per-variant datasheet
 * and product URLs (which DO have a home on `VariantDraft`) cover the most
 * common "link to the datasheet" need in the meantime.
 */

import { useNavigate } from '@tanstack/react-router';
import { useEffect, useMemo, useRef, useState } from 'react';

import type { PartDraft, PartId, PartRecord, QuantityUnit } from '../../bindings.gen';
import {
  NumberField,
  SelectField,
  TextField,
  type SelectFieldOption,
} from '../../components/Field';
import { useToast } from '../../components/Toast';
import {
  useAddDimension,
  useAddSupplierListing,
  useAddVariant,
  useAttributes,
  useCategories,
  useCategoryAttributeDefs,
  useCreatePart,
  useDimensions,
  usePart,
  useSetAttribute,
  useSetTags,
  useTags,
  useUpdatePart,
  useVariants,
} from '../../hooks/inventory';
import { errorHint, errorMessage } from '../../lib/format';
import { AttributeFields } from './AttributeFields';
import { buildMatchCandidate } from './buildMatchCandidate';
import {
  DimensionFields,
  isDimensionEntryFilled,
  toDimensionDraft,
  type DimensionEntry,
} from './DimensionFields';
import { DuplicatePanel } from './DuplicatePanel';
import { savePart, type SavePartError, type SaveVariantInput } from './savePart';
import { suggestedName } from './suggestedName';
import { TagInput } from './TagInput';
import {
  isListingEntryFilled,
  isVariantEntryFilled,
  toListingDraft,
  toVariantDraft,
  VariantsEditor,
  type VariantEntry,
} from './VariantsEditor';
import './PartForm.css';

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

export interface PartFormProps {
  /** Present in edit mode (`/inventory/$partId/edit`); absent for create. */
  partId?: PartId;
}

export function PartForm({ partId }: PartFormProps) {
  const isEdit = partId !== undefined;
  const navigate = useNavigate();
  const { toast } = useToast();

  const categoriesQuery = useCategories();

  const [displayName, setDisplayName] = useState('');
  const [categoryId, setCategoryId] = useState('');
  const [description, setDescription] = useState('');
  const [bin, setBin] = useState('');
  const [usageBehavior, setUsageBehavior] = useState('usually_consumed');
  const [quantityUnit, setQuantityUnit] = useState<QuantityUnit>('each');
  const [lowStockThreshold, setLowStockThreshold] = useState<number | ''>('');
  const [publicNotes, setPublicNotes] = useState('');
  const [privateNotes, setPrivateNotes] = useState('');
  const [tags, setTags] = useState<string[]>([]);
  const [attributeValues, setAttributeValues] = useState<Record<string, string>>({});
  const [dimensions, setDimensions] = useState<DimensionEntry[]>([]);
  const [variants, setVariants] = useState<VariantEntry[]>([]);
  const [saveError, setSaveError] = useState<{ message: string; partId: PartId | null } | null>(
    null,
  );
  const [saving, setSaving] = useState(false);

  const attributeDefsQuery = useCategoryAttributeDefs(categoryId === '' ? undefined : categoryId);
  const attributeDefs = useMemo(() => attributeDefsQuery.data ?? [], [attributeDefsQuery.data]);

  // ---- Edit-mode hydration (load once per part) -----------------------
  const existingPartQuery = usePart(partId);
  const existingAttributesQuery = useAttributes(partId);
  const existingDimensionsQuery = useDimensions(partId);
  const existingVariantsQuery = useVariants(partId);
  const existingTagsQuery = useTags(partId);
  const hydratedFor = useRef<PartId | null>(null);

  useEffect(() => {
    if (!isEdit) return;
    const part = existingPartQuery.data;
    if (!part || hydratedFor.current === part.id) return;
    // Wait until every edit-mode query has resolved so the form hydrates in
    // one shot rather than flashing partially-populated.
    if (
      existingAttributesQuery.data === undefined ||
      existingDimensionsQuery.data === undefined ||
      existingVariantsQuery.data === undefined ||
      existingTagsQuery.data === undefined
    ) {
      return;
    }
    hydratedFor.current = part.id;
    setDisplayName(part.display_name);
    setCategoryId(part.category_id);
    setDescription(part.description);
    setBin(part.bin_label ?? '');
    setUsageBehavior(part.usage_behavior);
    setQuantityUnit(part.quantity_unit);
    setLowStockThreshold(part.low_stock_threshold === null ? '' : part.low_stock_threshold / 1000);
    setPublicNotes(part.public_notes);
    setPrivateNotes(part.private_notes);
    setTags(existingTagsQuery.data);
    setAttributeValues(
      Object.fromEntries(existingAttributesQuery.data.map(([key, original]) => [key, original])),
    );
    setDimensions(
      existingDimensionsQuery.data.map((d) => ({
        group: d.group,
        name: d.name,
        rawValue: d.value_num === null ? '' : `${d.value_num}${d.display_unit}`,
        source: d.source,
        notes: d.notes,
        measuredDate: d.measured_date ?? '',
        existingId: d.id,
      })),
    );
    setVariants(
      existingVariantsQuery.data.map((v) => ({
        manufacturer: v.manufacturer,
        mpn: v.mpn,
        description: v.description,
        package: v.package ?? '',
        datasheetUrl: v.datasheet_url ?? '',
        productUrl: v.product_url ?? '',
        lifecycle: v.lifecycle ?? '',
        notes: v.notes,
        listings: [],
        existingId: v.id,
      })),
    );
  }, [
    isEdit,
    existingPartQuery.data,
    existingAttributesQuery.data,
    existingDimensionsQuery.data,
    existingVariantsQuery.data,
    existingTagsQuery.data,
  ]);

  // ---- Mutations ------------------------------------------------------
  const createPart = useCreatePart();
  const updatePart = useUpdatePart();
  const setAttributeMutation = useSetAttribute();
  const addDimensionMutation = useAddDimension();
  const addVariantMutation = useAddVariant();
  const addListingMutation = useAddSupplierListing();
  const setTagsMutation = useSetTags();

  // ---- Derived: the primary variant, for the duplicate candidate ------
  const primaryVariant = variants[0];
  const candidate = useMemo(
    () =>
      buildMatchCandidate({
        categoryId,
        attributeDefs,
        attributeValues,
        variant: primaryVariant
          ? {
              manufacturer: primaryVariant.manufacturer,
              mpn: primaryVariant.mpn,
              package: primaryVariant.package || null,
              supplier: primaryVariant.listings[0]?.supplier ?? '',
              supplierSku: primaryVariant.listings[0]?.supplierSku ?? '',
            }
          : undefined,
      }),
    [categoryId, attributeDefs, attributeValues, primaryVariant],
  );

  // The primary variant as a ready draft for "Add as a variant of this
  // part" — only when it's actually filled in.
  const primaryVariantDraft = useMemo(
    () =>
      primaryVariant && isVariantEntryFilled(primaryVariant)
        ? toVariantDraft(primaryVariant)
        : null,
    [primaryVariant],
  );

  const categoryOptions: SelectFieldOption[] = useMemo(
    () => [
      { value: '', label: 'Select a category…' },
      ...(categoriesQuery.data ?? []).map((c) => ({ value: c.id, label: c.name })),
    ],
    [categoriesQuery.data],
  );

  const categoryName = useMemo(
    () => (categoriesQuery.data ?? []).find((c) => c.id === categoryId)?.name ?? '',
    [categoriesQuery.data, categoryId],
  );

  const suggestion = suggestedName({ categoryName, attributeDefs, attributeValues });
  const canSuggest = suggestion !== '' && suggestion !== displayName.trim().toLowerCase();

  function updateAttribute(key: string, raw: string) {
    setAttributeValues((current) => ({ ...current, [key]: raw }));
  }

  const canSave = displayName.trim() !== '' && categoryId !== '' && !saving;

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    if (!canSave) return;
    setSaveError(null);
    setSaving(true);

    const trimmedThreshold =
      lowStockThreshold === '' ? null : Math.round(Number(lowStockThreshold) * 1000);

    const draft: PartDraft = {
      display_name: displayName.trim(),
      category_id: categoryId,
      description: description.trim(),
      bin_label: bin.trim() === '' ? null : bin.trim(),
      usage_behavior: usageBehavior,
      quantity_unit: quantityUnit,
      low_stock_threshold: trimmedThreshold,
      public_notes: publicNotes.trim(),
      private_notes: privateNotes.trim(),
    };

    const mode = isEdit
      ? {
          kind: 'update' as const,
          record: mergeIntoRecord(existingPartQuery.data, partId as PartId, draft),
        }
      : { kind: 'create' as const, draft };

    const attributes = Object.entries(attributeValues)
      .map(([key, raw]) => [key, raw.trim()] as [string, string])
      .filter(([, raw]) => raw !== '');

    const dimensionDrafts = dimensions
      .filter((d) => !d.existingId && isDimensionEntryFilled(d))
      .map(toDimensionDraft);

    const variantInputs: SaveVariantInput[] = variants
      .filter((v) => !v.existingId && isVariantEntryFilled(v))
      .map((v) => ({
        draft: toVariantDraft(v),
        listings: v.listings.filter(isListingEntryFilled).map(toListingDraft),
      }));

    try {
      const savedId = await savePart(
        {
          createPart: (d) => createPart.mutateAsync(d),
          updatePart: (record) => updatePart.mutateAsync(record).then(() => undefined),
          setAttribute: (id, key, raw) =>
            setAttributeMutation.mutateAsync({ partId: id, key, raw }).then(() => undefined),
          setTags: (id, next) =>
            setTagsMutation.mutateAsync({ partId: id, tags: next }).then(() => undefined),
          addDimension: (id, d) =>
            addDimensionMutation.mutateAsync({ partId: id, draft: d }).then(() => undefined),
          addVariant: (id, d) => addVariantMutation.mutateAsync({ partId: id, draft: d }),
          addSupplierListing: (variantId, d) =>
            addListingMutation.mutateAsync({ variantId, draft: d }).then(() => undefined),
        },
        { mode, attributes, tags, dimensions: dimensionDrafts, variants: variantInputs },
      );

      toast({
        title: isEdit ? `Updated ${draft.display_name}` : `Created ${draft.display_name}`,
        kind: 'success',
      });
      void navigate({ to: '/inventory/$partId', params: { partId: savedId } });
    } catch (failure) {
      const err = failure as SavePartError;
      const hint =
        err.error && typeof err.error === 'object' && 'code' in err.error
          ? errorHint((err.error as { code: string }).code)
          : null;
      const detail = hint ?? errorMessage(err.error);
      const message = err.partId
        ? `The part was saved, but “${err.step}” failed: ${detail} Fix it and save again to finish the rest.`
        : `Could not create the part: ${detail}`;
      setSaveError({ message, partId: err.partId });
      toast({ title: 'Save incomplete', description: detail, kind: 'error' });
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="part-form">
      <header className="part-form-header">
        <p className="part-form-eyebrow">{isEdit ? 'Edit part' : 'Create part'}</p>
        <h1 className="part-form-title">{isEdit ? displayName || 'Edit part' : 'New part'}</h1>
      </header>

      <form className="part-form-body" onSubmit={handleSubmit}>
        <fieldset className="part-form-section">
          <legend className="part-form-legend">Basic info</legend>
          <div className="part-form-name-row">
            <TextField
              label="Display name"
              value={displayName}
              onChange={setDisplayName}
              placeholder="e.g. 10k 0603 1% resistor"
              required
              autoFocus
            />
            <button
              type="button"
              className="part-form-suggest"
              onClick={() => setDisplayName(suggestion)}
              disabled={!canSuggest}
              title={
                canSuggest ? `Use “${suggestion}”` : 'Enter a category and identity specs first'
              }
            >
              Suggest
            </button>
          </div>

          <SelectField
            label="Category"
            value={categoryId}
            onChange={setCategoryId}
            options={categoryOptions}
            required
          />

          <TextField label="Description" value={description} onChange={setDescription} />

          <TagInput tags={tags} onChange={setTags} />

          <div className="part-form-grid">
            <TextField label="Bin" value={bin} onChange={setBin} placeholder="e.g. A12" />
            <SelectField
              label="Usage behavior"
              value={usageBehavior}
              onChange={setUsageBehavior}
              options={USAGE_OPTIONS}
            />
            <SelectField
              label="Quantity unit"
              value={quantityUnit}
              onChange={(v) => setQuantityUnit(v as QuantityUnit)}
              options={UNIT_OPTIONS}
            />
            <NumberField
              label="Low-stock threshold"
              value={lowStockThreshold}
              onChange={setLowStockThreshold}
              min={0}
            />
          </div>

          <div className="part-form-grid">
            <TextField label="Public notes" value={publicNotes} onChange={setPublicNotes} />
            <TextField label="Private notes" value={privateNotes} onChange={setPrivateNotes} />
          </div>
        </fieldset>

        {categoryId !== '' && attributeDefs.length > 0 ? (
          <fieldset className="part-form-section">
            <legend className="part-form-legend">{categoryName} attributes</legend>
            <AttributeFields
              defs={attributeDefs}
              values={attributeValues}
              onChange={updateAttribute}
            />
          </fieldset>
        ) : null}

        <DuplicatePanel
          candidate={candidate}
          primaryVariant={primaryVariantDraft}
          currentPartId={partId}
        />

        <fieldset className="part-form-section">
          <legend className="part-form-legend">Dimensions</legend>
          <DimensionFields dimensions={dimensions} onChange={setDimensions} />
        </fieldset>

        <fieldset className="part-form-section">
          <legend className="part-form-legend">
            Manufacturer variants &amp; supplier listings
          </legend>
          <VariantsEditor variants={variants} onChange={setVariants} />
        </fieldset>

        {saveError ? <p className="part-form-error">{saveError.message}</p> : null}

        <div className="part-form-actions">
          <button
            type="button"
            className="part-form-cancel"
            onClick={() => void navigate({ to: '/inventory', search: { q: '' } })}
            disabled={saving}
          >
            Cancel
          </button>
          <button type="submit" className="part-form-submit" disabled={!canSave}>
            {saving ? 'Saving…' : isEdit ? 'Save changes' : 'Create part'}
          </button>
        </div>
      </form>
    </section>
  );
}

/** Merge the form's edited `PartDraft` fields into the loaded `PartRecord`
 * so `update_part` receives a complete record — preserving the fields the
 * form doesn't edit (id/created_at/modified_at/metadata_complete/archived).
 * Falls back to a synthesized record if the load somehow hasn't resolved
 * (defensive; `handleSubmit` only runs once the form is interactive, which
 * in edit mode means the part loaded). */
function mergeIntoRecord(
  existing: PartRecord | null | undefined,
  id: PartId,
  draft: PartDraft,
): PartRecord {
  const base: PartRecord =
    existing ??
    ({
      id,
      metadata_complete: false,
      archived: false,
      created_at: '',
      modified_at: '',
    } as PartRecord);
  return {
    ...base,
    id,
    display_name: draft.display_name,
    category_id: draft.category_id,
    description: draft.description,
    bin_label: draft.bin_label,
    usage_behavior: draft.usage_behavior,
    quantity_unit: draft.quantity_unit,
    low_stock_threshold: draft.low_stock_threshold,
    public_notes: draft.public_notes,
    private_notes: draft.private_notes,
  };
}
