/**
 * The BOM editor (Phase 4 Task 6, spec §"Projects and BOMs"): the seven
 * spec columns — Part / Per build / Needed / Available / Reserved /
 * Consumed / Missing — computed entirely server-side (`BomItemRecord`, see
 * `crates/inventory-db/src/bom.rs`'s `derive_reserved_consumed`), plus
 * add/edit/remove/substitute actions. Self-contained: fetches its own
 * `useBom(projectId)` rather than taking `items` as a prop, the same "a
 * section owns its own query" pattern `PartDetailTransactions` etc. use, so
 * `ProjectDetail` only has to mount `<BomTable projectId={id}/>`.
 *
 * `BomItemRecord` doesn't carry the part's `quantity_unit` (only
 * `part_display_name`) — every quantity here renders with
 * `formatQuantity(value, 'each')`. That's correct for the common case and
 * never wrong for `each`-unit parts (the suffix is empty), but understates a
 * continuous-unit BOM line (e.g. wire by the meter) the same documented way
 * `InventoryTable.tsx`'s `SearchHit`-based columns already do for the plain
 * Inventory table — a real, pre-existing gap this task doesn't fix (adding
 * `quantity_unit` to `BomItemRecord` is bindings/backend work outside its
 * scope), not an oversight.
 *
 * No inline `StockGauge` per row: the gauge's three segments are
 * available/reserved/checked-out (a part's *global* physical stock split),
 * while a BOM line's columns are available/reserved/consumed/missing (this
 * *project's* draw against that stock, plus how much of the build is done)
 * — different axes that would misuse the gauge's meaning if forced into it.
 * The seven columns already show the split the spec asks for.
 *
 * Edit and "set substitutes" are merged into one dialog (`EditBomItemDialog`)
 * rather than two, since both edit the same line and a caller opening "Edit"
 * expects to see everything about the line in one place; the substitutes
 * multi-select still calls the dedicated `useSetBomSubstitutes` mutation
 * distinctly from `useUpdateBomItem`, matching the two separate commands.
 */

import * as Dialog from '@radix-ui/react-dialog';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { Command } from 'cmdk';
import { useState, type FormEvent } from 'react';

import type { BomItemRecord, PartId, ProjectId, SearchHit } from '../../bindings.gen';
import { DataTable, type DataTableColumn } from '../../components/DataTable';
import { CheckboxField, NumberField, TextField } from '../../components/Field';
import { useToast } from '../../components/Toast';
import { usePart, useSearch } from '../../hooks/inventory';
import {
  useAddBomItem,
  useBom,
  useRemoveBomItem,
  useSetBomSubstitutes,
  useUpdateBomItem,
} from '../../hooks/projects';
import { errorHint, errorMessage, formatQuantity, type CommandError } from '../../lib/format';
import { usePartInspector } from '../part/PartInspectorContext';
import './BomTable.css';

// See the module doc comment: `BomItemRecord` has no `quantity_unit`.
const BOM_UNIT = 'each';

const COLUMNS: DataTableColumn<BomItemRecord>[] = [
  {
    key: 'part',
    header: 'Part',
    width: 220,
    mono: true,
    render: (row) => (
      <div className="bom-table-part-cell">
        <span className="bom-table-part-name">{row.part_display_name}</span>
        {row.reference_designators ? (
          <span className="bom-table-part-refdes">{row.reference_designators}</span>
        ) : null}
      </div>
    ),
  },
  {
    key: 'per_build',
    header: 'Per build',
    width: 84,
    mono: true,
    render: (row) => formatQuantity(row.quantity_per_build, BOM_UNIT),
  },
  {
    key: 'needed',
    header: 'Needed',
    width: 84,
    mono: true,
    render: (row) => formatQuantity(row.total_required, BOM_UNIT),
  },
  {
    key: 'available',
    header: 'Available',
    width: 84,
    mono: true,
    render: (row) => formatQuantity(row.available, BOM_UNIT),
  },
  {
    key: 'reserved',
    header: 'Reserved',
    width: 84,
    mono: true,
    render: (row) => formatQuantity(row.reserved, BOM_UNIT),
  },
  {
    key: 'consumed',
    header: 'Consumed',
    width: 84,
    mono: true,
    render: (row) => formatQuantity(row.consumed, BOM_UNIT),
  },
  {
    key: 'missing',
    header: 'Missing',
    width: 84,
    mono: true,
    render: (row) => (
      <span className={row.missing > 0 ? 'bom-table-missing' : undefined}>
        {formatQuantity(row.missing, BOM_UNIT)}
      </span>
    ),
  },
];

