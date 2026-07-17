/**
 * Add/remove manufacturer variants and, per variant, supplier listings
 * (spec §9, §4): each variant row becomes one `VariantDraft`
 * (`useAddVariant`) and each of its listing rows one `ListingDraft`
 * (`useAddSupplierListing`, needs the real `VariantId` `add_variant`
 * returns — so listings can only be created after their variant, which is
 * exactly the order `PartForm`'s save sequence follows).
 *
 * The first variant row is the form's "primary" variant: `buildMatchCandidate`
 * (`buildMatchCandidate.ts`) reads its manufacturer/mpn/package, and its
 * first listing's supplier/SKU, to feed the `DuplicatePanel`'s search —
 * matching the backend's own `ExactMpn`/`ExactSku` match levels, which are
 * the strongest verdicts precisely because manufacturer/MPN and supplier/SKU
 * are the least ambiguous identifiers a part can carry.
 */

import { useId } from 'react';

import type { ListingDraft, VariantDraft } from '../../bindings.gen';
import { TextField } from '../../components/Field';
import './VariantsEditor.css';

export interface ListingEntry {
  supplier: string;
  supplierSku: string;
  productUrl: string;
  packaging: string;
  typicalOrder: string;
  lastUnitPrice: string;
  currency: string;
  lastPurchaseDate: string;
}

export function emptyListingEntry(): ListingEntry {
  return {
    supplier: '',
    supplierSku: '',
    productUrl: '',
    packaging: '',
    typicalOrder: '',
    lastUnitPrice: '',
    currency: '',
    lastPurchaseDate: '',
  };
}

export interface VariantEntry {
  manufacturer: string;
  mpn: string;
  description: string;
  package: string;
  datasheetUrl: string;
  productUrl: string;
  lifecycle: string;
  notes: string;
  listings: ListingEntry[];
  /** Set for a variant loaded from an existing part in edit mode. Variants
   * are append-only in the backend (`add_variant`; no update/remove), so a
   * loaded variant is shown read-only and skipped on save — only newly-added
   * variants are `add_variant`-ed. Absent for a fresh, unsaved variant. */
  existingId?: string;
}

export function emptyVariantEntry(): VariantEntry {
  return {
    manufacturer: '',
    mpn: '',
    description: '',
    package: '',
    datasheetUrl: '',
    productUrl: '',
    lifecycle: '',
    notes: '',
    listings: [],
  };
}

function nullableTrim(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === '' ? null : trimmed;
}

/** `VariantEntry` -> `VariantDraft` (`add_variant`'s wire shape). */
export function toVariantDraft(entry: VariantEntry): VariantDraft {
  return {
    manufacturer: entry.manufacturer.trim(),
    mpn: entry.mpn.trim(),
    description: entry.description.trim(),
    package: nullableTrim(entry.package),
    datasheet_url: nullableTrim(entry.datasheetUrl),
    product_url: nullableTrim(entry.productUrl),
    lifecycle: nullableTrim(entry.lifecycle),
    notes: entry.notes.trim(),
  };
}

/** A variant is worth saving once it has a manufacturer or an MPN — an
 * entirely blank trailing row is silently skipped. */
export function isVariantEntryFilled(entry: VariantEntry): boolean {
  return entry.manufacturer.trim() !== '' || entry.mpn.trim() !== '';
}

/** `ListingEntry` -> `ListingDraft` (`add_supplier_listing`'s wire shape).
 * `typicalOrder` (whole/fractional units, e.g. `"100"`) converts to the
 * milli-unit `Quantity` integer the backend stores; `lastUnitPrice` (a
 * decimal currency amount, e.g. `"0.0123"`) converts to currency-micros —
 * both `null` for a blank field, never `0`/`NaN` for "not entered". */
