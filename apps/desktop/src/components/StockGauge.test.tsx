import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import {
  computeStockGaugeSegments,
  isStockLow,
  stockGaugeAriaLabel,
  stockLowTickPosition,
  StockGauge,
} from './StockGauge';

afterEach(cleanup);

describe('computeStockGaugeSegments', () => {
  it('sizes each segment as a fraction of current stock (available+reserved+checkedOut)', () => {
    // 5000/3000/1000 milli -> current stock is 9000 milli (9 units), not some
    // other reference total: each segment's percentage is of that sum.
    const segments = computeStockGaugeSegments(5000, 3000, 1000);
    expect(segments.total).toBe(9000);
    expect(segments.available).toBeCloseTo((5000 / 9000) * 100, 6);
    expect(segments.reserved).toBeCloseTo((3000 / 9000) * 100, 6);
    expect(segments.checkedOut).toBeCloseTo((1000 / 9000) * 100, 6);
    expect(segments.available + segments.reserved + segments.checkedOut).toBeCloseTo(100, 6);
  });

  it('produces round percentages for round inputs', () => {
    const segments = computeStockGaugeSegments(6000, 3000, 1000);
    expect(segments.total).toBe(10000);
    expect(segments.available).toBeCloseTo(60, 6);
    expect(segments.reserved).toBeCloseTo(30, 6);
    expect(segments.checkedOut).toBeCloseTo(10, 6);
  });

  it('returns zero-width segments (no NaN/Infinity) when there is no stock at all', () => {
    const segments = computeStockGaugeSegments(0, 0, 0);
    expect(segments.total).toBe(0);
    expect(segments.available).toBe(0);
    expect(segments.reserved).toBe(0);
    expect(segments.checkedOut).toBe(0);
  });
});

describe('isStockLow', () => {
  it('is low when available is below the threshold', () => {
    expect(isStockLow(2000, 5000)).toBe(true);
  });

  it('is not low when available meets or exceeds the threshold', () => {
    expect(isStockLow(5000, 5000)).toBe(false);
    expect(isStockLow(6000, 5000)).toBe(false);
  });

  it('is never low when no threshold is configured', () => {
    expect(isStockLow(0, null)).toBe(false);
    expect(isStockLow(0, undefined)).toBe(false);
  });
});

describe('stockLowTickPosition', () => {
  it('places the tick at the threshold fraction of current stock', () => {
    expect(stockLowTickPosition(9000, 4000)).toBeCloseTo((4000 / 9000) * 100, 6);
  });

  it('clamps to the right edge when the threshold exceeds current stock', () => {
    expect(stockLowTickPosition(1000, 5000)).toBe(100);
  });

  it('is zero when there is no current stock', () => {
    expect(stockLowTickPosition(0, 5000)).toBe(0);
  });
});

describe('stockGaugeAriaLabel', () => {
  it('reports each state in "each" units', () => {
    expect(stockGaugeAriaLabel(5000, 3000, 1000, 'each')).toBe(
      '5 available, 3 reserved, 1 checked out',
    );
  });

  it('reports fractional continuous-unit quantities with their suffix', () => {
    expect(stockGaugeAriaLabel(1500, 0, 0, 'meter')).toBe(
      '1.5 m available, 0 m reserved, 0 m checked out',
    );
  });

  it('reports the zero-stock case as a single phrase', () => {
    expect(stockGaugeAriaLabel(0, 0, 0, 'each')).toBe('0 in stock');
  });
});

describe('StockGauge component', () => {
  it('renders an accessible role="img" with the composed aria-label', () => {
    render(<StockGauge available={5000} reserved={3000} checkedOut={1000} unit="each" />);
    expect(
      screen.getByRole('img', { name: '5 available, 3 reserved, 1 checked out' }),
    ).toBeTruthy();
  });

  it('renders three color segments sized by their percentage of current stock', () => {
    const { container } = render(
      <StockGauge available={6000} reserved={3000} checkedOut={1000} unit="each" />,
    );
    const available = container.querySelector('.stock-gauge-segment-available') as HTMLElement;
    const reserved = container.querySelector('.stock-gauge-segment-reserved') as HTMLElement;
    const checkedOut = container.querySelector('.stock-gauge-segment-checked-out') as HTMLElement;
    expect(available.style.width).toBe('60%');
    expect(reserved.style.width).toBe('30%');
    expect(checkedOut.style.width).toBe('10%');
  });

  it('shows the amber low tick when available is under the low threshold', () => {
    const { container } = render(
      <StockGauge available={1000} reserved={0} checkedOut={0} unit="each" lowThreshold={5000} />,
    );
    expect(container.querySelector('.stock-gauge-low-tick')).toBeTruthy();
  });

  it('omits the low tick when available meets the threshold or none is configured', () => {
    const { container: withinThreshold } = render(
      <StockGauge available={9000} reserved={0} checkedOut={0} unit="each" lowThreshold={5000} />,
    );
    expect(withinThreshold.querySelector('.stock-gauge-low-tick')).toBeNull();

    const { container: noThreshold } = render(
      <StockGauge available={1000} reserved={0} checkedOut={0} unit="each" />,
    );
    expect(noThreshold.querySelector('.stock-gauge-low-tick')).toBeNull();
  });

  it('renders the empty-gauge, "0 in stock" state when there is no stock at all', () => {
    const { container } = render(
      <StockGauge available={0} reserved={0} checkedOut={0} unit="each" />,
    );
    expect(screen.getByRole('img', { name: '0 in stock' })).toBeTruthy();
    expect(screen.getByText('0 in stock')).toBeTruthy();
    expect(container.querySelector('.stock-gauge-segment')).toBeNull();
  });

  it('defaults to the inline size and applies the panel size class when requested', () => {
    const { container: inlineContainer } = render(
      <StockGauge available={5000} reserved={0} checkedOut={0} unit="each" />,
    );
    expect(inlineContainer.querySelector('.stock-gauge-inline')).toBeTruthy();

    const { container: panelContainer } = render(
      <StockGauge available={5000} reserved={3000} checkedOut={1000} unit="each" size="panel" />,
    );
    expect(panelContainer.querySelector('.stock-gauge-panel')).toBeTruthy();
  });

  it('labels each segment individually in the panel size', () => {
    render(
      <StockGauge available={5000} reserved={3000} checkedOut={1000} unit="each" size="panel" />,
    );
    expect(screen.getByText('5 available')).toBeTruthy();
    expect(screen.getByText('3 reserved')).toBeTruthy();
    expect(screen.getByText('1 checked out')).toBeTruthy();
  });

  it('shows a single monospaced numeric label in the inline size', () => {
    const { container } = render(
      <StockGauge available={5000} reserved={3000} checkedOut={1000} unit="each" />,
    );
    expect(container.querySelector('.stock-gauge-label')?.textContent).toBe('5');
  });
});