export interface BomTableProps {
  projectId: ProjectId;
}

export function BomTable({ projectId }: BomTableProps) {
  const bomQuery = useBom(projectId);
  const partInspector = usePartInspector();
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<BomItemRecord | null>(null);
  const [removing, setRemoving] = useState<BomItemRecord | null>(null);

  function handleActivate(row: BomItemRecord) {
    partInspector.open(row.part_id);
  }

  if (bomQuery.isPending) {
    return <p className="bom-table-status">Loading BOM…</p>;
  }
  if (bomQuery.isError) {
    return (
      <p className="bom-table-status bom-table-status-error">
        Could not load the BOM: {errorMessage(bomQuery.error)}
      </p>
    );
  }

  const items = bomQuery.data;
  const knownPartIds = new Set(items.map((item) => item.part_id));

  return (
    <div className="bom-table-wrap">
      <div className="bom-table-toolbar">
        <h2 className="bom-table-heading">Bill of materials</h2>
        <button type="button" className="bom-table-add" onClick={() => setAdding(true)}>
          Add part
        </button>
      </div>
      <DataTable
        rows={items}
        columns={COLUMNS}
        getRowId={(row) => row.id}
        onActivate={handleActivate}
        rowActions={(row) => (
          <BomRowMenu row={row} onEdit={() => setEditing(row)} onRemove={() => setRemoving(row)} />
        )}
        emptyMessage="No BOM lines yet — add a part to start the bill of materials."
        aria-label="Bill of materials"
      />
      {adding ? (
        <AddBomItemDialog
          projectId={projectId}
          excludePartIds={knownPartIds}
          onClose={() => setAdding(false)}
        />
      ) : null}
      {editing ? (
        <EditBomItemDialog projectId={projectId} item={editing} onClose={() => setEditing(null)} />
      ) : null}
      {removing ? (
        <RemoveBomItemDialog
          projectId={projectId}
          item={removing}
          onClose={() => setRemoving(null)}
        />
      ) : null}
    </div>
  );
}

interface BomRowMenuProps {
  row: BomItemRecord;
  onEdit: () => void;
  onRemove: () => void;
}

function BomRowMenu({ row, onEdit, onRemove }: BomRowMenuProps) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          className="bom-table-row-trigger"
          aria-label={`Actions for ${row.part_display_name}`}
        >
          ⋯
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="bom-table-row-menu" align="end" sideOffset={4}>
          <DropdownMenu.Item className="bom-table-row-menu-item" onSelect={onEdit}>
            Edit line
          </DropdownMenu.Item>
          <DropdownMenu.Item
            className="bom-table-row-menu-item bom-table-row-menu-item--danger"
            onSelect={onRemove}
          >
            Remove
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

interface AddBomItemDialogProps {
  projectId: ProjectId;
  /** Parts already on this BOM — filtered out of search results so a caller
   * can't pick a duplicate line (the backend's `UNIQUE(project_id, part_id)`
   * would reject it anyway; this just avoids the round trip). */
  excludePartIds: Set<PartId>;
  onClose: () => void;
}

/** Two-step dialog — search/pick a part, then fill in the line's fields —
 * the same shape `QuickAction.tsx`'s part-search step uses, reusing
 * `useSearch` rather than a bespoke part picker. */
