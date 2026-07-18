import { useParams } from '@tanstack/react-router';

import type { ProjectId } from '../../bindings.gen';
import { ProjectDetail } from './ProjectDetail';

/**
 * `/projects/$projectId` route wrapper — mirrors `PartDetailPage`'s split
 * between the routed page (reads the param) and the reusable body
 * (`ProjectDetail`, testable with a plain `projectId` prop).
 */
export function ProjectDetailPage() {
  const { projectId } = useParams({ from: '/projects/$projectId' });
  return <ProjectDetail projectId={projectId as ProjectId} />;
}