export function toListingDraft(entry: ListingEntry): ListingDraft {
  const typicalOrder = entry.typicalOrder.trim();
  const lastUnitPrice = entry.lastUnitPrice.trim();
  return {
    supplier: entry.supplier.trim(),
    supplier_sku: entry.supplierSku.trim(),
    product_url: nullableTrim(entry.productUrl),
    packaging: nullableTrim(entry.packaging),
    typical_order: typicalOrder === '' ? null : Math.round(Number(typicalOrder) * 1000),
    last_unit_price_micros:
      lastUnitPrice === '' ? null : Math.round(Number(lastUnitPrice) * 1_000_000),
    currency: nullableTrim(entry.currency),
    last_purchase_date: nullableTrim(entry.lastPurchaseDate),
  };
}

/** A listing is worth saving once it has a supplier and a SKU — the two
 * fields `set_attribute`-style identity matching (`ExactSku`) keys off. */
export function isListingEntryFilled(entry: ListingEntry): boolean {
  return entry.supplier.trim() !== '' && entry.supplierSku.trim() !== '';
}

export interface VariantsEditorProps {
  variants: VariantEntry[];
  onChange: (next: VariantEntry[]) => void;
  disabled?: boolean;
}

export function VariantsEditor({ variants, onChange, disabled }: VariantsEditorProps) {
  function updateVariant(index: number, patch: Partial<VariantEntry>) {
    onChange(variants.map((v, i) => (i === index ? { ...v, ...patch } : v)));
  }

  function removeVariant(index: number) {
    onChange(variants.filter((_, i) => i !== index));
  }

  function addVariant() {
    onChange([...variants, emptyVariantEntry()]);
  }

  return (
    <div className="variants-editor">
      {variants.map((variant, index) => (
        <VariantRow
          key={index}
          variant={variant}
          isPrimary={index === 0}
          disabled={disabled}
          onChange={(patch) => updateVariant(index, patch)}
          onRemove={() => removeVariant(index)}
        />
      ))}
      <button
        type="button"
        className="variants-editor-add"
        onClick={addVariant}
        disabled={disabled}
      >
        + Add variant
      </button>
    </div>
  );
}

interface VariantRowProps {
  variant: VariantEntry;
  isPrimary: boolean;
  onChange: (patch: Partial<VariantEntry>) => void;
  onRemove: () => void;
  disabled?: boolean;
}

