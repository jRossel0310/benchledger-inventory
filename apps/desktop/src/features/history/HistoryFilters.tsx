/**
 * The History screen's filter bar (Phase 3 Task 9): date range, transaction
 * type, part (a compact search-select over `useSearch`, the same command
 * the Ctrl+K palette uses), project, and — when the screen was reached via a
 * group deep link — a "viewing one group" chip with a way back to the full
 * list. Every field is optional and AND-composed, matching the backend's
 * `HistoryFilter` (see `crates/inventory-db/src/history.rs`); this component
 * only manages the on-screen controls; `History.tsx` owns the actual query.
 */

import { useState } from 'react';

import type { GroupId, PartId, ProjectId } from '../../bindings.gen';
import { DateField, SelectField, TextField } from '../../components/Field';
import { useProjects, useSearch as usePartLookup } from '../../hooks/inventory';
import { formatTxnType } from '../../lib/format';
import './History.css';

/** Every transaction type the ledger schema allows (the CHECK constraint on
 * `transactions.txn_type`, `crates/inventory-db/migrations/0002_inventory_schema.sql`),
 * in the same order that migration lists them — the type filter's option
 * list, so a new ledger op type only needs adding here to stay in sync. */
export const TXN_TYPES = [
  'receive',
  'reserve',
  'release_reservation',
  'check_out',
  'return',
  'consume_available',
  'consume_reserved',
  'consume_checked_out',
  'adjust_up',
  'adjust_down',
  'transfer_reservation',
  'reverse',
] as const;

export interface HistoryFilterState {
  dateFrom: string;
  dateTo: string;
  txnType: string;
  partId: PartId | null;
  partLabel: string;
  projectId: ProjectId | null;
  /** Set only via a deep link (e.g. a future import's "view this batch"
   * link) — there is no free-text group-id input; ULIDs aren't something a
   * person would type. Cleared via the "View all" chip below. */
  groupId: GroupId | null;
}

export const EMPTY_HISTORY_FILTER_STATE: HistoryFilterState = {
  dateFrom: '',
  dateTo: '',
  txnType: '',
  partId: null,
  partLabel: '',
  projectId: null,
  groupId: null,
};

export interface HistoryFiltersProps {
  value: HistoryFilterState;
  onChange: (next: HistoryFilterState) => void;
}

export function HistoryFilters({ value, onChange }: HistoryFiltersProps) {
  const projectsQuery = useProjects();
  const projectOptions = [
    { value: '', label: 'All projects' },
    ...(projectsQuery.data ?? []).map((p) => ({ value: p.id, label: p.name })),
  ];
  const typeOptions = [
    { value: '', label: 'All types' },
    ...TXN_TYPES.map((t) => ({ value: t, label: formatTxnType(t) })),
  ];

  return (
    <div className="history-filters">
      <DateField
        label="From"
        value={value.dateFrom}
        onChange={(dateFrom) => onChange({ ...value, dateFrom })}
      />
      <DateField
        label="To"
        value={value.dateTo}
        onChange={(dateTo) => onChange({ ...value, dateTo })}
      />
      <SelectField
        label="Type"
        value={value.txnType}
        onChange={(txnType) => onChange({ ...value, txnType })}
        options={typeOptions}
      />
      <PartFilterField
        partId={value.partId}
        partLabel={value.partLabel}
        onChange={(partId, partLabel) => onChange({ ...value, partId, partLabel })}
      />
      <SelectField
        label="Project"
        value={value.projectId ?? ''}
        onChange={(next) => onChange({ ...value, projectId: next === '' ? null : next })}
        options={projectOptions}
      />
      {value.groupId ? (
        <div className="history-filters-group-chip">
          <span className="history-filters-group-chip-label">Viewing one group</span>
          <button
            type="button"
            className="history-filters-group-chip-clear"
            onClick={() => onChange({ ...value, groupId: null })}
          >
            View all
          </button>
        </div>
      ) : null}
    </div>
  );
}

interface PartFilterFieldProps {
  partId: PartId | null;
  partLabel: string;
  onChange: (partId: PartId | null, partLabel: string) => void;
}

/** A compact search-select over parts: typing narrows via the same `search`
 * command the Ctrl+K palette uses (`useSearch`, disabled for a blank query);
 * picking a suggestion locks in `partId` and shows its name in the (now
 * read-only-feeling) box with a clear control, rather than a free-text
 * filter that could silently match nothing. */
function PartFilterField({ partId, partLabel, onChange }: PartFilterFieldProps) {
  const [query, setQuery] = useState('');
  const lookup = usePartLookup(query);
  const suggestions = partId === null ? (lookup.data ?? []).slice(0, 8) : [];

  return (
    <div className="history-filters-part">
      <TextField
        label="Part"
        value={partId !== null ? partLabel : query}
        onChange={(next) => {
          if (partId !== null) {
            // Typing again after a part was picked starts a fresh search.
            onChange(null, '');
          }
          setQuery(next);
        }}
        placeholder="Search parts…"
      />
      {partId !== null ? (
        <button
          type="button"
          className="history-filters-part-clear"
          aria-label="Clear part filter"
          onClick={() => {
            onChange(null, '');
            setQuery('');
          }}
        >
          ×
        </button>
      ) : null}
      {suggestions.length > 0 ? (
        <ul className="history-filters-part-suggestions" role="listbox">
          {suggestions.map((hit) => (
            <li key={hit.part_id}>
              <button
                type="button"
                className="history-filters-part-suggestion"
                onClick={() => {
                  onChange(hit.part_id, hit.display_name);
                  setQuery('');
                }}
              >
                {hit.display_name}
                <span className="history-filters-part-suggestion-category">
                  {hit.category_name}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
