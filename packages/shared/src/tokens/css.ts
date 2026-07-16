import { FONT_TOKEN_NAMES, fonts } from './fonts';
import { SEMANTIC_TOKEN_NAMES, themes, type ThemeName } from './semantic';

/**
 * Emit `:root` CSS custom properties for a theme: the theme's semantic color
 * tokens followed by the (theme-independent) font tokens. Deterministic
 * order.
 */
export function generateCssVariables(theme: ThemeName): string {
  const colorLines = SEMANTIC_TOKEN_NAMES.map((name) => `  --${name}: ${themes[theme][name]};`);
  const fontLines = FONT_TOKEN_NAMES.map((name) => `  --${name}: ${fonts[name]};`);
  return `:root {\n${[...colorLines, ...fontLines].join('\n')}\n}\n`;
}
