/**
 * Inline per-row quick actions for the Inventory table (Phase 3 Task 4): a
 * hover/focus "more" menu with Add stock / Consume / Reserve / Check out /
 * Assign bin. Add stock and Consume are fully wired end-to-end through the
 * same `useApplyLedgerOp` hook the Ctrl+K quick-action flows (Task 5) also
 * use, via a minimal shared quantity dialog local to this file — Task 5
 * deliberately leaves this working wiring alone rather than replacing it.
 * Reserve and Check out (Phase 3 Task 5) need a project, so they open the
 * shared `QuickAction` dialog (via `useQuickAction()`) with this row's part
 * preselected — the same dialog the Ctrl+K palette opens, with its own
 * project picker (including inline "Create new project…"). Assign bin
 * (Phase 3 Task 8) is this same row-menu pattern applied to `bin_label` — see
 * `AssignBinDialog` below — so it works identically whether the row is
 * reached from the plain Inventory table or the Bin browser's reused
 * `InventoryTable`.
 */

import * as Dialog from '@radix-ui/react-dialog';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { useRef, useState, type FormEvent } from 'react';

import type { PartRecord, SearchHit } from '../../bindings.gen';
import { NumberField, TextField } from '../../components/Field';
import { useToast } from '../../components/Toast';
import { useQuickAction } from '../quick/QuickActionContext';
import { useApplyLedgerOp, useBins, useUpdatePart } from '../../hooks/inventory';
import { commands, unwrap } from '../../lib/commands';
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
  const [assigningBin, setAssigningBin] = useState(false);
  const { toast } = useToast();
  const quickAction = useQuickAction();

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

  function openReserveOrCheckOut(kind: 'reserve' | 'check_out') {
    quickAction.open({ kind, part: { id: row.part_id, displayName: row.display_name } });
  }

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
              className="row-actions-item"
              onSelect={() => openReserveOrCheckOut('reserve')}
            >
              Reserve
            </DropdownMenu.Item>
            <DropdownMenu.Item
              className="row-actions-item"
              onSelect={() => openReserveOrCheckOut('check_out')}
            >
              Check out
            </DropdownMenu.Item>
            <DropdownMenu.Separator className="row-actions-separator" />
            <DropdownMenu.Item className="row-actions-item" onSelect={() => setAssigningBin(true)}>
              Assign bin
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
      {assigningBin ? <AssignBinDialog row={row} onClose={() => setAssigningBin(false)} /> : null}
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

interface AssignBinDialogProps {
  row: SearchHit;
  onClose: () => void;
}

/** Two-phase dialog state: the plain label-entry form, or (once submit finds
 * the target label already occupied by other parts) a confirm step carrying
 * the label and occupant count the warning message needs. */
type AssignBinStep = { kind: 'form' } | { kind: 'confirm'; label: string; occupantCount: number };

/**
 * Assign, reassign, or clear a part's bin (Phase 3 Task 8, shared by the
 * plain Inventory table and the Bin browser's reused `InventoryTable`).
 * Reuses `useUpdatePart` — a bin is just a field on the part record, there's
 * no dedicated "assign bin" command — rather than duplicating write logic. A
 * `SearchHit` doesn't carry every field `update_part` needs, so submit loads
 * the full `PartRecord` via `get_part` first, then patches `bin_label` and
 * saves.
 *
 * Empty input normalizes to `null`, never `''`: an empty-string bin would be
 * a value no `IS NULL` check recognizes as "no bin" (the dashboard's
 * unbinned count and the Bin browser's own Unassigned bucket both check
 * `NULL`), so it would silently vanish from both instead of showing up as
 * unassigned.
 *
 * Assigning into a bin that already holds *other* parts is a WARNING, never
 * a block — multiple parts per bin is normal (spec). `useBins()` supplies
 * live occupancy; if the (case-insensitively matched) target label already
 * has `part_count > 0`, submit pauses on the confirm step instead of saving
 * immediately, and only proceeds once the user picks "Assign anyway".
 * Clearing a bin, or re-submitting the part's own current bin unchanged,
 * never warns — neither one is "moving into an occupied bin."
 */
