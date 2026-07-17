/**
 * Route tree. Code-based (not file-based) routing: every screen is a
 * "titled panel" stub for now — Tasks 3-9 of the Phase 3 plan fill in the
 * real feature components behind these same routes.
 */

import { createRootRoute, createRoute } from '@tanstack/react-router';

import { BinsPage } from '../features/bins/BinsPage';
import { DashboardPage } from '../features/dashboard/DashboardPage';
import { HistoryPage } from '../features/history/HistoryPage';
import { CreatePartPage } from '../features/inventory/CreatePartPage';
import { EditPartPage } from '../features/inventory/EditPartPage';
import { InventoryPage } from '../features/inventory/InventoryPage';
import { OrdersPage } from '../features/orders/OrdersPage';
import { PartDetailPage } from '../features/part/PartDetailPage';
import { ProjectsPage } from '../features/projects/ProjectsPage';
import { SettingsPage } from '../features/settings/SettingsPage';
import { AppShell } from './AppShell';

export const rootRoute = createRootRoute({ component: AppShell });

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: DashboardPage,
});

/** The Inventory browser's search-query state lives entirely in the `q`
 * route search param (Phase 3 Task 4) — the top command-bar search, the
 * screen's own search box, filter chips, and saved views all read/write
 * this one string. Coerced to `''` (never `undefined`) for any missing/
 * non-string `q` so every consumer can treat it as a plain string. */
export interface InventorySearch {
  q: string;
}

function validateInventorySearch(search: Record<string, unknown>): InventorySearch {
  return { q: typeof search.q === 'string' ? search.q : '' };
}

const inventoryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/inventory',
  component: InventoryPage,
  validateSearch: validateInventorySearch,
});

/** Static path, registered alongside the `$partId` param route below — the
 * Ctrl+K palette's "Create part" action routes here (see
 * `CommandPalette.tsx`). TanStack Router prefers a static segment match over
 * a param match at the same depth, so `/inventory/new` never gets swallowed
 * by `/inventory/$partId`. */
const newPartRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/inventory/new',
  component: CreatePartPage,
});

const partDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/inventory/$partId',
  component: PartDetailPage,
});

/** `/inventory/$partId/edit` — the category-adaptive part form in edit mode
 * (Phase 3 Task 6), reusing `PartForm`. Deeper than `/inventory/$partId`, so
 * the `edit` segment matches here rather than being read as a nested param. */
const editPartRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/inventory/$partId/edit',
  component: EditPartPage,
});

const binsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/bins',
  component: BinsPage,
});

const historyRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/history',
  component: HistoryPage,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/settings',
  component: SettingsPage,
});

const projectsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/projects',
  component: ProjectsPage,
});

const ordersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/orders',
  component: OrdersPage,
});

export const routeTree = rootRoute.addChildren([
  indexRoute,
  inventoryRoute,
  newPartRoute,
  partDetailRoute,
  editPartRoute,
  binsRoute,
  historyRoute,
  settingsRoute,
  projectsRoute,
  ordersRoute,
]);
