/**
 * Part detail's Overview tab (Phase 3 Task 7): description, tags, usage
 * behavior, and notes. `public_notes` is always shown; `private_notes` is
 * shown too (nothing in this app is ever hidden from its own owner) but
 * flagged "local only" — the private-notes column never leaves this
 * database (no publish/export path touches it), so the badge is a factual
 * label, not a promise this screen enforces itself.
 */

import type { PartRecord } from '../../bindings.gen';
import { useTags } from '../../hooks/inventory';
import './PartDetail.css';

const USAGE_LABELS: Record<string, string> = {
  usually_consumed: 'Usually consumed',
  usually_checked_out: 'Usually checked out',
  ask: 'Ask each time',
};

export interface PartDetailOverviewProps {
  part: PartRecord;
}

export function PartDetailOverview({ part }: PartDetailOverviewProps) {
  const tagsQuery = useTags(part.id);
  const tags = tagsQuery.data ?? [];

  return (
    <div className="part-detail-overview">
      <section className="part-detail-section">
        <h3 className="part-detail-section-title">Description</h3>
        <p className="part-detail-section-body">
          {part.description.trim() ? part.description : 'No description yet.'}
        </p>
      </section>

      <section className="part-detail-section">
        <h3 className="part-detail-section-title">Tags</h3>
        {tags.length > 0 ? (
          <ul className="part-detail-tag-list">
            {tags.map((tag) => (
              <li key={tag} className="part-detail-tag">
                {tag}
              </li>
            ))}
          </ul>
        ) : (
          <p className="part-detail-section-body part-detail-muted">No tags.</p>
        )}
      </section>

      <section className="part-detail-section">
        <h3 className="part-detail-section-title">Usage behavior</h3>
        <p className="part-detail-section-body">
          {USAGE_LABELS[part.usage_behavior] ?? part.usage_behavior}
        </p>
      </section>

      <section className="part-detail-section">
        <h3 className="part-detail-section-title">Notes</h3>
        {part.public_notes.trim() ? (
          <p className="part-detail-section-body">{part.public_notes}</p>
        ) : (
          <p className="part-detail-section-body part-detail-muted">No public notes.</p>
        )}
        {part.private_notes.trim() ? (
          <div className="part-detail-private-note">
            <span className="part-detail-badge part-detail-badge-local">Local only</span>
            <p className="part-detail-section-body">{part.private_notes}</p>
          </div>
        ) : null}
      </section>
    </div>
  );
}