function AssignBinDialog({ row, onClose }: AssignBinDialogProps) {
  const [value, setValue] = useState(row.bin_label ?? '');
  const [step, setStep] = useState<AssignBinStep>({ kind: 'form' });
  const [loadError, setLoadError] = useState<string | null>(null);
  const { toast } = useToast();
  const binsQuery = useBins();
  // The label `proceed` is currently saving, for the success toast.
  // `useUpdatePart`'s mutation `variables` isn't used for this instead:
  // `onDone` is a closure captured at whatever render `useUpdatePart` last
  // ran, which isn't reliably the render that issued the in-flight
  // `.mutate()` call — a plain ref set immediately before `.mutate()` is
  // unambiguous regardless of render timing.
  const pendingLabelRef = useRef<string | null>(null);

  const updatePart = useUpdatePart({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not assign bin',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      const assigned = pendingLabelRef.current;
      toast({ title: assigned ? `Moved to bin ${assigned}` : 'Bin cleared', kind: 'success' });
      onClose();
    },
  });

  async function proceed(normalized: string | null) {
    setLoadError(null);
    let record: PartRecord | null;
    try {
      record = await unwrap(commands.getPart(row.part_id));
    } catch {
      setLoadError('Could not load this part — try again.');
      return;
    }
    if (!record) {
      setLoadError('This part no longer exists.');
      return;
    }
    pendingLabelRef.current = normalized;
    updatePart.mutate({ ...record, bin_label: normalized });
  }

  function isSameBin(a: string | null, b: string | null): boolean {
    if (a === null || b === null) return a === b;
    return a.toLowerCase() === b.toLowerCase();
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = value.trim();
    const normalized = trimmed === '' ? null : trimmed;
    if (isSameBin(normalized, row.bin_label)) {
      // Nothing actually changed — close rather than round-trip a no-op
      // write (and never treat "re-saving your own current bin" as moving
      // into an occupied one).
      onClose();
      return;
    }
    if (normalized !== null) {
      const occupant = (binsQuery.data ?? []).find(
        (bin) => bin.bin_label !== null && isSameBin(bin.bin_label, normalized),
      );
      if (occupant && occupant.part_count > 0) {
        setStep({ kind: 'confirm', label: normalized, occupantCount: occupant.part_count });
        return;
      }
    }
    void proceed(normalized);
  }

  function confirmAssign() {
    if (step.kind !== 'confirm') return;
    void proceed(step.label);
  }

  const pending = updatePart.isPending;
  // Occupancy is only trustworthy once `useBins()` has actually returned —
  // disabling submit for that brief initial fetch (never on a later refetch
  // or an error, both of which leave stale-but-still-usable `data` in place)
  // keeps a fast submit from ever bypassing the warning by racing ahead of
  // the bins query, without blocking the common case where bins are already
  // warm in the cache (e.g. opened from the Bin browser itself).
  const submitDisabled = pending || binsQuery.isLoading;

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="row-actions-dialog-overlay" />
        <Dialog.Content className="row-actions-dialog-content">
          {step.kind === 'form' ? (
            <>
              <Dialog.Title className="row-actions-dialog-title">Assign bin</Dialog.Title>
              <Dialog.Description className="row-actions-dialog-description">
                {row.display_name}
              </Dialog.Description>
              <form className="row-actions-dialog-form" onSubmit={handleSubmit}>
                <TextField
                  label="Bin"
                  value={value}
                  onChange={setValue}
                  placeholder="e.g. A12 — leave blank to clear"
                  disabled={pending}
                  autoFocus
                />
                {loadError ? <p className="row-actions-dialog-error">{loadError}</p> : null}
                <div className="row-actions-dialog-buttons">
                  <button
                    type="button"
                    className="row-actions-dialog-cancel"
                    onClick={onClose}
                    disabled={pending}
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    className="row-actions-dialog-submit"
                    disabled={submitDisabled}
                  >
                    {pending ? 'Saving…' : 'Assign'}
                  </button>
                </div>
              </form>
            </>
          ) : (
            <>
              <Dialog.Title className="row-actions-dialog-title">Bin already occupied</Dialog.Title>
              <Dialog.Description className="row-actions-dialog-description">
                Bin {step.label} already holds {step.occupantCount} part
                {step.occupantCount === 1 ? '' : 's'} — assign anyway?
              </Dialog.Description>
              {loadError ? <p className="row-actions-dialog-error">{loadError}</p> : null}
              <div className="row-actions-dialog-buttons">
                <button
                  type="button"
                  className="row-actions-dialog-cancel"
                  onClick={() => setStep({ kind: 'form' })}
                  disabled={pending}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  className="row-actions-dialog-submit"
                  onClick={confirmAssign}
                  disabled={pending}
                >
                  {pending ? 'Saving…' : 'Assign anyway'}
                </button>
              </div>
            </>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
