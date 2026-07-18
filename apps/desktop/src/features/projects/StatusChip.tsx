/**
 * The project lifecycle status badge (Phase 4 Task 6), shared by
 * `ProjectsList` (a table column) and `ProjectDetail` (the header). Colors
 * come from the dedicated `color-status-*` tokens
 * (`packages/shared/src/tokens/semantic.ts`) — deliberately distinct from
 * the stock-state tokens (`color-stock-available`/`reserved`/`checked-out`/
 * `low`) a project's *lifecycle* status has nothing to do with: reusing
 * those would make "green" mean two unrelated things depending on which
 * screen you're looking at.
 */

import type { ProjectStatus } from '../../bindings.gen';
import './StatusChip.css';

const STATUS_LABELS: Record<ProjectStatus, string> = {
  planned: 'Planned',
  active: 'Active',
  completed: 'Completed',
  archived: 'Archived',
};

export interface StatusChipProps {
  status: ProjectStatus;
}

export function StatusChip({ status }: StatusChipProps) {
  return <span className={`status-chip status-chip-${status}`}>{STATUS_LABELS[status]}</span>;
}
