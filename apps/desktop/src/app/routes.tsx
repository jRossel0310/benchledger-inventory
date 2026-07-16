/**
 * Route tree. Code-based (not file-based) routing: every screen is a
 * "titled panel" stub for now — Tasks 3-9 of the Phase 3 plan fill in the
 * real feature components behind these same routes.
 */

import { createRootRoute, createRoute } from '@tanstack/react-router';

import { BinsPage } from '../features/bins/BinsPage';
import { DashboardPage } from '../features/dashboard/DashboardPage';
import { HistoryPage } from '../features/history/HistoryPage';
import { InventoryPage } from '../features/inventory/InventoryPage';
import { PartDetailPage } from '../features/inventory/PartDetailPage';
import { OrdersPage } from '../features/orders/OrdersPage';
import { ProjectsPage } from '../features/projects/ProjectsPage';
import { SettingsPage } from '../features/settings/SettingsPage';
import { AppShell } from './AppShell';

export const rootRoute = createRootRoute({ component: AppShell });

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: DashboardPage,
});

const inventoryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/inventory',
  component: InventoryPage,
});

const partDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/inventory/$partId',
  component: PartDetailPage,
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
  partDetailRoute,
  binsRoute,
  historyRoute,
  settingsRoute,
  projectsRoute,
  ordersRoute,
]);
