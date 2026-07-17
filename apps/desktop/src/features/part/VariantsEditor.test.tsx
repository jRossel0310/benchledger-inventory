import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  emptyListingEntry,
  emptyVariantEntry,
  isListingEntryFilled,
  isVariantEntryFilled,
  toListingDraft,
  toVariantDraft,
  VariantsEditor,
  type ListingEntry,
  type VariantEntry,
} from './VariantsEditor';

afterEach(cleanup);

describe('VariantsEditor — variants', () => {
  it('renders no variant rows and an "Add variant" button when empty', () => {
    render(<VariantsEditor variants={[]} onChange={vi.fn()} />);
    expect(screen.queryByLabelText('Manufacturer')).toBeNull();
    expect(screen.getByText('+ Add variant')).toBeTruthy();
  });

  it('appends a fresh empty variant when "Add variant" is clicked', () => {
    const onChange = vi.fn();
    render(<VariantsEditor variants={[]} onChange={onChange} />);
    fireEvent.click(screen.getByText('+ Add variant'));
    expect(onChange).toHaveBeenCalledWith([emptyVariantEntry()]);
  });

  it('labels the first row "Primary variant" and later ones just "Variant"', () => {
    render(
      <VariantsEditor
        variants={[variantRow({ manufacturer: 'Yageo' }), variantRow({ manufacturer: 'Vishay' })]}
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByText('Primary variant')).toBeTruthy();
    expect(screen.getByText('Variant')).toBeTruthy();
  });

  it('reports an edited field through onChange without touching other variants', () => {
    const onChange = vi.fn();
    const rows = [variantRow({ manufacturer: 'Yageo' }), variantRow({ manufacturer: 'Vishay' })];
    render(<VariantsEditor variants={rows} onChange={onChange} />);

    fireEvent.change(screen.getAllByLabelText('MPN')[0]!, {
      target: { value: 'RC0603FR-0710KL' },
    });

    expect(onChange).toHaveBeenCalledWith([{ ...rows[0], mpn: 'RC0603FR-0710KL' }, rows[1]]);
  });

  it('removes only the targeted variant, including its listings', () => {
    const onChange = vi.fn();
    const rows = [variantRow({ manufacturer: 'Yageo' }), variantRow({ manufacturer: 'Vishay' })];
    render(<VariantsEditor variants={rows} onChange={onChange} />);

    fireEvent.click(screen.getByLabelText('Remove variant Yageo'));

    expect(onChange).toHaveBeenCalledWith([rows[1]]);
  });

  it('disables every field and button when disabled', () => {
    render(<VariantsEditor variants={[variantRow()]} onChange={vi.fn()} disabled />);
    expect((screen.getByLabelText('Manufacturer') as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByText('+ Add variant') as HTMLButtonElement).disabled).toBe(true);
  });
});

