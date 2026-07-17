/**
 * Part detail's Variants tab (Phase 3 Task 7): the manufacturer-specific
 * options on file for this part (`list_variants`) — manufacturer, MPN,
 * package, lifecycle, datasheet/product links, and which one is preferred
 * (the one the rest of the app defaults to, e.g. for a future reorder flow).
 * "Set preferred" (`set_preferred_variant`) is the only write this tab
 * offers; adding/editing variants themselves stays on the part form (T6) —
 * a full variant editor doesn't belong in a read-first inspector.
 */

import type { PartId, VariantId } from '../../bindings.gen';
import { useSetPreferredVariant, useVariants } from '../../hooks/inventory';
import { useToast } from '../../components/Toast';
import { errorHint, errorMessage } from '../../lib/format';
import './PartDetail.css';

export interface PartDetailVariantsProps {
  partId: PartId;
}

export function PartDetailVariants({ partId }: PartDetailVariantsProps) {
  const variantsQuery = useVariants(partId);
  const { toast } = useToast();

  const setPreferred = useSetPreferredVariant({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not set preferred variant',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
      }
    },
  });

  if (variantsQuery.isPending) {
    return <p className="part-detail-status">Loading variants…</p>;
  }
  if (variantsQuery.isError) {
    return (
      <p className="part-detail-status part-detail-status-error">
        Could not load variants: {errorMessage(variantsQuery.error)}
      </p>
    );
  }

  const variants = variantsQuery.data ?? [];
  if (variants.length === 0) {
    return (
      <p className="part-detail-status">
        No manufacturer variants on file yet — add one from Edit.
      </p>
    );
  }

  function handleSetPreferred(variantId: VariantId) {
    setPreferred.mutate({ partId, variantId });
  }

  return (
    <ul className="part-detail-variant-list">
      {variants.map((variant) => (
        <li key={variant.id} className="part-detail-variant-card">
          <div className="part-detail-variant-header">
            <span className="part-detail-mono part-detail-variant-manufacturer">
              {variant.manufacturer}
            </span>
            <span className="part-detail-mono part-detail-variant-mpn">{variant.mpn}</span>
            {variant.is_preferred ? (
              <span className="part-detail-badge part-detail-badge-preferred">Preferred</span>
            ) : (
              <button
                type="button"
                className="part-detail-link-button"
                disabled={setPreferred.isPending}
                onClick={() => handleSetPreferred(variant.id)}
              >
                Set preferred
              </button>
            )}
          </div>
          <p className="part-detail-section-body">{variant.description || 'No description.'}</p>
          <dl className="part-detail-inline-fields">
            <div>
              <dt>Package</dt>
              <dd className="part-detail-mono">{variant.package ?? '—'}</dd>
            </div>
            <div>
              <dt>Lifecycle</dt>
              <dd>{variant.lifecycle ?? '—'}</dd>
            </div>
          </dl>
          <div className="part-detail-variant-links">
            {variant.datasheet_url ? (
              <a href={variant.datasheet_url} target="_blank" rel="noreferrer">
                Datasheet
              </a>
            ) : null}
            {variant.product_url ? (
              <a href={variant.product_url} target="_blank" rel="noreferrer">
                Product page
              </a>
            ) : null}
          </div>
          {variant.notes ? <p className="part-detail-muted">{variant.notes}</p> : null}
        </li>
      ))}
    </ul>
  );
}
