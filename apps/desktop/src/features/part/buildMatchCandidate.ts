/**
 * Builds the `MatchCandidate` the `DuplicatePanel`'s debounced `find_matches`
 * call scores against parts already on file, from the part form's current
 * state (spec §9, §10). Pure and framework-free so the "what counts as
 * enough signal to search" rule is unit-testable without rendering
 * anything.
 *
 * Returns `null` — disabling the search entirely, per `useFindMatches` — when
 * nothing identity-defining has been entered yet: an empty/near-empty
 * candidate would either match everything or nothing useful, and firing a
 * request on every keystroke of, say, the free-text description field would
 * be noise. "Enough signal" is deliberately narrower than "every entered
 * attribute": it's an `identity`-flagged attribute, a package, an MPN, or a
 * supplier SKU — the same fields the design brief names ("as identity-
 * defining fields ... are entered"). Once a candidate is built at all,
 * though, `attributes` carries every entered value (not just identity
 * ones) — the backend's matching engine already restricts identity
 * comparison to `identity`-flagged defs internally, so sending the rest is
 * harmless and saves this helper from duplicating that filtering.
 */

import type { AttributeDefRow, CategoryId, MatchCandidate } from '../../bindings.gen';

/** The minimum a caller needs from the form's "primary" variant/listing (the
 * first row of `VariantsEditor`, if any) to feed `MatchCandidate`'s
 * manufacturer/mpn/supplier/supplier_sku fields — a subset of `VariantDraft`
 * + `ListingDraft` so callers don't have to construct either draft shape
 * just to build a candidate. */
export interface MatchCandidateVariantSource {
  manufacturer: string;
  mpn: string;
  package: string | null;
  supplier: string;
  supplierSku: string;
}

export interface BuildMatchCandidateInput {
  categoryId: CategoryId | '';
  attributeDefs: AttributeDefRow[];
  /** Raw text per attribute key, exactly as `set_attribute` would receive
   * it — the same map `AttributeFields` maintains. */
  attributeValues: Record<string, string>;
  /** The form's primary variant, if one has been entered — see
   * `VariantsEditor`'s "first row" convention. */
  variant?: MatchCandidateVariantSource;
}

function nonEmpty(value: string | undefined | null): string | null {
  const trimmed = value?.trim() ?? '';
  return trimmed === '' ? null : trimmed;
}

export function buildMatchCandidate({
  categoryId,
  attributeDefs,
  attributeValues,
  variant,
}: BuildMatchCandidateInput): MatchCandidate | null {
  const attributes = Object.entries(attributeValues)
    .map(([key, raw]) => [key, raw.trim()] as [string, string])
    .filter(([, raw]) => raw !== '');

  const identityKeys = new Set(attributeDefs.filter((def) => def.identity).map((def) => def.key));
  const hasIdentityAttr = attributes.some(([key]) => identityKeys.has(key));

  const manufacturer = nonEmpty(variant?.manufacturer);
  const mpn = nonEmpty(variant?.mpn);
  const supplier = nonEmpty(variant?.supplier);
  const supplierSku = nonEmpty(variant?.supplierSku);
  // "package" is usually one of `attributes` (most passive categories link
  // it as a regular, identity-flagged attribute); fall back to the primary
  // variant's own package field (e.g. a category that doesn't link
  // "package" but the user still recorded a THT/SMD variant package).
  const packageValue =
    attributes.find(([key]) => key === 'package')?.[1] ?? nonEmpty(variant?.package) ?? null;

  const hasSignal =
    hasIdentityAttr || mpn !== null || packageValue !== null || supplierSku !== null;
  if (!hasSignal) return null;

  return {
    supplier,
    supplier_sku: supplierSku,
    manufacturer,
    mpn,
    category_id: categoryId || null,
    attributes,
    package: packageValue,
  };
}