function VariantRow({ variant, isPrimary, onChange, onRemove, disabled }: VariantRowProps) {
  // A loaded (persisted) variant is append-only in the backend — shown
  // read-only rather than offering an edit/remove the command surface can't
  // honor. Its listings are likewise not deep-loaded/editable here.
  const isExisting = Boolean(variant.existingId);
  const fieldsDisabled = disabled || isExisting;

  function updateListing(index: number, patch: Partial<ListingEntry>) {
    onChange({
      listings: variant.listings.map((l, i) => (i === index ? { ...l, ...patch } : l)),
    });
  }

  function removeListing(index: number) {
    onChange({ listings: variant.listings.filter((_, i) => i !== index) });
  }

  function addListing() {
    onChange({ listings: [...variant.listings, emptyListingEntry()] });
  }

  return (
    <div className="variant-row">
      <div className="variant-row-header">
        <span className="variant-row-eyebrow">
          {isExisting ? 'Saved variant' : isPrimary ? 'Primary variant' : 'Variant'}
        </span>
        {isExisting ? null : (
          <button
            type="button"
            className="variant-row-remove"
            onClick={onRemove}
            disabled={disabled}
            aria-label={`Remove variant ${variant.manufacturer || variant.mpn || ''}`.trim()}
          >
            Remove
          </button>
        )}
      </div>
      <div className="variant-row-fields">
        <TextField
          label="Manufacturer"
          value={variant.manufacturer}
          onChange={(manufacturer) => onChange({ manufacturer })}
          disabled={fieldsDisabled}
        />
        <TextField
          label="MPN"
          value={variant.mpn}
          onChange={(mpn) => onChange({ mpn })}
          disabled={fieldsDisabled}
        />
        <TextField
          label="Package"
          value={variant.package}
          onChange={(pkg) => onChange({ package: pkg })}
          placeholder="Optional"
          disabled={fieldsDisabled}
        />
        <TextField
          label="Lifecycle"
          value={variant.lifecycle}
          onChange={(lifecycle) => onChange({ lifecycle })}
          placeholder="e.g. Active, NRND, EOL"
          disabled={fieldsDisabled}
        />
        <TextField
          label="Datasheet URL"
          value={variant.datasheetUrl}
          onChange={(datasheetUrl) => onChange({ datasheetUrl })}
          placeholder="https://…"
          disabled={fieldsDisabled}
        />
        <TextField
          label="Product URL"
          value={variant.productUrl}
          onChange={(productUrl) => onChange({ productUrl })}
          placeholder="https://…"
          disabled={fieldsDisabled}
        />
        <TextField
          label="Description"
          value={variant.description}
          onChange={(description) => onChange({ description })}
          placeholder="Optional"
          disabled={fieldsDisabled}
        />
        <TextField
          label="Notes"
          value={variant.notes}
          onChange={(notes) => onChange({ notes })}
          placeholder="Optional"
          disabled={fieldsDisabled}
        />
      </div>

      {isExisting ? null : (
        <div className="variant-listings">
          {variant.listings.map((listing, index) => (
            <ListingRow
              key={index}
              listing={listing}
              disabled={disabled}
              onChange={(patch) => updateListing(index, patch)}
              onRemove={() => removeListing(index)}
            />
          ))}
          <button
            type="button"
            className="variant-listings-add"
            onClick={addListing}
            disabled={disabled}
          >
            + Add supplier listing
          </button>
        </div>
      )}
    </div>
  );
}

interface ListingRowProps {
  listing: ListingEntry;
  onChange: (patch: Partial<ListingEntry>) => void;
  onRemove: () => void;
  disabled?: boolean;
}

function ListingRow({ listing, onChange, onRemove, disabled }: ListingRowProps) {
  const priceId = useId();
  return (
    <div className="listing-row">
      <TextField
        label="Supplier"
        value={listing.supplier}
        onChange={(supplier) => onChange({ supplier })}
        disabled={disabled}
      />
      <TextField
        label="Supplier SKU"
        value={listing.supplierSku}
        onChange={(supplierSku) => onChange({ supplierSku })}
        disabled={disabled}
      />
      <TextField
        label="Packaging"
        value={listing.packaging}
        onChange={(packaging) => onChange({ packaging })}
        placeholder="e.g. Cut tape, Reel"
        disabled={disabled}
      />
      <TextField
        label="Typical order qty"
        value={listing.typicalOrder}
        onChange={(typicalOrder) => onChange({ typicalOrder })}
        placeholder="Optional"
        disabled={disabled}
      />
      <div className="field">
        <label className="field-label" htmlFor={priceId}>
          Unit price
        </label>
        <input
          id={priceId}
          type="text"
          className="field-input field-input-mono"
          value={listing.lastUnitPrice}
          placeholder="Optional"
          disabled={disabled}
          onChange={(event) => onChange({ lastUnitPrice: event.target.value })}
        />
      </div>
      <TextField
        label="Currency"
        value={listing.currency}
        onChange={(currency) => onChange({ currency })}
        placeholder="e.g. USD"
        disabled={disabled}
      />
      <TextField
        label="Product URL"
        value={listing.productUrl}
        onChange={(productUrl) => onChange({ productUrl })}
        placeholder="https://…"
        disabled={disabled}
      />
      <button
        type="button"
        className="listing-row-remove"
        onClick={onRemove}
        disabled={disabled}
        aria-label={`Remove listing ${listing.supplier || ''}`.trim()}
      >
        Remove
      </button>
    </div>
  );
}
