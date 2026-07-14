import { SEMANTIC_TOKEN_NAMES, themes, type ThemeName } from './semantic';

/** Emit `:root` CSS custom properties for a theme. Deterministic order. */
export function generateCssVariables(theme: ThemeName): string {
  const lines = SEMANTIC_TOKEN_NAMES.map((name) => `  --${name}: ${themes[theme][name]};`);
  return `:root {\n${lines.join('\n')}\n}\n`;
}
