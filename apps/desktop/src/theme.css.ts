import { generateCssVariables, type ThemeName } from '@ei/shared';

/** Inject semantic token CSS variables into the document (idempotent). */
export function applyTheme(theme: ThemeName): void {
  const id = 'ei-theme-vars';
  let el = document.getElementById(id) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement('style');
    el.id = id;
    document.head.appendChild(el);
  }
  el.textContent = generateCssVariables(theme);
}
