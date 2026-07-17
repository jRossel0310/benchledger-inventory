/**
 * Global "open the part inspector drawer" entry point (Phase 3 Task 7,
 * mirroring `features/quick/QuickActionContext.tsx`'s pattern for the
 * QuickAction dialog): the Inventory table's row click, and any other
 * "inspect this part without navigating away" affordance, call
 * `usePartInspector().open(partId)` instead of each owning its own
 * open/close state and mounting its own `<PartInspector/>`. Mounted once in
 * `AppShell.tsx` so it works from any route, nested alongside
 * `QuickActionProvider` — the inspector's own primary actions
 * (Add stock/Consume/Reserve/Check out) open the QuickAction dialog *on top
 * of* the drawer via that separate context, so both need to be live at the
 * shell level regardless of nesting order.
 */

import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';

import type { PartId } from '../../bindings.gen';
import { PartInspector } from './PartInspector';

interface PartInspectorContextValue {
  /** Opens the drawer for `partId`. If one is already open, this replaces
   * it (there is only ever one inspector drawer on screen at a time). */
  open: (partId: PartId) => void;
}

const PartInspectorContext = createContext<PartInspectorContextValue | null>(null);

export function PartInspectorProvider({ children }: { children: ReactNode }) {
  const [partId, setPartId] = useState<PartId | null>(null);

  const open = useCallback((next: PartId) => setPartId(next), []);
  const close = useCallback(() => setPartId(null), []);

  const value = useMemo<PartInspectorContextValue>(() => ({ open }), [open]);

  return (
    <PartInspectorContext.Provider value={value}>
      {children}
      {partId !== null ? <PartInspector partId={partId} onClose={close} /> : null}
    </PartInspectorContext.Provider>
  );
}

/** Must be called from within a `PartInspectorProvider` (mounted once in
 * `AppShell.tsx`). */
export function usePartInspector(): PartInspectorContextValue {
  const ctx = useContext(PartInspectorContext);
  if (!ctx) throw new Error('usePartInspector must be used within a PartInspectorProvider');
  return ctx;
}
