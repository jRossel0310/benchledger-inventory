/**
 * Part detail's Metadata tab (Phase 3 Task 7): whole-record completeness and
 * timestamps. Nothing in the current schema tracks *per-field* provenance
 * (where each individual value came from, or a confidence score) — that's a
 * Phase 5 enrichment concern this screen must not invent ahead of the
 * backend actually providing it (see the "Refresh product data" stub in
 * `PartDetail.tsx`). `metadata_complete` is the one completeness signal the
 * backend does compute today, so this tab shows exactly that, plus the
 * record's own `created_at`/`modified_at`/`archived` fields — never a
 * fabricated per-field source list.
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
        Per-field provenance (where each value came from) isn&apos;t tracked yet — this is a
        whole-record completeness flag, not a source list.
      </p>
    </div>
  );
}
