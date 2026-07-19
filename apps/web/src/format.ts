/**
 * Milli-unit quantity display for the web SPA -- the same exact-integer
 * split as the desktop's `lib/format.ts` `formatQuantity` (no float
 * division, so `1500` -> `1.5` without rounding artifacts). The snapshot's
 * `quantity_unit` carries display abbreviations (`"each"`, `"m"`, `"ft"`);
 * `each` renders as a bare number, anything else is appended as a suffix.
 */
export function formatQuantity(milli: number, unit: string): string {
  const sign = milli < 0 ? '-' : '';
  const abs = Math.abs(milli);
  const whole = Math.floor(abs / 1000);
  const frac = abs % 1000;
  const number =
    frac === 0
      ? `${sign}${whole}`
      : `${sign}${whole}.${String(frac).padStart(3, '0').replace(/0+$/, '')}`;
  return unit === 'each' || unit === '' ? number : `${number} ${unit}`;
}
