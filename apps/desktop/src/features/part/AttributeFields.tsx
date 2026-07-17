/**
 * Renders one typed field per category attribute (spec §9): the
 * category-adaptive heart of `PartForm.tsx`. Which widget a given attribute
 * gets is entirely data-driven off its `data_type` (`useCategoryAttributeDefs`,
 * `AttributeDefRow`) — text/number/boolean/choice/multi_choice/url are thin
 * wrappers over the shared `Field.tsx` primitives; `number_unit` gets its own
 * `NumberUnitField` below for the live normalized preview (`usePreviewUnitValue`,
 * design direction's "normalized spec values" instrument-readout idea);
 * `range` gets a two-bound `low..high` input matching the backend's
 * `split_range` wire format.
 *
 * `values` is a flat `Record<attribute key, raw text>` — exactly the shape
 * `set_attribute(partId, key, raw)` expects for every data type (including
 * the comma-joined `multi_choice` list and the `"low..high"` `range`
 * encoding), so `PartForm`'s save step can iterate it directly with no
 * further per-type translation. A key absent from `values` (or present with
 * an empty/whitespace string) means "not entered" and is skipped on save —
 * never written as a hollow empty attribute.
 *
 * Hidden defs (`hidden: true`, set via Settings' category editor — not this
 * task) are never rendered; defs are sorted by `display_order` then `key`
 * (redundant with the backend's own ordering, but keeps this component
 * correct even if a caller — e.g. a test — hands it defs out of order).
 */

import { useState } from 'react';

import type { AttributeDefRow } from '../../bindings.gen';
import {
  CheckboxField,
  MultiSelectField,
  NumberField,
  SelectField,
  TextField,
  type SelectFieldOption,
} from '../../components/Field';
import { usePreviewUnitValue } from '../../hooks/inventory';
import { useDebouncedCallback } from '../../hooks/useDebouncedCallback';
import './AttributeFields.css';

export interface AttributeFieldsProps {
  defs: AttributeDefRow[];
  values: Record<string, string>;
  onChange: (key: string, raw: string) => void;
  disabled?: boolean;
}

const PREVIEW_DEBOUNCE_MS = 300;

export function AttributeFields({ defs, values, onChange, disabled }: AttributeFieldsProps) {
  const visible = [...defs]
    .filter((def) => !def.hidden)
    .sort((a, b) => a.display_order - b.display_order || a.key.localeCompare(b.key));

  if (visible.length === 0) return null;

  return (
    <div className="attribute-fields">
      {visible.map((def) => (
        <AttributeField
          key={def.key}
          def={def}
          value={values[def.key] ?? ''}
          onChange={(raw) => onChange(def.key, raw)}
          disabled={disabled}
        />
      ))}
    </div>
  );
}

interface AttributeFieldProps {
  def: AttributeDefRow;
  value: string;
  onChange: (raw: string) => void;
  disabled?: boolean;
}

function AttributeField({ def, value, onChange, disabled }: AttributeFieldProps) {
  switch (def.data_type) {
    case 'number_unit':
      return <NumberUnitField def={def} value={value} onChange={onChange} disabled={disabled} />;
    case 'range':
      return <RangeField def={def} value={value} onChange={onChange} disabled={disabled} />;
    case 'number':
      return (
        <NumberField
          label={def.label}
          value={value === '' ? '' : Number(value)}
          onChange={(next) => onChange(next === '' ? '' : String(next))}
          disabled={disabled}
        />
      );
    case 'boolean':
      return (
        <CheckboxField
          label={def.label}
          checked={value === 'true'}
          onChange={(checked) => onChange(checked ? 'true' : 'false')}
          disabled={disabled}
        />
      );
    case 'choice': {
      const options: SelectFieldOption[] = [
        { value: '', label: '—' },
        ...def.choices.map((choice) => ({ value: choice, label: choice })),
      ];
      return (
        <SelectField
          label={def.label}
          value={value}
          onChange={onChange}
          options={options}
          disabled={disabled}
        />
      );
    }
    case 'multi_choice': {
      const options: SelectFieldOption[] = def.choices.map((choice) => ({
        value: choice,
        label: choice,
      }));
      const selected = value
        ? value
            .split(',')
            .map((v) => v.trim())
            .filter(Boolean)
        : [];
      return (
        <MultiSelectField
          label={def.label}
          values={selected}
          onChange={(next) => onChange(next.join(', '))}
          options={options}
          disabled={disabled}
        />
      );
    }
    case 'url':
      return (
        <TextField
          label={def.label}
          value={value}
          onChange={onChange}
          placeholder="https://…"
          disabled={disabled}
        />
      );
    case 'text':
    default:
      return <TextField label={def.label} value={value} onChange={onChange} disabled={disabled} />;
  }
}

/** `number_unit`: a text input (free-form entry like `"10k"` or
 * `"10000 ohm"` — never a plain `<input type="number">`, which can't
 * express SI-prefix/unit notation) plus a debounced live preview of the
 * canonical normalized form the backend would store, via the stateless
 * `preview_unit_value` command. The preview is debounced independently of
 * `onChange` (which fires on every keystroke, immediately, so the form's
 * save state and the `DuplicatePanel`'s candidate stay current) — only the
 * preview *request* waits `PREVIEW_DEBOUNCE_MS` after typing stops, so an
 * in-progress value like `"10"` doesn't fire a doomed request on every
 * character. An unparsable in-progress value shows a quiet fallback note
 * instead of an alarming error — the real validation happens on save. */
function NumberUnitField({ def, value, onChange, disabled }: AttributeFieldProps) {
  const [previewRaw, setPreviewRaw] = useState(value);
  const debouncedPreview = useDebouncedCallback(setPreviewRaw, PREVIEW_DEBOUNCE_MS);
  const preview = usePreviewUnitValue(def.unit_kind, previewRaw);

  function handleChange(next: string) {
    onChange(next);
    debouncedPreview.run(next);
  }

  return (
    <div className="field">
      <TextField label={def.label} value={value} onChange={handleChange} disabled={disabled} />
      {preview.data ? (
        <p className="attribute-field-preview">{preview.data}</p>
      ) : preview.isError && previewRaw.trim() ? (
        <p className="field-hint">Normalized on save</p>
      ) : null}
    </div>
  );
}

/** `range`: two bound inputs combined into the backend's `"low..high"` wire
 * format (`split_range`, `crates/inventory-db/src/attributes.rs`). Both
 * bounds share the def's `unit_kind`, so `"1V..2V"` and `"1..2"` (unit
 * implied by the field) are both valid entry — the backend re-parses each
 * bound independently on save. */
function RangeField({ def, value, onChange, disabled }: AttributeFieldProps) {
  const [low, high] = value.includes('..') ? value.split('..') : ['', ''];

  function emit(nextLow: string, nextHigh: string) {
    const bothEmpty = nextLow.trim() === '' && nextHigh.trim() === '';
    onChange(bothEmpty ? '' : `${nextLow}..${nextHigh}`);
  }

  return (
    <div className="attribute-range-field">
      <TextField
        label={`${def.label} (low)`}
        value={low ?? ''}
        onChange={(next) => emit(next, high ?? '')}
        disabled={disabled}
      />
      <TextField
        label={`${def.label} (high)`}
        value={high ?? ''}
        onChange={(next) => emit(low ?? '', next)}
        disabled={disabled}
      />
    </div>
  );
}
