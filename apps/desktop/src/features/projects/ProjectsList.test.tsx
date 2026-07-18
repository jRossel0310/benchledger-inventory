import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
  useParams,
} from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listProjectsFull: vi.fn(),
      listBom: vi.fn(),
      createProjectFull: vi.fn(),
    },
  };
});

import type { BomItemRecord, ProjectRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { ProjectsList } from './ProjectsList';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

let offsetHeightSpy: ReturnType<typeof vi.spyOn>;
let offsetWidthSpy: ReturnType<typeof vi.spyOn>;

beforeAll(() => {
  offsetHeightSpy = vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(400);
  offsetWidthSpy = vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(1000);
});

afterAll(() => {
  offsetHeightSpy.mockRestore();
  offsetWidthSpy.mockRestore();
});

beforeEach(() => {
  window.scrollTo = vi.fn();
  vi.resetAllMocks();
});

afterEach(cleanup);

function project(overrides: Partial<ProjectRecord> = {}): ProjectRecord {
  return {
    id: 'proj1',
    name: 'Blinky Board',
    status: 'active',
    description: 'ATmega328P LED blinker',
    build_quantity: 5,
    repo_link: null,
    notes: '',
    created_at: '2026-01-01 00:00:00',
    completed_at: null,
    ...overrides,
  };
}

function bomItem(overrides: Partial<BomItemRecord> = {}): BomItemRecord {
  return {
    id: 'b1',
    project_id: 'proj1',
    part_id: 'p1',
    part_display_name: '10k 0603 1% resistor',
    quantity_per_build: 10_000,
    total_required: 50_000,
    reference_designators: 'R1-R10',
    required: true,
    notes: '',
    substitutes: [],
    reserved: 50_000,
    consumed: 0,
    available: 100_000,
    missing: 0,
    ...overrides,
  };
}

function renderProjectsList() {
  const rootRoute = createRootRoute();
  const projectsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/projects',
    component: ProjectsList,
  });
  const projectDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/projects/$projectId',
    component: () => {
      const { projectId } = useParams({ from: '/projects/$projectId' });
      return <div>Project detail stub: {projectId}</div>;
    },
  });
  const routeTree = rootRoute.addChildren([projectsRoute, projectDetailRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/projects'] }),
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('ProjectsList', () => {
  it('renders every project with its status chip, build quantity, and BOM line count', async () => {
    vi.mocked(commands.listProjectsFull).mockReturnValue(
      ok([
        project({ id: 'proj1', name: 'Blinky Board', status: 'active', build_quantity: 5 }),
        project({ id: 'proj2', name: 'Bench PSU Rebuild', status: 'planned', build_quantity: 1 }),
      ]),
    );
    vi.mocked(commands.listBom).mockImplementation((projectId: string) =>
      ok(projectId === 'proj1' ? [bomItem(), bomItem({ id: 'b2' })] : []),
    );

    renderProjectsList();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    expect(commands.listProjectsFull).toHaveBeenCalledWith(null);
    const grid = within(screen.getByRole('grid', { name: 'Projects' }));
    expect(grid.getByText('Bench PSU Rebuild')).toBeTruthy();
    expect(grid.getByText('Active')).toBeTruthy();
    expect(grid.getByText('Planned')).toBeTruthy();
    expect(grid.getByText('5')).toBeTruthy();
    await waitFor(() => expect(grid.getByText('2')).toBeTruthy());
  });

  it('shows the empty state when there are no projects', async () => {
    vi.mocked(commands.listProjectsFull).mockReturnValue(ok([]));
    renderProjectsList();

    await waitFor(() =>
      expect(
        screen.getByText('No projects yet — create one to start tracking a build.'),
      ).toBeTruthy(),
    );
  });

  it('filtering by status re-queries listProjectsFull with that status', async () => {
    vi.mocked(commands.listProjectsFull).mockReturnValue(ok([]));
    renderProjectsList();

    await waitFor(() => expect(commands.listProjectsFull).toHaveBeenCalledWith(null));

    fireEvent.click(screen.getByRole('tab', { name: 'Planned' }));
    await waitFor(() => expect(commands.listProjectsFull).toHaveBeenCalledWith('planned'));

    fireEvent.click(screen.getByRole('tab', { name: 'Archived' }));
    await waitFor(() => expect(commands.listProjectsFull).toHaveBeenCalledWith('archived'));
  });

  it('activating a row navigates to that project detail route', async () => {
    vi.mocked(commands.listProjectsFull).mockReturnValue(ok([project()]));
    vi.mocked(commands.listBom).mockReturnValue(ok([]));
    renderProjectsList();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    fireEvent.click(screen.getByText('Blinky Board'));

    await waitFor(() => expect(screen.getByText('Project detail stub: proj1')).toBeTruthy());
  });

  it('creating a project calls createProjectFull and navigates to the new project', async () => {
    vi.mocked(commands.listProjectsFull).mockReturnValue(ok([]));
    vi.mocked(commands.createProjectFull).mockReturnValue(
      ok(project({ id: 'proj9', name: 'New Widget' })),
    );
    renderProjectsList();

    await waitFor(() =>
      expect(
        screen.getByText('No projects yet — create one to start tracking a build.'),
      ).toBeTruthy(),
    );

    fireEvent.click(screen.getByRole('button', { name: 'New project' }));
    const region = screen.getByRole('region', { name: 'New project' });
    fireEvent.change(within(region).getByLabelText('Name'), {
      target: { value: 'New Widget' },
    });
    fireEvent.click(within(region).getByRole('button', { name: 'Create project' }));

    await waitFor(() =>
      expect(commands.createProjectFull).toHaveBeenCalledWith(
        expect.objectContaining({ name: 'New Widget', build_quantity: 1 }),
      ),
    );
    await waitFor(() => expect(screen.getByText('Project detail stub: proj9')).toBeTruthy());
  });
});
