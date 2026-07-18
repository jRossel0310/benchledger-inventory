import { palette } from './palette';

export const SEMANTIC_TOKEN_NAMES = [
  'color-bg-app',
  'color-bg-panel',
  'color-bg-elevated',
  'color-border',
  'color-text-primary',
  'color-text-secondary',
  'color-text-muted',
  'color-action-primary',
  'color-action-hover',
  'color-focus-ring',
  'color-stock-available',
  'color-stock-reserved',
  'color-stock-checked-out',
  'color-stock-low',
  'color-warning',
  'color-error',
  'color-success',
  // Project-status chip accents (Phase 4 Projects UI) — distinct from the
  // stock-state tokens above by design: a project's lifecycle status and a
  // part's physical stock split are unrelated facts and must never share a
  // color vocabulary (see palette.ts's comment on the underlying hexes).
  'color-status-planned',
  'color-status-active',
  'color-status-completed',
  'color-status-archived',
] as const;

export type SemanticTokenName = (typeof SEMANTIC_TOKEN_NAMES)[number];
export type SemanticTheme = Record<SemanticTokenName, string>;
export type ThemeName = 'dark' | 'light';

export const themes: Record<ThemeName, SemanticTheme> = {
  dark: {
    'color-bg-app': palette.graphite950,
    'color-bg-panel': palette.graphite900,
    'color-bg-elevated': palette.graphite850,
    'color-border': palette.graphite700,
    'color-text-primary': palette.offWhite,
    'color-text-secondary': palette.graphite200,
    'color-text-muted': palette.graphite300,
    'color-action-primary': palette.blue500,
    'color-action-hover': palette.blue400,
    'color-focus-ring': palette.blue400,
    'color-stock-available': palette.green500,
    'color-stock-reserved': palette.violet500,
    'color-stock-checked-out': palette.cyan500,
    'color-stock-low': palette.amber500,
    'color-warning': palette.amber500,
    'color-error': palette.red500,
    'color-success': palette.green500,
    'color-status-planned': palette.slate400,
    'color-status-active': palette.blue500,
    'color-status-completed': palette.teal500,
    'color-status-archived': palette.taupe400,
  },
  light: {
    'color-bg-app': palette.paper,
    'color-bg-panel': palette.paperPanel,
    'color-bg-elevated': palette.paperElevated,
    'color-border': palette.graphite200,
    'color-text-primary': palette.ink900,
    'color-text-secondary': palette.ink600,
    'color-text-muted': palette.ink400,
    'color-action-primary': palette.blue500,
    'color-action-hover': palette.blue400,
    'color-focus-ring': palette.blue500,
    'color-stock-available': palette.green500,
    'color-stock-reserved': palette.violet500,
    'color-stock-checked-out': palette.cyan500,
    'color-stock-low': palette.amber500,
    'color-warning': palette.amber500,
    'color-error': palette.red500,
    'color-success': palette.green500,
    'color-status-planned': palette.slate400,
    'color-status-active': palette.blue500,
    'color-status-completed': palette.teal500,
    'color-status-archived': palette.taupe400,
  },
};
