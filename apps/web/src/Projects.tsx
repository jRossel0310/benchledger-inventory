/**
 * Read-only projects view: every project the snapshot publishes -- name,
 * lifecycle status chip (the desktop's `--color-status-*` token vocabulary,
 * restyled here since the web app shares no components), description,
 * build quantity, and the associated parts cross-linked to their detail
 * routes. The snapshot deliberately carries no project notes or repo links.
 */

import type { Snapshot, SnapshotProject } from '@ei/shared';
import './Projects.css';

const KNOWN_STATUSES = ['planned', 'active', 'completed', 'archived'] as const;
type KnownStatus = (typeof KNOWN_STATUSES)[number];

function isKnownStatus(status: string): status is KnownStatus {
  return (KNOWN_STATUSES as readonly string[]).includes(status);
}

function StatusChip({ status }: { status: string }) {
  const modifier = isKnownStatus(status) ? status : 'unknown';
  const label = status.charAt(0).toUpperCase() + status.slice(1);
  return <span className={`project-status project-status-${modifier}`}>{label}</span>;
}

function ProjectCard({ project, snapshot }: { project: SnapshotProject; snapshot: Snapshot }) {
  const parts = project.partIds.flatMap((id) => {
    const part = snapshot.parts.find((p) => p.id === id);
    return part === undefined ? [] : [part];
  });
  return (
    <section className="project-card">
      <h3 className="project-title-line">
        <span className="project-name">{project.name}</span>
        <StatusChip status={project.status} />
      </h3>
      {project.description !== '' && <p className="project-description">{project.description}</p>}
      {project.buildQuantity > 0 && (
        <p className="project-build">
          Build quantity: <span className="project-build-count">{project.buildQuantity}</span>
        </p>
      )}
      {parts.length > 0 ? (
        <ul className="project-parts">
          {parts.map((part) => (
            <li key={part.id} className="project-part">
              <a className="project-part-name" href={`#/part/${part.id}`}>
                {part.displayName}
              </a>
            </li>
          ))}
        </ul>
      ) : (
        <p className="project-none">No parts associated.</p>
      )}
    </section>
  );
}

export function Projects({ snapshot }: { snapshot: Snapshot }) {
  return (
    <div className="projects">
      <h2 className="projects-heading">Projects</h2>
      {snapshot.projects.length === 0 && (
        <p className="project-none">No projects in the published snapshot.</p>
      )}
      {snapshot.projects.map((project) => (
        <ProjectCard key={project.id} project={project} snapshot={snapshot} />
      ))}
    </div>
  );
}
