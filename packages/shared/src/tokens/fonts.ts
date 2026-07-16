/**
 * Font-stack tokens. System stacks only (the app is self-contained, no web
 * fonts) — see docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md.
 * Unlike the semantic color tokens, font stacks are the same in every theme,
 * so there is no per-theme record here, just one set of values.
 */
export const FONT_TOKEN_NAMES = ['font-data', 'font-ui'] as const;

export type FontTokenName = (typeof FONT_TOKEN_NAMES)[number];
export type FontTokens = Record<FontTokenName, string>;

export const fonts: FontTokens = {
  // Instrument face: identifiers and measurements (part IDs, MPNs, SKUs, bin
  // labels, quantities, normalized spec values, timestamps).
  'font-data': "ui-monospace, 'Cascadia Code', 'Cascadia Mono', 'SF Mono', Consolas, monospace",
  // UI face: section labels, buttons, descriptions, empty states.
  'font-ui': "'Segoe UI', system-ui, sans-serif",
};
