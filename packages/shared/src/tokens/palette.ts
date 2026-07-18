/** Primitive palette. Dark graphite base, saturated non-pastel accents. */
export const palette = {
  graphite950: '#111214',
  graphite900: '#17181b',
  graphite850: '#1d1f23',
  graphite800: '#232529',
  graphite700: '#2e3138',
  graphite300: '#a6adbb',
  graphite200: '#c7ccd6',
  offWhite: '#eef0f4',
  paper: '#f4f5f7',
  paperPanel: '#ffffff',
  paperElevated: '#eceef2',
  ink900: '#181a1e',
  ink600: '#3f4450',
  ink400: '#666d7c',
  blue500: '#2f6fed',
  blue400: '#4f86f0',
  green500: '#1f9d55',
  amber500: '#e08a00',
  red500: '#d63333',
  violet500: '#7a5af8',
  cyan500: '#0e9db8',
  // Project-status accents (planned/active/completed/archived) — deliberately
  // quieter/desaturated relative to the vivid stock-state accents above, per
  // the design direction's "everything else stays quiet" rule: the stock
  // gauge is the one bold, saturated device; a status chip is a label, not
  // another instrument reading, so it must never be mistaken for a
  // available/reserved/checked-out/low-stock signal.
  slate400: '#7c8798',
  teal500: '#3f9188',
  taupe400: '#8a8073',
} as const;
