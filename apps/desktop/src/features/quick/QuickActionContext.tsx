/**
 * Global "open a QuickAction dialog" entry point. The Ctrl+K `CommandPalette`,
 * inline row actions (Reserve/Check out in `RowActions.tsx`), and — once
 * Task 7 lands — the part-detail inspector all need to open the same
 * keyboard-first quick-action dialog, sometimes with a part already chosen.
 * Rather than each caller managing its own open/close state and mounting
 * its own `<QuickAction/>`, one provider owns the single active request and
 * renders the one dialog instance; callers just call
 * `useQuickAction().open(request)`. Mounted once in `AppShell.tsx` (the root
 * route's component, wrapping every routed screen), so it works from any
 * route — nested inside `main.tsx`'s `ToastProvider`, which `QuickAction`
 * relies on via `useToast()`.
 */

import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';

import { QuickAction, type QuickActionRequest } from './QuickAction';

export type { QuickActionPart, QuickActionRequest } from './QuickAction';

interface QuickActionContextValue {
  /** Opens the quick-action dialog for `request`. If a request is already
   * open, this replaces it (there is only ever one QuickAction dialog on
   * screen at a time). */
  open: (request: QuickActionRequest) => void;
}

const QuickActionContext = createContext<QuickActionContextValue | null>(null);

export function QuickActionProvider({ children }: { children: ReactNode }) {
  const [request, setRequest] = useState<QuickActionRequest | null>(null);

  const open = useCallback((next: QuickActionRequest) => setRequest(next), []);
  const close = useCallback(() => setRequest(null), []);

  const value = useMemo<QuickActionContextValue>(() => ({ open }), [open]);

  return (
    <QuickActionContext.Provider value={value}>
      {children}
      {request ? <QuickAction request={request} onClose={close} /> : null}
    </QuickActionContext.Provider>
  );
}

/** Must be called from within a `QuickActionProvider` (mounted once in
 * `AppShell.tsx`). */
export function useQuickAction(): QuickActionContextValue {
  const ctx = useContext(QuickActionContext);
  if (!ctx) throw new Error('useQuickAction must be used within a QuickActionProvider');
  return ctx;
}
