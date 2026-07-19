/**
 * Part detail's Metadata tab (Phase 3 Task 7): whole-record completeness and
 * timestamps. Since Phase 5c, the schema *does* track per-field provenance
 * (the `field_provenance` table, written by `apply_enrichment`) — but no
 * read command or UI surfaces it yet, so this tab must not invent a source
 * list ahead of that work. `metadata_complete` is the one completeness
 * signal already wired end to end, so this tab shows exactly that, plus the
 * record's own `created_at`/`modified_at`/`archived` fields.
 */

import type { PartRecord } from '../../bindings.gen';
import { formatTimestamp } from '../../lib/format';
import './PartDetail.css';

export interface PartDetailMetadataProps {
  part: PartRecord;
}

export function PartDetailMetadata({ part }: PartDetailMetadataProps) {
  return (
    <div className="part-detail-metadata">
      <dl className="part-detail-inline-fields part-detail-metadata-list">
        <div>
          <dt>Metadata complete</dt>
          <dd>{part.metadata_complete ? 'Yes' : 'No'}</dd>
        </div>
        <div>
          <dt>Archived</dt>
          <dd>{part.archived ? 'Yes' : 'No'}</dd>
        </div>
        <div>
          <dt>Created</dt>
          <dd className="part-detail-mono">{formatTimestamp(part.created_at)}</dd>
        </div>
        <div>
          <dt>Modified</dt>
          <dd className="part-detail-mono">{formatTimestamp(part.modified_at)}</dd>
        </div>
      </dl>
      <p className="part-detail-muted">
        Per-field provenance (where each value came from) is recorded by enrichment but not yet
        displayed here — this is a whole-record completeness flag, not a source list.
      </p>
    </div>
  );
}
