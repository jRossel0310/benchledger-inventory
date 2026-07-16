/**
 * A Radix `Toast` provider + `useToast()` hook: the app-wide surface for the
 * "the toast says `Received 10`" copy voice from the design direction doc.
 * Wired into the app root once (see `main.tsx`); any component below it
 * calls `useToast()` to show a message.
 */

import * as RadixToast from '@radix-ui/react-toast';
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';

import './Toast.css';

export type ToastKind = 'success' | 'warning' | 'error';

export interface ToastOptions {
  title: string;
  description?: string;
  kind?: ToastKind;
  durationMs?: number;
}

interface ToastEntry extends ToastOptions {
  id: string;
}

interface ToastContextValue {
  toast: (options: ToastOptions) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

let toastSeq = 0;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastEntry[]>([]);

  const dismiss = useCallback((id: string) => {
    setToasts((current) => current.filter((entry) => entry.id !== id));
  }, []);

  const toast = useCallback((options: ToastOptions) => {
    const id = `toast-${++toastSeq}`;
    setToasts((current) => [...current, { id, ...options }]);
  }, []);

  const value = useMemo<ToastContextValue>(() => ({ toast }), [toast]);

  return (
    <ToastContext.Provider value={value}>
      <RadixToast.Provider swipeDirection="right">
        {children}
        {toasts.map((entry) => (
          <RadixToast.Root
            key={entry.id}
            className={`toast toast-${entry.kind ?? 'success'}`}
            duration={entry.durationMs ?? 4000}
            onOpenChange={(open) => {
              if (!open) dismiss(entry.id);
            }}
          >
            <RadixToast.Title className="toast-title">{entry.title}</RadixToast.Title>
            {entry.description ? (
              <RadixToast.Description className="toast-description">
                {entry.description}
              </RadixToast.Description>
            ) : null}
            <RadixToast.Close className="toast-close" aria-label="Dismiss">
              ×
            </RadixToast.Close>
          </RadixToast.Root>
        ))}
        <RadixToast.Viewport className="toast-viewport" />
      </RadixToast.Provider>
    </ToastContext.Provider>
  );
}

/** `toast({ title, kind })` shows a message; `kind` (default `"success"`)
 * selects the token-colored accent (success/warning/error). Must be called
 * from within a `ToastProvider`. */
export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error('useToast must be used within a ToastProvider');
  return ctx;
}
