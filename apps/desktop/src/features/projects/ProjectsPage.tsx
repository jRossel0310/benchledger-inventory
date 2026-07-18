import { ProjectsList } from './ProjectsList';

/**
 * `/projects` route wrapper — mirrors `DashboardPage`/`BinsPage`'s split
 * between the routed page and its real content (Phase 4 Task 6 replaces the
 * Phase 3 stub that lived here).
 */
export function ProjectsPage() {
  return <ProjectsList />;
}
