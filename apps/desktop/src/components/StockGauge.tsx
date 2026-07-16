/**
 * The signature bench-instrument device (see
 * docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md): a
 * segmented horizontal bar showing a part's available/reserved/checked-out
 * split, colored with the shared stock-state tokens. All quantities are
 * milli-units (see `lib/format.ts`) — the pure helpers below do exact
 * integer arithmetic and only convert to a percentage (a display concern)
 * for the final width, never rounding the underlying milli values.
 */

import { formatQuantity } from '../lib/format';
import './StockGauge.css';

export type StockGaugeSize = 'inline' | 'panel';

export interface StockGaugeProps {
  /** Milli-unit quantity currently available (unreserved, on hand). */
  available: number;
  /** Milli-unit quantity reserved against a project. */
  reserved: number;
  /** Milli-unit quantity checked out (left the shelf). */
  checkedOut: number;
  /** The part's `QuantityUnit` (or a display abbreviation) — passed straight
   * through to `formatQuantity`. */
  unit: string;
  /** Milli-unit threshold below which `available` is considered low. `null`/
   * `undefined` (no threshold configured) never shows the low tick. */
  lowThreshold?: number | null;
  /** `inline` (default): compact, for table rows. `panel`: taller, with a
   * per-segment legend, for the part-detail header. */
  size?: StockGaugeSize;
}

export interface StockGaugeSegments {
  /** Current stock = available + reserved + checkedOut, in milli-units. */
  total: number;
  /** Percentage (0-100) of `total` that is available. */
  available: number;
  /** Percentage (0-100) of `total` that is reserved. */
  reserved: number;
  /** Percentage (0-100) of `total` that is checked out. */
  checkedOut: number;
}

/** Segment widths as percentages of *current stock* (available + reserved +
 * checkedOut) — not of some fixed reference — so the bar always fills edge
 * to edge when there is any stock at all. Returns all-zero (never NaN) when
 * there is no stock. */
export function computeStockGaugeSegments(
  available: number,
  reserved: number,
  checkedOut: number,
): StockGaugeSegments {
  const total = available + reserved + checkedOut;
  const pct = (value: number) => (total > 0 ? (value / total) * 100 : 0);
  return {
    total,
    available: pct(available),
    reserved: pct(reserved),
    checkedOut: pct(checkedOut),
  };
}

/** Whether `available` is under the configured low-stock threshold. A `null`/
 * `undefined` threshold means "no threshold configured" — never low. */
export function isStockLow(available: number, lowThreshold?: number | null): boolean {
  return lowThreshold != null && available < lowThreshold;
}

/** Where the amber low-stock tick sits along the bar: the threshold's
 * position as a percentage of current stock, clamped to the visible track
 * (0-100) so an unreachable threshold still shows at the edge instead of
 * off-bar. */
export function stockLowTickPosition(total: number, lowThreshold: number): number {
  if (total <= 0) return 0;
  return Math.min(Math.max((lowThreshold / total) * 100, 0), 100);
}

/** The accessible `aria-label` for the gauge's `role="img"`: e.g. "5
 * available, 3 reserved, 1 checked out", or the single-phrase "0 in stock"
 * when there is no stock in any state. */
export function stockGaugeAriaLabel(
  available: number,
  reserved: number,
  checkedOut: number,
  unit: string,
): string {
  const total = available + reserved + checkedOut;
  if (total === 0) return `${formatQuantity(0, unit)} in stock`;
  return [
    `${formatQuantity(available, unit)} available`,
    `${formatQuantity(reserved, unit)} reserved`,
    `${formatQuantity(checkedOut, unit)} checked out`,
  ].join(', ');
}

export function StockGauge({
  available,
  reserved,
  checkedOut,
  unit,
  lowThreshold,
  size = 'inline',
}: StockGaugeProps) {
  const segments = computeStockGaugeSegments(available, reserved, checkedOut);
  const isEmpty = segments.total === 0;
  const low = isStockLow(available, lowThreshold);
  const tickPosition = low ? stockLowTickPosition(segments.total, lowThreshold as number) : null;
  const ariaLabel = stockGaugeAriaLabel(available, reserved, checkedOut, unit);

  return (
    <div className={`stock-gauge stock-gauge-${size}`}>
      <div className="stock-gauge-track" role="img" aria-label={ariaLabel}>
        {isEmpty ? (
          <span className="stock-gauge-empty" />
        ) : (
          <>
            <span
              className="stock-gauge-segment stock-gauge-segment-available"
              style={{ width: `${segments.available}%` }}
            />
            <span
              className="stock-gauge-segment stock-gauge-segment-reserved"
              style={{ width: `${segments.reserved}%` }}
            />
            <span
              className="stock-gauge-segment stock-gauge-segment-checked-out"
              style={{ width: `${segments.checkedOut}%` }}
            />
          </>
        )}
        {tickPosition !== null && (
          <span className="stock-gauge-low-tick" style={{ left: `${tickPosition}%` }} />
        )}
      </div>
      {size === 'panel' ? (
        <div className="stock-gauge-legend">
          <span className="stock-gauge-legend-item">
            <span
              className="stock-gauge-legend-swatch stock-gauge-legend-swatch-available"
              aria-hidden="true"
            />
            <span className="stock-gauge-legend-text">
              {formatQuantity(available, unit)} available
            </span>
          </span>
          <span className="stock-gauge-legend-item">
            <span
              className="stock-gauge-legend-swatch stock-gauge-legend-swatch-reserved"
              aria-hidden="true"
            />
            <span className="stock-gauge-legend-text">
              {formatQuantity(reserved, unit)} reserved
            </span>
          </span>
          <span className="stock-gauge-legend-item">
            <span
              className="stock-gauge-legend-swatch stock-gauge-legend-swatch-checked-out"
              aria-hidden="true"
            />
            <span className="stock-gauge-legend-text">
              {formatQuantity(checkedOut, unit)} checked out
            </span>
          </span>
        </div>
      ) : (
        <span className="stock-gauge-label">
          {isEmpty ? '0 in stock' : formatQuantity(available, unit)}
        </span>
      )}
    </div>
  );
}
