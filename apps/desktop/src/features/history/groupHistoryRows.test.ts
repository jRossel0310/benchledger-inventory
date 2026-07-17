import { describe, expect, it } from 'vitest';

import type { HistoryRow } from '../../bindings.gen';
import { groupHistoryRows } from './groupHistoryRows';

function row(overrides: Partial<HistoryRow> = {}): HistoryRow {
  return {
    id: 't1',
    part_id: 'p1',
    display_name: 'Part',
    quantity_unit: 'each',
    part_archived: false,
    txn_type: 'receive',
    quantity: 1000,
    from_state: null,
    to_state: null,
    project_id: null,
    to_project_id: null,
    project_name: null,
    note: '',
    group_id: null,
    group_kind: null,
    group_total: 0,
    reversed_txn_id: null,
    import_id: null,
    created_at: '2026-07-01 00:00:00',
    reversible: true,
    ...overrides,
  };
}

describe('groupHistoryRows', () => {
  it('returns an empty list for no rows', () => {
    expect(groupHistoryRows([])).toEqual([]);
  });

  it('renders ungrouped rows as flat entries in order', () => {
    const rows = [row({ id: 't1' }), row({ id: 't2' })];
    const entries = groupHistoryRows(rows);
    expect(entries).toEqual([
      { kind: 'row', row: rows[0] },
      { kind: 'row', row: rows[1] },
    ]);
  });

  it('clusters consecutive rows sharing a group_id into one group entry', () => {
    const rows = [
      row({ id: 't1', group_id: 'g1', group_kind: 'receive_batch' }),
      row({ id: 't2', group_id: 'g1', group_kind: 'receive_batch' }),
    ];
    const entries = groupHistoryRows(rows);
    expect(entries).toEqual([
      { kind: 'group', groupId: 'g1', groupKind: 'receive_batch', members: rows },
    ]);
  });

  it('keeps flat rows and groups interleaved in their original order', () => {
    const flatBefore = row({ id: 't0' });
    const member1 = row({ id: 't1', group_id: 'g1', group_kind: 'receive_batch' });
    const member2 = row({ id: 't2', group_id: 'g1', group_kind: 'receive_batch' });
    const flatAfter = row({ id: 't3' });
    const entries = groupHistoryRows([flatBefore, member1, member2, flatAfter]);
    expect(entries).toEqual([
      { kind: 'row', row: flatBefore },
      { kind: 'group', groupId: 'g1', groupKind: 'receive_batch', members: [member1, member2] },
      { kind: 'row', row: flatAfter },
    ]);
  });

  it('starts a new group entry when the group_id changes, even back-to-back', () => {
    const a = row({ id: 't1', group_id: 'g1', group_kind: 'receive_batch' });
    const b = row({ id: 't2', group_id: 'g2', group_kind: 'reserve_bom' });
    const entries = groupHistoryRows([a, b]);
    expect(entries).toEqual([
      { kind: 'group', groupId: 'g1', groupKind: 'receive_batch', members: [a] },
      { kind: 'group', groupId: 'g2', groupKind: 'reserve_bom', members: [b] },
    ]);
  });

  it('falls back to an empty group kind label when group_kind is missing', () => {
    const a = row({ id: 't1', group_id: 'g1', group_kind: null });
    const entries = groupHistoryRows([a]);
    expect(entries).toEqual([{ kind: 'group', groupId: 'g1', groupKind: '', members: [a] }]);
  });
});
