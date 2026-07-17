/**
 * Add/remove structured dimensions (spec §9, §6): each row becomes one
 * `DimensionDraft` — `useAddDimension(partId, draft)` on save, one call per
 * row (no batched "add dimensions" command; see `PartForm.tsx`'s save
 * sequence doc comment for the non-atomic save this implies across every
 * section, not just this one).
 *
 * A row's `value` is free-form text (e.g. `"5mm"`, `"1.2 g"`) — the same
 * `raw_value` the backend re-parses and normalizes on `add_dimension`
 * (length -> mm, mass -> g); no live preview here (unlike `AttributeFields`'
 * `number_unit`), since a dimension has no `unit_kind` to normalize against
 * up front — the backend infers it from the value's own unit suffix.
 */

import { useId } from 'react';

import type { DimensionDraft, DimensionGroup, DimensionSource } from '../../bindings.gen';
import { SelectField, TextField, type SelectFieldOption } from '../../components/Field';
import './DimensionFields.css';

export interface DimensionEntry {
  group: DimensionGroup;
  name: string;
  rawValue: string;
  source: DimensionSource;
  notes: string;
  measuredDate: string;
  /** Set for a row loaded from an existing part in edit mode. Dimensions are
   * append-only in the backend (`add_dimension`; no update), so a loaded row
   * is shown read-only and skipped on save — only newly-added rows are
   * `add_dimension`-ed. Absent for a fresh, unsaved row. */
  existingId?: string;
}

export function emptyDimensionEntry(): DimensionEntry {
  return {
    group: 'overall',
    name: '',
    rawValue: '',
    source: 'measured',
    notes: '',
    measuredDate: '',
  };
}

/** `DimensionEntry` (this component's editable, all-string UI shape) ->
 * `DimensionDraft` (`useAddDimension`'s wire shape) — `PartForm`'s save
 * step calls this per row. A blank `measuredDate` becomes `null` (matching
 * `DimensionDraft.measured_date: string | null`, not an empty-string date). */
export function toDimensionDraft(entry: DimensionEntry): DimensionDraft {
  return {
    group: entry.group,
    name: entry.name.trim(),
    raw_value: entry.rawValue.trim(),
    source: entry.source,
    notes: entry.notes.trim(),
    measured_date: entry.measuredDate.trim() === '' ? null : entry.measuredDate.trim(),
  };
}

/** A row is worth saving once it has both a name and a value — an entirely
 * blank trailing row (the common state right after "+ Add dimension") is
 * silently skipped rather than sent to the backend as an empty dimension. */
export function isDimensionEntryFilled(entry: DimensionEntry): boolean {
  return entry.name.trim() !== '' && entry.rawValue.trim() !== '';
}

const GROUP_OPTIONS: SelectFieldOption[] = [
  { value: 'overall', label: 'Overall' },
  { value: 'body', label: 'Body' },
  { value: 'mounting', label: 'Mounting' },
  { value: 'custom', label: 'Custom' },
];

const SOURCE_OPTIONS: SelectFieldOption[] = [
  { value: 'manufacturer', label: 'Manufacturer' },
  { value: 'datasheet', label: 'Datasheet' },
  { value: 'supplier', label: 'Supplier' },
  { value: 'measured', label: 'Measured' },
  { value: 'estimated', label: 'Estimated' },
];

export interface DimensionFieldsProps {
  dimensions: DimensionEntry[];
  onChange: (next: DimensionEntry[]) => void;
  disabled?: boolean;
}

export function DimensionFields({ dimensions, onChange, disabled }: DimensionFieldsProps) {
  function updateRow(index: number, patch: Partial<DimensionEntry>) {
    onChange(dimensions.map((row, i) => (i === index ? { ...row, ...patch } : row)));
  }

  function removeRow(index: number) {
    onChange(dimensions.filter((_, i) => i !== index));
  }

  function addRow() {
    onChange([...dimensions, emptyDimensionEntry()]);
  }

  return (
    <div className="dimension-fields">
      {dimensions.map((row, index) => (
        <DimensionRow
          // Rows have no stable id until saved; index is fine for a
          // locally-edited list where rows are only ever appended/removed
          // in place, never reordered.
          key={index}
          row={row}
          disabled={disabled}
          onChange={(patch) => updateRow(index, patch)}
          onRemove={() => removeRow(index)}
        />
      ))}
      <button type="button" className="dimension-fields-add" onClick={addRow} disabled={disabled}>
        + Add dimension
      </button>
    </div>
  );
}

interface DimensionRowProps {
  row: DimensionEntry;
  onChange: (patch: Partial<DimensionEntry>) => void;
  onRemove: () => void;
  disabled?: boolean;
}

function DimensionRow({ row, onChange, onRemove, disabled }: DimensionRowProps) {
  const notesId = useId();
  const dateId = useId();
  // A loaded (persisted) dimension is append-only in the backend — shown
  // read-only rather than offering an edit/remove the command surface can't
  // honor.
  const isExisting = Boolean(row.existingId);
  const fieldsDisabled = disabled || isExisting;
  return (
    <div className="dimension-row">
      <TextField
        label="Name"
        value={row.name}
        onChange={(name) => onChange({ name })}
        placeholder="e.g. Length, Body height"
        disabled={fieldsDisabled}
      />
      <TextField
        label="Value"
        value={row.rawValue}
        onChange={(rawValue) => onChange({ rawValue })}
        placeholder="e.g. 5mm, 1.2g"
        disabled={fieldsDisabled}
      />
      <SelectField
        label="Group"
        value={row.group}
        onChange={(group) => onChange({ group: group as DimensionGroup })}
        options={GROUP_OPTIONS}
        disabled={fieldsDisabled}
      />
      <SelectField
        label="Source"
        value={row.source}
        onChange={(source) => onChange({ source: source as DimensionSource })}
        options={SOURCE_OPTIONS}
        disabled={fieldsDisabled}
      />
      <div className="field">
        <label className="field-label" htmlFor={dateId}>
          Measured date
        </label>
        <input
          id={dateId}
          type="date"
          className="field-input field-input-mono"
          value={row.measuredDate}
          disabled={fieldsDisabled}
          onChange={(event) => onChange({ measuredDate: event.target.value })}
        />
      </div>
      <div className="field">
        <label className="field-label" htmlFor={notesId}>
          Notes
        </label>
        <input
          id={notesId}
          type="text"
          className="field-input"
          value={row.notes}
          placeholder="Optional"
          disabled={fieldsDisabled}
          onChange={(event) => onChange({ notes: event.target.value })}
        />
      </div>
      {isExisting ? (
        <span className="dimension-row-saved">Saved</span>
      ) : (
        <button
          type="button"
          className="dimension-row-remove"
          onClick={onRemove}
          disabled={disabled}
          aria-label={`Remove ${row.name || 'dimension'}`}
        >
          Remove
        </button>
      )}
    </div>
  );
}
