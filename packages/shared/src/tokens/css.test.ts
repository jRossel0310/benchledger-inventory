import { describe, expect, it } from 'vitest';
import { generateCssVariables, SEMANTIC_TOKEN_NAMES, themes } from '../index';

describe('design tokens', () => {
  it('defines every semantic token in both themes', () => {
    for (const theme of ['dark', 'light'] as const) {
      for (const name of SEMANTIC_TOKEN_NAMES) {
        expect(themes[theme][name], `${theme}/${name}`).toMatch(/^#[0-9a-f]{6}$/);
      }
    }
  });

  it('emits one CSS custom property per semantic token', () => {
    const css = generateCssVariables('dark');
    for (const name of SEMANTIC_TOKEN_NAMES) {
      expect(css).toContain(`--${name}:`);
    }
  });

  it('covers the token names the spec requires', () => {
    const required = [
      'color-bg-app', 'color-bg-panel', 'color-bg-elevated', 'color-border',
      'color-text-primary', 'color-text-secondary', 'color-text-muted',
      'color-action-primary', 'color-action-hover', 'color-focus-ring',
      'color-stock-available', 'color-stock-reserved', 'color-stock-checked-out',
      'color-stock-low', 'color-warning', 'color-error', 'color-success',
    ];
    for (const name of required) expect(SEMANTIC_TOKEN_NAMES).toContain(name);
  });
});
