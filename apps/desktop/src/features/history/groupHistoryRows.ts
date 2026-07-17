/**
 * Clusters a page of `HistoryRow`s (already newest-first via `list_history`'s
 * `ORDER BY t.rowid DESC`) into flat rows and grouped runs, so the History
 * screen (Phase 3 Task 9) can render every member of a transaction group
 * together under one expandable header instead of as separate flat rows.
 *
 * This works without a further "group rollup" query because
 * `apply_group`/`reverse_group` insert every member of a group consecutively
 * within one database transaction (see `crates/inventory-db/src/ledger.rs`),
 * so members of the same group always have contiguous `rowid`s — see
 * `crates/inventory-db/src/history.rs`'s module doc and
 * `crates/inventory-db/tests/history.rs`'s
 * `group_rollup_reports_kind_and_keeps_members_contiguous`. A group can in
 * principle straddle a page boundary if the page size doesn't align with a
 * group's member count; this function only clusters what's on the current
 * page, matching that documented, accepted limitation.
 */

import type { GroupId, HistoryRow } from '../../bindings.gen';

export type HistoryEntry =
  | { kind: 'row'; row: HistoryRow }
  | { kind: 'group'; groupId: GroupId; groupKind: string; members: HistoryRow[] };

export function groupHistoryRows(rows: HistoryRow[]): HistoryEntry[] {
  const entries: HistoryEntry[] = [];
  let i = 0;
  while (i < rows.length) {
    const row = rows[i];
    if (row === undefined) break;
    if (row.group_id === null) {
      entries.push({ kind: 'row', row });
      i += 1;
      continue;
    }
    const groupId = row.group_id;
    const members: HistoryRow[] = [row];
    let j = i + 1;
    let next = rows[j];
    while (next !== undefined && next.group_id === groupId) {
      members.push(next);
      j += 1;
      next = rows[j];
    }
    entries.push({ kind: 'group', groupId, groupKind: row.group_kind ?? '', members });
    i = j;
  }
  return entries;
}
