/**
 * Part detail's Supplier listings tab (Phase 3 Task 7): every supplier SKU
 * on file, grouped by the manufacturer variant it's a listing for —
 * `list_supplier_listings` is scoped to one `variant_id` (Phase 2's data
 * model: a listing belongs to a variant, not directly to a part), so this
 * tab first loads the part's variants (`list_variants`) and then, per
 * variant, its listings via a small child component — one `useQuery` call
 * per variant, which keeps each hook call unconditional (rules of hooks)
 * while still following the part -> variant -> listing chain the backend
 * models.
 */

import type { PartId, VariantId } from '../../bindings.gen';
import { useSupplierListings, useVariants } from '../../hooks/inventory';
import { errorMessage, formatPrice, formatQuantity, formatTimestamp } from '../../lib/format';
import './PartDetail.css';

export interface PartDetailSupplierListingsProps {
  partId: PartId;
}

export function PartDetailSupplierListings({ partId }: PartDetailSupplierListingsProps) {
  const variantsQuery = useVariants(partId);

  if (variantsQuery.isPending) {
    return <p className="part-detail-status">Loading supplier listings…</p>;
  }
  if (variantsQuery.isError) {
    return (
      <p className="part-detail-status part-detail-status-error">
        Could not load supplier listings: {errorMessage(variantsQuery.error)}
      </p>
    );
  }

  const variants = variantsQuery.data ?? [];
  if (variants.length === 0) {
    return (
      <p className="part-detail-status">
        No manufacturer variants on file yet — supplier listings attach to a variant.
      </p>
    );
  }

  return (
    <div className="part-detail-listing-groups">
      {variants.map((variant) => (
        <section key={variant.id} className="part-detail-listing-group">
          <h3 className="part-detail-section-title">
            <span className="part-detail-mono">{variant.manufacturer}</span>{' '}
            <span className="part-detail-mono">{variant.mpn}</span>
          </h3>
          <VariantListings variantId={variant.id} />
        </section>
      ))}
    </div>
  );
}

function VariantListings({ variantId }: { variantId: VariantId }) {
  const listingsQuery = useSupplierListings(variantId);

  if (listingsQuery.isPending) {
    return <p className="part-detail-status">Loading…</p>;
  }
  if (listingsQuery.isError) {
    return (
      <p className="part-detail-status part-detail-status-error">
        Could not load listings: {errorMessage(listingsQuery.error)}
      </p>
    );
  }

  const listings = listingsQuery.data ?? [];
  if (listings.length === 0) {
    return <p className="part-detail-status">No supplier listings for this variant yet.</p>;
  }

  return (
    <table className="part-detail-table">
      <thead>
        <tr>
          <th>Supplier</th>
          <th>SKU</th>
          <th>Price</th>
          <th>Packaging</th>
          <th>Typical order</th>
          <th>Last purchased</th>
        </tr>
      </thead>
      <tbody>
        {listings.map((listing) => (
          <tr key={listing.id}>
            <td>
              {listing.product_url ? (
                <a href={listing.product_url} target="_blank" rel="noreferrer">
                  {listing.supplier}
                </a>
              ) : (
                listing.supplier
              )}
            </td>
            <td className="part-detail-mono">{listing.supplier_sku}</td>
            <td className="part-detail-mono">
              {formatPrice(listing.last_unit_price_micros, listing.currency)}
            </td>
            <td>{listing.packaging ?? '—'}</td>
            <td className="part-detail-mono">
              {listing.typical_order !== null ? formatQuantity(listing.typical_order, 'each') : '—'}
            </td>
            <td className="part-detail-mono">
              {listing.last_purchase_date ? formatTimestamp(listing.last_purchase_date) : '—'}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
