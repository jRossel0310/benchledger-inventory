/**
 * The Bin browser screen (Phase 3 Task 8, see
 * docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md §9): a
 * part's storage location is its `bin_label` field, not a separate `bins`
 * table (see `crates/inventory-db/src/bins.rs`'s module doc) — this screen
 * groups parts by that label so the shelf can be browsed the way you'd walk
 * it, rather than scrolling the flat Inventory table hunting for a location.
 *
 * Left: a tile grid from `useBins()` (backend-aggregated label + part count,
 * `crates/inventory-db/src/bins.rs`'s `list_bins`), including a distinct
 * "Unassigned" tile for the `bin_label IS NULL` bucket — always shown, even
 * at a count of `0`, so "every part has a home" is visible as a state, not
 * just inferred from the tile's absence. Selecting a tile shows its parts by
 * reusing `InventoryTable`: a named bin reuses the search grammar's `bin:`
 * filter directly (the same mechanism Filters.tsx/SavedViews.tsx already
 * rely on); Unassigned has no such key (`bin:` is an exact-match filter — a
 * SQL `NULL` never equality-matches a value, see `search.rs`'s
 * `filter_exact_ci` — and the search grammar doesn't have an "is unbinned"
 * flag either), so it drives `InventoryTable`'s optional client-side
 * `filter` prop over the full unfiltered result instead of adding a new
 * backend search key for one screen's one bucket (the same call
 * `SavedViews.tsx` already documented making for its own "Unassigned bin"
 * preset).
 *
 * Right: the selected bin's parts, plus (named bins only) a rename control.
 * Renaming (`useRenameBin`, backed by the atomic `rename_bin` command) moves
 * every part currently in the old label to the new one in one step; merging
 * into an already-occupied label warns via a confirm dialog rather than
 * blocking (same "warn, don't forbid" rule the per-row assign flow in
 * `RowActions.tsx`'s `AssignBinDialog` follows for the same reason: multiple
 * parts sharing one bin is normal, spec-permitted behavior, not an error
 * state). Creating a *new* bin needs no dedicated control here — assigning
 * any part to a label that doesn't exist yet (`AssignBinDialog`, reachable
 * from any row in the tables below) is how a bin comes into existence, per
 * the "bins are just labels on parts" model.
 */

import { useState } from 'react';
import type { FormEvent } from 'react';

import type { BinSummary, SearchHit } from '../../bindings.gen';
import { useToast } from '../../components/Toast';
import { TextField } from '../../components/Field';
import { useBins, useRenameBin } from '../../hooks/inventory';
import { errorHint, errorMessage } from '../../lib/format';
import { InventoryTable } from '../inventory/InventoryTable';
import './BinBrowser.css';

type Selection = { kind: 'bin'; label: string } | { kind: 'unassigned' } | null;

/** Quote a bin label for the search grammar's `bin:` filter when it contains
 * whitespace — the same rule `Filters.tsx`'s `withCategory` applies to
 * `category:` values, mirroring `inventory_core::search::tokenize`'s quoting
 * (`crates/inventory-core/src/search.rs`). */
function quoteForSearch(label: string): string {
  return /\s/.test(label) ? `"${label}"` : label;
}

