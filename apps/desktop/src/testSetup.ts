/**
 * Vitest environment setup (registered as `test.setupFiles` in
 * `vitest.config.ts`). jsdom does not implement the Pointer Events API
 * (`window.PointerEvent` is `undefined`, and elements have no
 * `set/release/hasPointerCapture`) — Radix primitives that open on
 * `pointerdown` (e.g. `DropdownMenu.Trigger`, used by the Inventory
 * browser's row actions, Phase 3 Task 4) or that manage pointer capture
 * during interaction silently no-op or throw without these. Polyfilling
 * them here, once, keeps every test file that renders a Radix
 * dropdown/menu/select from having to redo this itself.
 */

if (typeof window !== 'undefined' && typeof window.PointerEvent === 'undefined') {
  class PointerEventPolyfill extends MouseEvent implements PointerEvent {
    pointerId: number;
    width: number;
    height: number;
    pressure: number;
    tangentialPressure: number;
    tiltX: number;
    tiltY: number;
    twist: number;
    pointerType: string;
    isPrimary: boolean;
    altitudeAngle: number;
    azimuthAngle: number;

    constructor(type: string, params: PointerEventInit = {}) {
      super(type, params);
      this.pointerId = params.pointerId ?? 0;
      this.width = params.width ?? 1;
      this.height = params.height ?? 1;
      this.pressure = params.pressure ?? 0;
      this.tangentialPressure = params.tangentialPressure ?? 0;
      this.tiltX = params.tiltX ?? 0;
      this.tiltY = params.tiltY ?? 0;
      this.twist = params.twist ?? 0;
      this.pointerType = params.pointerType ?? 'mouse';
      this.isPrimary = params.isPrimary ?? true;
      this.altitudeAngle = params.altitudeAngle ?? 0;
      this.azimuthAngle = params.azimuthAngle ?? 0;
    }

    getCoalescedEvents(): PointerEvent[] {
      return [];
    }

    getPredictedEvents(): PointerEvent[] {
      return [];
    }
  }

  window.PointerEvent = PointerEventPolyfill;
}

for (const method of ['hasPointerCapture', 'setPointerCapture', 'releasePointerCapture'] as const) {
  if (typeof Element.prototype[method] !== 'function') {
    // @ts-expect-error -- jsdom has no pointer-capture support at all.
    Element.prototype[method] = () => (method === 'hasPointerCapture' ? false : undefined);
  }
}

if (typeof Element.prototype.scrollIntoView !== 'function') {
  Element.prototype.scrollIntoView = () => {};
}

/** jsdom has no `ResizeObserver` — `cmdk`'s `Command.List` (used by the
 * Ctrl+K command palette and the QuickAction dialog's part-search step,
 * Phase 3 Task 5) observes its own size to expose a `--cmdk-list-height`
 * CSS variable. A no-op stand-in is enough: the tests care about filtering
 * and keyboard selection, never the measured pixel height. */
if (typeof window !== 'undefined' && typeof window.ResizeObserver === 'undefined') {
  class ResizeObserverPolyfill {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  window.ResizeObserver = ResizeObserverPolyfill as unknown as typeof ResizeObserver;
}
