import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { ToastProvider, useToast } from './Toast';

afterEach(cleanup);

function ToastTrigger() {
  const { toast } = useToast();
  return (
    <button type="button" onClick={() => toast({ title: 'Received 10', kind: 'success' })}>
      Show toast
    </button>
  );
}

describe('Toast', () => {
  it('shows a message when toast() is called', () => {
    render(
      <ToastProvider>
        <ToastTrigger />
      </ToastProvider>,
    );

    fireEvent.click(screen.getByText('Show toast'));

    expect(screen.getByText('Received 10')).toBeTruthy();
  });

  it('applies the kind-specific class for error toasts', () => {
    function ErrorTrigger() {
      const { toast } = useToast();
      return (
        <button type="button" onClick={() => toast({ title: 'Insufficient stock', kind: 'error' })}>
          Show error
        </button>
      );
    }

    render(
      <ToastProvider>
        <ErrorTrigger />
      </ToastProvider>,
    );

    fireEvent.click(screen.getByText('Show error'));

    const message = screen.getByText('Insufficient stock');
    const root = message.closest('.toast');
    expect(root?.className).toContain('toast-error');
  });

  it('shows the optional description alongside the title', () => {
    function DescriptionTrigger() {
      const { toast } = useToast();
      return (
        <button
          type="button"
          onClick={() =>
            toast({ title: 'Part archived', description: 'Only reversals allowed now.' })
          }
        >
          Show
        </button>
      );
    }

    render(
      <ToastProvider>
        <DescriptionTrigger />
      </ToastProvider>,
    );

    fireEvent.click(screen.getByText('Show'));

    expect(screen.getByText('Part archived')).toBeTruthy();
    expect(screen.getByText('Only reversals allowed now.')).toBeTruthy();
  });

  it('throws a clear error when useToast is called outside a ToastProvider', () => {
    function Rogue() {
      useToast();
      return null;
    }
    expect(() => render(<Rogue />)).toThrow(/ToastProvider/);
  });
});
