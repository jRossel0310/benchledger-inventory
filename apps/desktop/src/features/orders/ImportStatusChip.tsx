/**
 * The import lifecycle status badge (Phase 5d Task 2, spec §10's
 * Upload -> Extract -> Match -> Review -> Confirm pipeline): parsed
 * (uploaded, not yet committed), committed (inventory received), reversed
 * (committed then undone). Mirrors `features/projects/StatusChip.tsx`'s
 * pattern exactly, but as its own component/token family — an import's
 * status is a third, unrelated vocabulary from both a project's lifecycle
 * status and a part's stock-state split (see `semantic.ts`'s
 * `color-import-*` token comment).
 *
 * `ImportRecord.status` crosses the IPC boundary as a plain `string` (no
 * generated literal union), so this renders any known value with its label/
 * token and falls back to the raw string (title-cased) for anything else
 * rather than silently dropping an unrecognized status.
 */

import './ImportStatusChip.css';

const KNOWN_STATUSES = ['parsed', 'committed', 'reversed'] as const;
type KnownImportStatus = (typeof KNOWN_STATUSES)[number];

const STATUS_LABELS: Record<KnownImportStatus, string> = {
  parsed: 'Parsed',
  committed: 'Committed',
  reversed: 'Reversed',
};

function isKnownStatus(status: string): status is KnownImportStatus {
  return (KNOWN_STATUSES as readonly string[]).includes(status);
}

function fallbackLabel(status: string): string {
  return status
    .split('_')
    .filter((word) => word.length > 0)
    .map((word) => `${word[0]?.toUpperCase() ?? ''}${word.slice(1)}`)
    .join(' ');
}

export interface ImportStatusChipProps {
  status: string;
}

export function ImportStatusChip({ status }: ImportStatusChipProps) {
  const known = isKnownStatus(status);
  const label = known ? STATUS_LABELS[status] : fallbackLabel(status);
  const modifier = known ? status : 'unknown';
  return <span className={`import-status-chip import-status-chip-${modifier}`}>{label}</span>;
}
