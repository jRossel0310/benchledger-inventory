/**
 * A minimal chip input for the part form's tags (spec §9): type a tag and
 * press Enter (or comma) to add it as a chip; backspace on an empty input
 * removes the last chip; each chip has an × to remove it. The whole set is
 * saved via `set_tags` (which replaces, not appends) on form save.
 *
 * Deliberately tiny and dependency-free — tags are a flat `string[]` with no
 * autocomplete/known-tag source in this phase, so a full combobox would be
 * over-built. Duplicate and blank tags are rejected on add.
 */

import { useId, useState, type KeyboardEvent } from 'react';

import './TagInput.css';

export interface TagInputProps {
  tags: string[];
  onChange: (next: string[]) => void;
  disabled?: boolean;
}

export function TagInput({ tags, onChange, disabled }: TagInputProps) {
  const id = useId();
  const [draft, setDraft] = useState('');

  function commit() {
    const value = draft.trim();
    if (value === '') return;
    if (!tags.includes(value)) onChange([...tags, value]);
    setDraft('');
  }

  function remove(tag: string) {
    onChange(tags.filter((t) => t !== tag));
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Enter' || event.key === ',') {
      event.preventDefault();
      commit();
    } else if (event.key === 'Backspace' && draft === '' && tags.length > 0) {
      onChange(tags.slice(0, -1));
    }
  }

  return (
    <div className="field">
      <label className="field-label" htmlFor={id}>
        Tags
      </label>
      <div className="tag-input">
        {tags.map((tag) => (
          <span key={tag} className="tag-chip">
            {tag}
            <button
              type="button"
              className="tag-chip-remove"
              onClick={() => remove(tag)}
              disabled={disabled}
              aria-label={`Remove tag ${tag}`}
            >
              ×
            </button>
          </span>
        ))}
        <input
          id={id}
          type="text"
          className="tag-input-field"
          value={draft}
          disabled={disabled}
          placeholder={tags.length === 0 ? 'Add a tag…' : ''}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={handleKeyDown}
          onBlur={commit}
        />
      </div>
    </div>
  );
}