function AddBomItemDialog({ projectId, excludePartIds, onClose }: AddBomItemDialogProps) {
  const [part, setPart] = useState<{ id: PartId; displayName: string } | null>(null);
  const [query, setQuery] = useState('');
  const [quantityPerBuild, setQuantityPerBuild] = useState<number | ''>(1);
  const [refDes, setRefDes] = useState('');
  const [required, setRequired] = useState(true);
  const [notes, setNotes] = useState('');
  const [submitError, setSubmitError] = useState<CommandError | null>(null);
  const { toast } = useToast();

  const searchQuery = useSearch(query);
  const results = (searchQuery.data ?? []).filter((hit) => !excludePartIds.has(hit.part_id));

  const addBomItem = useAddBomItem({
    onDone: (error, data) => {
      if (error) {
        setSubmitError(error);
        toast({
          title: 'Could not add BOM line',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      if (data) {
        toast({ title: `Added ${data.part_display_name} to the BOM`, kind: 'success' });
      }
      onClose();
    },
  });

  function pick(hit: SearchHit) {
    setPart({ id: hit.part_id, displayName: hit.display_name });
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!part || quantityPerBuild === '' || quantityPerBuild <= 0) return;
    setSubmitError(null);
    addBomItem.mutate({
      projectId,
      draft: {
        part_id: part.id,
        quantity_per_build: Math.round(quantityPerBuild * 1000),
        reference_designators: refDes.trim(),
        required,
        notes: notes.trim(),
      },
    });
  }

  const pending = addBomItem.isPending;

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="bom-table-dialog-overlay" />
        <Dialog.Content className="bom-table-dialog-content">
          <Dialog.Title className="bom-table-dialog-title">Add to BOM</Dialog.Title>
          <Dialog.Description className="bom-table-dialog-description">
            {part ? part.displayName : 'Search for a part to add.'}
          </Dialog.Description>

          {!part ? (
            <Command shouldFilter={false} label="Search parts" className="bom-table-search">
              <Command.Input
                autoFocus
                value={query}
                onValueChange={setQuery}
                placeholder="Search parts…"
                className="bom-table-search-input"
              />
              <Command.List className="bom-table-search-list">
                <Command.Empty className="bom-table-search-empty">
                  {query.trim() ? 'No parts match.' : 'Type to search a part…'}
                </Command.Empty>
                {results.map((hit) => (
                  <Command.Item
                    key={hit.part_id}
                    value={hit.part_id}
                    onSelect={() => pick(hit)}
                    className="bom-table-search-item"
                  >
                    <span className="bom-table-search-item-name">{hit.display_name}</span>
                    <span className="bom-table-search-item-meta">{hit.category_name}</span>
                  </Command.Item>
                ))}
              </Command.List>
            </Command>
          ) : (
            <form className="bom-table-form" onSubmit={handleSubmit}>
              <NumberField
                label="Quantity per build"
                value={quantityPerBuild}
                onChange={setQuantityPerBuild}
                min={0}
                step={0.001}
                required
                autoFocus
                disabled={pending}
              />
              <TextField
                label="Reference designators"
                value={refDes}
                onChange={setRefDes}
                placeholder="e.g. R1-R10"
                disabled={pending}
              />
              <CheckboxField
                label="Required"
                checked={required}
                onChange={setRequired}
                disabled={pending}
              />
              <TextField
                label="Notes"
                value={notes}
                onChange={setNotes}
                placeholder="Optional"
                disabled={pending}
              />
              {submitError ? (
                <p className="bom-table-dialog-error">
                  {errorHint(submitError.code) ?? submitError.message}
                </p>
              ) : null}
              <div className="bom-table-dialog-buttons">
                <button
                  type="button"
                  className="bom-table-dialog-cancel"
                  onClick={onClose}
                  disabled={pending}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="bom-table-dialog-submit"
                  disabled={pending || quantityPerBuild === '' || quantityPerBuild <= 0}
                >
                  {pending ? 'Adding…' : 'Add to BOM'}
                </button>
              </div>
            </form>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface EditBomItemDialogProps {
  projectId: ProjectId;
  item: BomItemRecord;
  onClose: () => void;
}

function EditBomItemDialog({ projectId, item, onClose }: EditBomItemDialogProps) {
  const [quantityPerBuild, setQuantityPerBuild] = useState<number | ''>(
    item.quantity_per_build / 1000,
  );
  const [refDes, setRefDes] = useState(item.reference_designators);
  const [required, setRequired] = useState(item.required);
  const [notes, setNotes] = useState(item.notes);
  const [substitutes, setSubstitutes] = useState<PartId[]>(item.substitutes);
  const [substituteQuery, setSubstituteQuery] = useState('');
  const [submitError, setSubmitError] = useState<CommandError | null>(null);
  const { toast } = useToast();

  const substituteSearch = useSearch(substituteQuery);
  const substituteResults = (substituteSearch.data ?? []).filter(
    (hit) => hit.part_id !== item.part_id && !substitutes.includes(hit.part_id),
  );

  const updateBomItem = useUpdateBomItem();
  const setBomSubstitutes = useSetBomSubstitutes();

  const substitutesChanged =
    substitutes.length !== item.substitutes.length ||
    substitutes.some((id) => !item.substitutes.includes(id));

  function addSubstitute(partId: PartId) {
    setSubstitutes((current) => (current.includes(partId) ? current : [...current, partId]));
    setSubstituteQuery('');
  }

  function removeSubstitute(partId: PartId) {
    setSubstitutes((current) => current.filter((id) => id !== partId));
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (quantityPerBuild === '' || quantityPerBuild <= 0) return;
    setSubmitError(null);
    try {
      await updateBomItem.mutateAsync({
        projectId,
        id: item.id,
        draft: {
          part_id: item.part_id,
          quantity_per_build: Math.round(quantityPerBuild * 1000),
          reference_designators: refDes.trim(),
          required,
          notes: notes.trim(),
        },
      });
      if (substitutesChanged) {
        await setBomSubstitutes.mutateAsync({
          projectId,
          bomItemId: item.id,
          partIds: substitutes,
        });
      }
      toast({ title: `Updated ${item.part_display_name}`, kind: 'success' });
      onClose();
    } catch (error) {
      const commandError = error as CommandError;
      setSubmitError(commandError);
      toast({
        title: 'Could not update BOM line',
        description: errorHint(commandError.code) ?? commandError.message,
        kind: 'error',
      });
    }
  }

  const pending = updateBomItem.isPending || setBomSubstitutes.isPending;

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="bom-table-dialog-overlay" />
        <Dialog.Content className="bom-table-dialog-content">
          <Dialog.Title className="bom-table-dialog-title">Edit BOM line</Dialog.Title>
          <Dialog.Description className="bom-table-dialog-description">
            {item.part_display_name}
          </Dialog.Description>
          <form className="bom-table-form" onSubmit={handleSubmit}>
            <NumberField
              label="Quantity per build"
              value={quantityPerBuild}
              onChange={setQuantityPerBuild}
              min={0}
              step={0.001}
              required
              autoFocus
              disabled={pending}
            />
            <TextField
              label="Reference designators"
              value={refDes}
              onChange={setRefDes}
              disabled={pending}
            />
            <CheckboxField
              label="Required"
              checked={required}
              onChange={setRequired}
              disabled={pending}
            />
            <TextField label="Notes" value={notes} onChange={setNotes} disabled={pending} />

            <div className="bom-table-substitutes">
              <p className="bom-table-substitutes-label">Substitutes</p>
              {substitutes.length > 0 ? (
                <ul className="bom-table-substitutes-list">
                  {substitutes.map((partId) => (
                    <SubstituteChip
                      key={partId}
                      partId={partId}
                      onRemove={() => removeSubstitute(partId)}
                    />
                  ))}
                </ul>
              ) : (
                <p className="bom-table-substitutes-empty">No substitutes set.</p>
              )}
              <Command
                shouldFilter={false}
                label="Search substitute parts"
                className="bom-table-search"
              >
                <Command.Input
                  value={substituteQuery}
                  onValueChange={setSubstituteQuery}
                  placeholder="Add a substitute part…"
                  className="bom-table-search-input"
                  disabled={pending}
                />
                {substituteQuery.trim() ? (
                  <Command.List className="bom-table-search-list">
                    <Command.Empty className="bom-table-search-empty">
                      No parts match.
                    </Command.Empty>
                    {substituteResults.map((hit) => (
                      <Command.Item
                        key={hit.part_id}
                        value={hit.part_id}
                        onSelect={() => addSubstitute(hit.part_id)}
                        className="bom-table-search-item"
                      >
                        <span className="bom-table-search-item-name">{hit.display_name}</span>
                      </Command.Item>
                    ))}
                  </Command.List>
                ) : null}
              </Command>
            </div>

            {submitError ? (
              <p className="bom-table-dialog-error">
                {errorHint(submitError.code) ?? submitError.message}
              </p>
            ) : null}
            <div className="bom-table-dialog-buttons">
              <button
                type="button"
                className="bom-table-dialog-cancel"
                onClick={onClose}
                disabled={pending}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="bom-table-dialog-submit"
                disabled={pending || quantityPerBuild === '' || quantityPerBuild <= 0}
              >
                {pending ? 'Saving…' : 'Save'}
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface SubstituteChipProps {
  partId: PartId;
  onRemove: () => void;
}

/** A substitute part's display name isn't carried by `BomItemRecord.
 * substitutes` (just the bare `PartId[]`), so each chip resolves its own
 * name via `usePart` — fine at the scale a substitutes list actually
 * reaches (a handful of alternates per line, not hundreds). */
function SubstituteChip({ partId, onRemove }: SubstituteChipProps) {
  const partQuery = usePart(partId);
  const label = partQuery.data?.display_name ?? partId;
  return (
    <li className="bom-table-substitute-chip">
      <span className="bom-table-substitute-chip-name">{label}</span>
      <button
        type="button"
        className="bom-table-substitute-chip-remove"
        onClick={onRemove}
        aria-label={`Remove substitute ${label}`}
      >
        ×
      </button>
    </li>
  );
}

interface RemoveBomItemDialogProps {
  projectId: ProjectId;
  item: BomItemRecord;
  onClose: () => void;
}

function RemoveBomItemDialog({ projectId, item, onClose }: RemoveBomItemDialogProps) {
  const { toast } = useToast();
  const removeBomItem = useRemoveBomItem({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not remove BOM line',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: `Removed ${item.part_display_name} from the BOM`, kind: 'success' });
      onClose();
    },
  });

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="bom-table-dialog-overlay" />
        <Dialog.Content className="bom-table-dialog-content">
          <Dialog.Title className="bom-table-dialog-title">Remove BOM line</Dialog.Title>
          <Dialog.Description className="bom-table-dialog-description">
            Remove {item.part_display_name} from the BOM?
          </Dialog.Description>
          <div className="bom-table-dialog-buttons">
            <button
              type="button"
              className="bom-table-dialog-cancel"
              onClick={onClose}
              disabled={removeBomItem.isPending}
            >
              Cancel
            </button>
            <button
              type="button"
              className="bom-table-dialog-submit bom-table-dialog-submit--danger"
              onClick={() => removeBomItem.mutate({ projectId, id: item.id })}
              disabled={removeBomItem.isPending}
            >
              {removeBomItem.isPending ? 'Removing…' : 'Remove'}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
