/**
 * Inline per-row quick actions for the Inventory table (Phase 3 Task 4): a
 * hover/focus "more" menu with Add stock / Consume / Reserve / Check out.
 * Add stock and Consume are fully wired end-to-end through the same
 * `useApplyLedgerOp` hook the Ctrl+K quick-action flows (Task 5) will also
 * use, via a minimal shared quantity dialog — Task 5 owns polishing this
 * dialog further, not replacing its wiring. Reserve and Check out require a
 * `project_id` (`LedgerOp::Reserve`/`CheckOut` — see
 * `crates/inventory-core/src/ledger.rs`) and no `list_projects` command
 * exists yet (Projects is a Phase 4 stub — see `ProjectsPage.tsx`), so
 * picking a project inline isn't possible yet; those two items stay
 * visible but disabled with an honest "press Ctrl+K" hint rather than
 * either omitting them or wiring a fake/no-project reservation.
 */

import * as Dialog from '@radix-ui/react-dialog';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { useState, type FormEvent } from 'react';

import type { SearchHit } from '../../bindings.gen';
import { NumberField, TextField } from '../../components/Field';
import { useToast } from '../../components/Toast';
import { useApplyLedgerOp } from '../../hooks/inventory';
import { errorHint, formatQuantity } from '../../lib/format';
import './RowActions.css';

type DialogOp = 'receive' | 'consume_available';

const DIALOG_COPY: Record<DialogOp, { title: string; submitLabel: string; verb: string }> = {
  receive: { title: 'Add stock', submitLabel: 'Add stock', verb: 'Received' },
  consume_available: { title: 'Consume', submitLabel: 'Consume', verb: 'Consumed' },
};

export interface RowActionsProps {
  row: SearchHit;
}

export function RowActions({ row }: RowActionsProps) {
  const [dialogOp, setDialogOp] = useState<DialogOp | null>(null);
  const { toast } = useToast();

  const applyOp = useApplyLedgerOp({
    onDone: (error, data) => {
      if (error) {
        const op = dialogOp ? DIALOG_COPY[dialogOp] : null;
        toast({
          title: op ? `Could not ${op.title.toLowerCase()}` : 'Could not apply the change',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      if (data && dialogOp) {
        toast({
          title: `${DIALOG_COPY[dialogOp].verb} ${formatQuantity(data.quantity, 'each')}`,
          kind: 'success',
        });
      }
      setDialogOp(null);
    },
  });

  function handleSubmit(quantityMilli: number, note: string) {
    if (dialogOp === 'receive') {
      applyOp.mutate({ type: 'receive', part_id: row.part_id, quantity: quantityMilli, note });
    } else if (dialogOp === 'consume_available') {
      applyOp.mutate({
        type: 'consume_available',
        part_id: row.part_id,
        quantity: quantityMilli,
        project_id: null,
        note,
      });
    }
  }

  return (
    <>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button
            type="button"
            className="row-actions-trigger"
            aria-label={`Actions for ${row.display_name}`}
          >
            ⋯
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className="row-actions-menu" align="end" sideOffset={4}>
            <DropdownMenu.Item className="row-actions-item" onSelect={() => setDialogOp('receive')}>
              Add stock
            </DropdownMenu.Item>
            <DropdownMenu.Item
              className="row-actions-item"
              onSelect={() => setDialogOp('consume_available')}
            >
              Consume
            </DropdownMenu.Item>
            <DropdownMenu.Item
              className="row-actions-item row-actions-item-disabled"
              disabled
              title="Reserving needs a project — press Ctrl+K"
            >
              Reserve — press Ctrl+K
            </DropdownMenu.Item>
            <DropdownMenu.Item
              className="row-actions-item row-actions-item-disabled"
              disabled
              title="Checking out needs a project — press Ctrl+K"
            >
              Check out — press Ctrl+K
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {dialogOp ? (
        <QuantityDialog
          title={DIALOG_COPY[dialogOp].title}
          partName={row.display_name}
          submitLabel={DIALOG_COPY[dialogOp].submitLabel}
          pending={applyOp.isPending}
          onCancel={() => setDialogOp(null)}
          onSubmit={handleSubmit}
        />
      ) : null}
    </>
  );
}

interface QuantityDialogProps {
  title: string;
  partName: string;
  submitLabel: string;
  pending: boolean;
  onCancel: () => void;
  onSubmit: (quantityMilli: number, note: string) => void;
}

/** A minimal quantity + note dialog shared by every row action that needs
 * one — Task 5's Ctrl+K quick-action flows are expected to share or replace
 * this rather than duplicate it. Quantities are entered in whole (or
 * fractional, for continuous units) display units and converted to the
 * milli-unit integer the ledger expects. `SearchHit` doesn't carry the
 * part's `quantity_unit` (Phase 2c's search result shape), so the field is
 * deliberately unit-less here rather than guessing/mislabeling a suffix. */
function QuantityDialog({
  title,
  partName,
  submitLabel,
  pending,
  onCancel,
  onSubmit,
}: QuantityDialogProps) {
  const [quantity, setQuantity] = useState<number | ''>('');
  const [note, setNote] = useState('');

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (quantity === '' || quantity <= 0) return;
    onSubmit(Math.round(quantity * 1000), note);
  }

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="row-actions-dialog-overlay" />
        <Dialog.Content className="row-actions-dialog-content">
          <Dialog.Title className="row-actions-dialog-title">{title}</Dialog.Title>
          <Dialog.Description className="row-actions-dialog-description">
            {partName}
          </Dialog.Description>
          <form className="row-actions-dialog-form" onSubmit={handleSubmit}>
            <NumberField
              label="Quantity"
              value={quantity}
              onChange={setQuantity}
              min={0}
              step={0.001}
              required
              disabled={pending}
            />
            <TextField
              label="Note"
              value={note}
              onChange={setNote}
              placeholder="Optional"
              disabled={pending}
            />
            <div className="row-actions-dialog-buttons">
              <button
                type="button"
                className="row-actions-dialog-cancel"
                onClick={onCancel}
                disabled={pending}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="row-actions-dialog-submit"
                disabled={pending || quantity === '' || quantity <= 0}
              >
                {pending ? 'Saving…' : submitLabel}
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
