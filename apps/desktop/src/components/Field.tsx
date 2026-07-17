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
