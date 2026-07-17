/**
 * Labeled input primitives shared by every form the later Phase 3 screens
 * build (part create/edit, dimensions, variants, ...): `TextField`,
 * `NumberField`, `SelectField`. Token-styled, `--font-ui` labels, a visible
 * focus ring (`shell.css`'s `:focus-visible` plus the input's own
 * `focus-visible` border).
 */

import { useId, type ChangeEvent, type KeyboardEvent, type ReactNode } from 'react';

import './Field.css';

interface FieldShellProps {
  label: string;
  hint?: string;
  error?: string | null;
  htmlFor: string;
  children: ReactNode;
}

function FieldShell({ label, hint, error, htmlFor, children }: FieldShellProps) {
  return (
    <div className="field">
      <label className="field-label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {error ? (
        <p className="field-error">{error}</p>
      ) : hint ? (
        <p className="field-hint">{hint}</p>
      ) : null}
    </div>
  );
}

export interface TextFieldProps {
  label: string;
  hint?: string;
  error?: string | null;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  required?: boolean;
  autoFocus?: boolean;
  /** Escape hatch for callers that need to intercept a key before it reaches
   * an enclosing `<form>` — e.g. the QuickAction dialog's inline "create a
   * project" field, which must not submit the outer ledger-op form on
   * Enter. */
  onKeyDown?: (event: KeyboardEvent<HTMLInputElement>) => void;
}

export function TextField({
  label,
  hint,
  error,
  value,
  onChange,
  placeholder,
  disabled,
  required,
  autoFocus,
  onKeyDown,
}: TextFieldProps) {
  const id = useId();
  return (
    <FieldShell label={label} hint={hint} error={error} htmlFor={id}>
      <input
        id={id}
        type="text"
        className="field-input"
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        required={required}
        autoFocus={autoFocus}
        onKeyDown={onKeyDown}
        onChange={(event: ChangeEvent<HTMLInputElement>) => onChange(event.target.value)}
      />
    </FieldShell>
  );
}

export interface NumberFieldProps {
  label: string;
  hint?: string;
  error?: string | null;
  /** `''` represents "empty" (distinct from `0`) so a cleared field doesn't
   * silently become zero. */
  value: number | '';
  onChange: (value: number | '') => void;
  min?: number;
  max?: number;
  step?: number;
  disabled?: boolean;
  required?: boolean;
  autoFocus?: boolean;
}

export function NumberField({
  label,
  hint,
  error,
  value,
  onChange,
  min,
  max,
  step,
  disabled,
  required,
  autoFocus,
}: NumberFieldProps) {
  const id = useId();
  return (
    <FieldShell label={label} hint={hint} error={error} htmlFor={id}>
      <input
        id={id}
        type="number"
        className="field-input field-input-mono"
        value={value}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        required={required}
        autoFocus={autoFocus}
        onChange={(event: ChangeEvent<HTMLInputElement>) => {
          const raw = event.target.value;
          onChange(raw === '' ? '' : Number(raw));
        }}
      />
    </FieldShell>
  );
}

export interface SelectFieldOption {
  value: string;
  label: string;
}

export interface SelectFieldProps {
  label: string;
  hint?: string;
  error?: string | null;
  value: string;
  onChange: (value: string) => void;
  options: SelectFieldOption[];
  disabled?: boolean;
  required?: boolean;
}

export function SelectField({
  label,
  hint,
  error,
  value,
  onChange,
  options,
  disabled,
  required,
}: SelectFieldProps) {
  const id = useId();
  return (
    <FieldShell label={label} hint={hint} error={error} htmlFor={id}>
      <select
        id={id}
        className="field-input"
        value={value}
        disabled={disabled}
        required={required}
        onChange={(event: ChangeEvent<HTMLSelectElement>) => onChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </FieldShell>
  );
}

export interface CheckboxFieldProps {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}

/** A single labeled checkbox — `boolean`-typed attribute fields (the part
 * form's category-adaptive attributes, Phase 3 Task 6) and any future
 * on/off toggle. Label sits beside the box (not above, unlike the other
 * `Field*` components) since a checkbox's own visual weight already reads
 * as the input. */
export function CheckboxField({ label, hint, checked, onChange, disabled }: CheckboxFieldProps) {
  const id = useId();
  return (
    <div className="field field-checkbox">
      <label className="field-checkbox-label" htmlFor={id}>
        <input
          id={id}
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={(event: ChangeEvent<HTMLInputElement>) => onChange(event.target.checked)}
        />
        {label}
      </label>
      {hint ? <p className="field-hint">{hint}</p> : null}
    </div>
  );
}

export interface MultiSelectFieldProps {
  label: string;
  hint?: string;
  error?: string | null;
  /** Currently-checked option values. */
  values: string[];
  onChange: (values: string[]) => void;
  options: SelectFieldOption[];
  disabled?: boolean;
}

/** A checkbox group over a fixed option list — `multi_choice`-typed
 * attribute fields (the part form's category-adaptive attributes, Phase 3
 * Task 6), whose raw wire value is a comma-joined list `set_attribute`
 * splits back apart server-side. Renders every option as its own checkbox
 * (never a native multi-select `<select multiple>`, which hides the full
 * option list behind a scroll box and needs Ctrl/Cmd-click to select more
 * than one — poor for the short, fully-visible choice lists this covers). */
export function MultiSelectField({
  label,
  hint,
  error,
  values,
  onChange,
  options,
  disabled,
}: MultiSelectFieldProps) {
  const groupId = useId();
  function toggle(value: string, checked: boolean) {
    onChange(checked ? [...values, value] : values.filter((v) => v !== value));
  }
  return (
    <div className="field">
      <span id={groupId} className="field-label">
        {label}
      </span>
      <div className="field-multiselect" role="group" aria-labelledby={groupId}>
        {options.map((option) => (
          <label key={option.value} className="field-checkbox-label">
            <input
              type="checkbox"
              checked={values.includes(option.value)}
              disabled={disabled}
              onChange={(event: ChangeEvent<HTMLInputElement>) =>
                toggle(option.value, event.target.checked)
              }
            />
            {option.label}
          </label>
        ))}
      </div>
      {error ? (
        <p className="field-error">{error}</p>
      ) : hint ? (
        <p className="field-hint">{hint}</p>
      ) : null}
    </div>
  );
}