function isSameBinLabel(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

export function BinBrowser() {
  const binsQuery = useBins();
  const [selection, setSelection] = useState<Selection>(null);

  if (binsQuery.isPending) {
    return <p className="bin-browser-status">Loading bins…</p>;
  }

  if (binsQuery.isError) {
    return (
      <p className="bin-browser-status bin-browser-status-error">
        Could not load bins: {errorMessage(binsQuery.error)}
      </p>
    );
  }

  const bins = binsQuery.data;

  if (bins.length === 0) {
    return (
      <section className="bin-browser bin-browser-empty">
        <p className="bin-browser-eyebrow">Bins</p>
        <h1 className="bin-browser-empty-title">No parts yet</h1>
        <p className="bin-browser-empty-description">
          Press <kbd>Ctrl</kbd>+<kbd>K</kbd> to create a part, then assign it a bin from its row
          menu.
        </p>
      </section>
    );
  }

  const namedBins = bins.filter(
    (bin): bin is BinSummary & { bin_label: string } => bin.bin_label !== null,
  );
  const unassigned = bins.find((bin) => bin.bin_label === null) ?? {
    bin_label: null,
    part_count: 0,
  };

  return (
    <section className="bin-browser">
      <header className="bin-browser-header">
        <p className="bin-browser-eyebrow">Bins</p>
        <h1 className="bin-browser-title">Physical storage, browsed by location</h1>
      </header>

      <div className="bin-browser-body">
        <div className="bin-browser-grid" role="list" aria-label="Bins">
          {namedBins.map((bin) => (
            <BinTile
              key={bin.bin_label}
              label={bin.bin_label}
              count={bin.part_count}
              active={selection?.kind === 'bin' && isSameBinLabel(selection.label, bin.bin_label)}
              onClick={() => setSelection({ kind: 'bin', label: bin.bin_label })}
            />
          ))}
          <BinTile
            label="Unassigned"
            count={unassigned.part_count}
            active={selection?.kind === 'unassigned'}
            distinct
            onClick={() => setSelection({ kind: 'unassigned' })}
          />
        </div>

        <div className="bin-browser-detail">
          {selection === null ? (
            <p className="bin-browser-hint">Select a bin to see its parts.</p>
          ) : selection.kind === 'bin' ? (
            <BinDetail
              key={selection.label}
              label={selection.label}
              bins={namedBins}
              onRenamed={(newLabel) => setSelection({ kind: 'bin', label: newLabel })}
            />
          ) : (
            <>
              <div className="bin-browser-detail-header">
                <h2 className="bin-browser-detail-title">
                  Unassigned{' '}
                  <span className="bin-browser-detail-count">{unassigned.part_count}</span>
                </h2>
              </div>
              <InventoryTable
                query=""
                filter={(hit: SearchHit) => hit.bin_label === null}
                emptyMessage="No unassigned parts — every part has a bin."
              />
            </>
          )}
        </div>
      </div>
    </section>
  );
}

interface BinTileProps {
  label: string;
  count: number;
  active: boolean;
  /** The Unassigned tile reads distinctly (dashed border, muted accent)
   * rather than as just another alphabetical entry — it's a catch-all, not a
   * physical location like the others. */
  distinct?: boolean;
  onClick: () => void;
}

function BinTile({ label, count, active, distinct, onClick }: BinTileProps) {
  return (
    <button
      type="button"
      role="listitem"
      className={`bin-tile${active ? ' bin-tile-active' : ''}${distinct ? ' bin-tile-distinct' : ''}`}
      aria-pressed={active}
      onClick={onClick}
    >
      <span className="bin-tile-label">{label}</span>
      <span className="bin-tile-count">{count}</span>
    </button>
  );
}

interface BinDetailProps {
  label: string;
  bins: (BinSummary & { bin_label: string })[];
  onRenamed: (newLabel: string) => void;
}

/** The selected named bin's header (rename control) plus its parts table. */
function BinDetail({ label, bins, onRenamed }: BinDetailProps) {
  return (
    <>
      <div className="bin-browser-detail-header">
        <h2 className="bin-browser-detail-title">
          Bin <span className="bin-browser-detail-label">{label}</span>
        </h2>
        <RenameBinForm currentLabel={label} bins={bins} onRenamed={onRenamed} />
      </div>
      <InventoryTable query={`bin:${quoteForSearch(label)}`} />
    </>
  );
}

interface RenameConfirm {
  newLabel: string;
  occupantCount: number;
}

interface RenameBinFormProps {
  currentLabel: string;
  bins: (BinSummary & { bin_label: string })[];
  onRenamed: (newLabel: string) => void;
}

/** Renames `currentLabel` to a new label, moving every part currently in it
 * (`useRenameBin`, the atomic `rename_bin` command) — a bulk operation, not
 * N per-part assigns. Renaming into a label that already has other parts
 * merges them; that's a WARNING (a confirm dialog), never a block — the same
 * "multiple parts per bin is normal" rule `AssignBinDialog` enforces for a
 * single part. Rendered with `key={selection.label}` by its caller so
 * switching which bin is selected resets this form's local state instead of
 * carrying over a stale in-progress edit. */
function RenameBinForm({ currentLabel, bins, onRenamed }: RenameBinFormProps) {
  const [value, setValue] = useState(currentLabel);
  const [confirm, setConfirm] = useState<RenameConfirm | null>(null);
  const { toast } = useToast();

  const renameBin = useRenameBin({
    onDone: (error, movedCount) => {
      if (error) {
        toast({
          title: 'Could not rename bin',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      const newLabel = confirm?.newLabel ?? value.trim();
      const count = movedCount ?? 0;
      toast({
        title: `Renamed to ${newLabel} (${count} part${count === 1 ? '' : 's'} moved)`,
        kind: 'success',
      });
      setConfirm(null);
      onRenamed(newLabel);
    },
  });

  function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = value.trim();
    if (!trimmed || isSameBinLabel(trimmed, currentLabel)) return;
    const occupant = bins.find(
      (bin) =>
        !isSameBinLabel(bin.bin_label, currentLabel) && isSameBinLabel(bin.bin_label, trimmed),
    );
    if (occupant && occupant.part_count > 0) {
      setConfirm({ newLabel: trimmed, occupantCount: occupant.part_count });
      return;
    }
    renameBin.mutate({ oldLabel: currentLabel, newLabel: trimmed });
  }

  function confirmRename() {
    if (!confirm) return;
    renameBin.mutate({ oldLabel: currentLabel, newLabel: confirm.newLabel });
  }

  const pending = renameBin.isPending;
  const trimmedValue = value.trim();
  const disabled = pending || !trimmedValue || isSameBinLabel(trimmedValue, currentLabel);

  return (
    <div className="bin-browser-rename">
      <form className="bin-browser-rename-form" onSubmit={submit}>
        <TextField label="Rename bin" value={value} onChange={setValue} disabled={pending} />
        <button type="submit" className="bin-browser-rename-submit" disabled={disabled}>
          {pending ? 'Renaming…' : 'Rename'}
        </button>
      </form>
      {confirm ? (
        <div
          className="bin-browser-rename-confirm"
          role="alertdialog"
          aria-label="Confirm bin merge"
        >
          <p className="bin-browser-rename-confirm-text">
            Bin {confirm.newLabel} already holds {confirm.occupantCount} part
            {confirm.occupantCount === 1 ? '' : 's'} — rename anyway?
          </p>
          <div className="bin-browser-rename-confirm-buttons">
            <button
              type="button"
              className="bin-browser-rename-confirm-cancel"
              onClick={() => setConfirm(null)}
              disabled={pending}
            >
              Cancel
            </button>
            <button
              type="button"
              className="bin-browser-rename-confirm-submit"
              onClick={confirmRename}
              disabled={pending}
            >
              {pending ? 'Renaming…' : 'Rename anyway'}
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
