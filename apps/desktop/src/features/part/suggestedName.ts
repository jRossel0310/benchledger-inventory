/**
 * Composes a suggested `display_name` from the selected category and
 * whatever identity attributes are filled in so far (e.g. "10k 1% 1/4W 0603
 * resistor") — offered to the user as a fill button in `PartForm.tsx`
 * (spec §9), never forced: they can always type their own name instead, and
 * this only recomputes the suggestion, never overwrites a name the user has
 * already typed.
 *
 * Heuristic, not a re-derivation of the seed data's naming convention: the
 * entered raw text of every `identity`-flagged attribute, in the category's
 * own `display_order`, followed by the lowercased category name. Good
 * enough for a one-click starting point; the user edits from there.
 */

import type { AttributeDefRow } from '../../bindings.gen';

export interface SuggestedNameInput {
  categoryName: string;
  attributeDefs: AttributeDefRow[];
  attributeValues: Record<string, string>;
}

export function suggestedName({
  categoryName,
  attributeDefs,
  attributeValues,
}: SuggestedNameInput): string {
  const category = categoryName.trim();
  if (!category) return '';

  const identityValues = [...attributeDefs]
    .filter((def) => def.identity)
    .sort((a, b) => a.display_order - b.display_order || a.key.localeCompare(b.key))
    .map((def) => attributeValues[def.key]?.trim())
    .filter((value): value is string => Boolean(value));

  return [...identityValues, category.toLowerCase()].join(' ');
}