describe('VariantsEditor — supplier listings', () => {
  it('adds a listing row scoped to its variant via "+ Add supplier listing"', () => {
    const onChange = vi.fn();
    render(<VariantsEditor variants={[variantRow()]} onChange={onChange} />);
    fireEvent.click(screen.getByText('+ Add supplier listing'));
    expect(onChange).toHaveBeenCalledWith([{ ...variantRow(), listings: [emptyListingEntry()] }]);
  });

  it('edits a listing field scoped to its own variant, leaving sibling variants untouched', () => {
    const onChange = vi.fn();
    const rows = [
      variantRow({ manufacturer: 'Yageo', listings: [listingRow({ supplier: 'DigiKey' })] }),
      variantRow({ manufacturer: 'Vishay' }),
    ];
    render(<VariantsEditor variants={rows} onChange={onChange} />);

    fireEvent.change(screen.getByLabelText('Supplier SKU'), {
      target: { value: '311-10.0KLRCT-ND' },
    });

    expect(onChange).toHaveBeenCalledWith([
      {
        ...rows[0],
        listings: [{ ...rows[0]!.listings[0]!, supplierSku: '311-10.0KLRCT-ND' }],
      },
      rows[1],
    ]);
  });

  it('removes only the targeted listing', () => {
    const onChange = vi.fn();
    const rows = [
      variantRow({
        listings: [listingRow({ supplier: 'DigiKey' }), listingRow({ supplier: 'Mouser' })],
      }),
    ];
    render(<VariantsEditor variants={rows} onChange={onChange} />);

    fireEvent.click(screen.getByLabelText('Remove listing DigiKey'));

    expect(onChange).toHaveBeenCalledWith([{ ...rows[0], listings: [rows[0]!.listings[1]!] }]);
  });

  it("scopes each variant's listing rows so a second variant's listings are independent", () => {
    const rows = [
      variantRow({ manufacturer: 'Yageo', listings: [listingRow({ supplier: 'DigiKey' })] }),
      variantRow({ manufacturer: 'Vishay', listings: [listingRow({ supplier: 'Mouser' })] }),
    ];
    render(<VariantsEditor variants={rows} onChange={vi.fn()} />);
    const [firstVariant, secondVariant] = screen
      .getAllByText(/^(Primary variant|Variant)$/)
      .map((el) => el.closest('.variant-row') as HTMLElement);
    expect(within(firstVariant!).getByDisplayValue('DigiKey')).toBeTruthy();
    expect(within(secondVariant!).getByDisplayValue('Mouser')).toBeTruthy();
  });
});

describe('toVariantDraft / isVariantEntryFilled', () => {
  it('trims fields and converts blanks to null for the nullable ones', () => {
    expect(
      toVariantDraft({
        manufacturer: ' Yageo ',
        mpn: ' RC0603FR-0710KL ',
        description: '  ',
        package: '',
        datasheetUrl: '  https://example.com/ds.pdf  ',
        productUrl: '',
        lifecycle: '',
        notes: '',
        listings: [],
      }),
    ).toEqual({
      manufacturer: 'Yageo',
      mpn: 'RC0603FR-0710KL',
      description: '',
      package: null,
      datasheet_url: 'https://example.com/ds.pdf',
      product_url: null,
      lifecycle: null,
      notes: '',
    });
  });

  it('is filled once either manufacturer or MPN is entered', () => {
    expect(isVariantEntryFilled(emptyVariantEntry())).toBe(false);
    expect(isVariantEntryFilled(variantRow({ manufacturer: 'Yageo' }))).toBe(true);
    expect(isVariantEntryFilled(variantRow({ mpn: 'RC0603FR-0710KL' }))).toBe(true);
  });
});

describe('toListingDraft / isListingEntryFilled', () => {
  it('converts a whole typical order quantity to milli-units', () => {
    expect(toListingDraft(listingRow({ typicalOrder: '100' })).typical_order).toBe(100_000);
  });

  it('converts a unit price to currency micros', () => {
    expect(toListingDraft(listingRow({ lastUnitPrice: '0.0123' })).last_unit_price_micros).toBe(
      12_300,
    );
  });

  it('leaves typical order and price null when blank', () => {
    const draft = toListingDraft(listingRow());
    expect(draft.typical_order).toBeNull();
    expect(draft.last_unit_price_micros).toBeNull();
  });

  it('is filled once both supplier and supplier SKU are entered', () => {
    expect(isListingEntryFilled(emptyListingEntry())).toBe(false);
    expect(isListingEntryFilled(listingRow({ supplier: 'DigiKey' }))).toBe(false);
    expect(
      isListingEntryFilled(listingRow({ supplier: 'DigiKey', supplierSku: '311-10.0KLRCT-ND' })),
    ).toBe(true);
  });
});

function variantRow(overrides: Partial<VariantEntry> = {}): VariantEntry {
  return { ...emptyVariantEntry(), ...overrides };
}

function listingRow(overrides: Partial<ListingEntry> = {}): ListingEntry {
  return { ...emptyListingEntry(), ...overrides };
}
