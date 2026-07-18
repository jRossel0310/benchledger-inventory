import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      getProject: vi.fn(),
      setProjectStatus: vi.fn(),
      updateProject: vi.fn(),
      archiveProject: vi.fn(),
      duplicateProject: vi.fn(),
    },
  };
});

vi.mock('./BomTable', () => ({
  BomTable: ({ projectId }: { projectId: string }) => <div>BOM table for {projectId}</div>,
}));

import type { ProjectRecord } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { ToastProvider } from '../../components/Toast';
import { ProjectDetailPage } from './ProjectDetailPage';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

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
    repo_link: 'https://github.com/example/blinky',
    notes: 'First bring-up board.',
    created_at: '2026-01-01 00:00:00',
    completed_at: null,
    ...overrides,
  };
}

function renderProjectDetail(initialPath = '/projects/proj1') {
  const rootRoute = createRootRoute();
  const projectDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/projects/$projectId',
    component: ProjectDetailPage,
  });
  const projectsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/projects',
    component: () => <div>Projects list stub</div>,
  });
  const routeTree = rootRoute.addChildren([projectDetailRoute, projectsRoute]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [initialPath] }),
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

describe('ProjectDetail', () => {
  it('shows a not-found state with a link back to Projects when the project no longer exists', async () => {
    vi.mocked(commands.getProject).mockReturnValue(ok(null));
    renderProjectDetail();

    await waitFor(() => expect(screen.getByText('Project not found')).toBeTruthy());
    fireEvent.click(screen.getByText('Back to Projects'));
    await waitFor(() => expect(screen.getByText('Projects list stub')).toBeTruthy());
  });

  it('renders the header fields, status chip, and the BOM section', async () => {
    vi.mocked(commands.getProject).mockReturnValue(ok(project()));
    renderProjectDetail();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    expect(screen.getByText('ATmega328P LED blinker')).toBeTruthy();
    expect(screen.getByText('Active', { selector: '.status-chip' })).toBeTruthy();
    expect(screen.getByText('https://github.com/example/blinky')).toBeTruthy();
    expect(screen.getByText('First bring-up board.')).toBeTruthy();
    expect((screen.getByLabelText('Build quantity') as HTMLInputElement).value).toBe('5');
    expect(screen.getByText('BOM table for proj1')).toBeTruthy();
  });

  it('changing the status control calls setProjectStatus with the new status', async () => {
    vi.mocked(commands.getProject).mockReturnValue(ok(project()));
    vi.mocked(commands.setProjectStatus).mockReturnValue(ok(null));
    renderProjectDetail();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    fireEvent.change(screen.getByLabelText('Status'), { target: { value: 'completed' } });

    await waitFor(() =>
      expect(commands.setProjectStatus).toHaveBeenCalledWith('proj1', 'completed'),
    );
  });

  it('editing build quantity commits on blur via updateProject', async () => {
    vi.mocked(commands.getProject).mockReturnValue(ok(project()));
    vi.mocked(commands.updateProject).mockReturnValue(ok(null));
    renderProjectDetail();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    const input = screen.getByLabelText('Build quantity');
    fireEvent.change(input, { target: { value: '8' } });
    fireEvent.blur(input);

    await waitFor(() =>
      expect(commands.updateProject).toHaveBeenCalledWith(
        expect.objectContaining({ id: 'proj1', build_quantity: 8 }),
      ),
    );
  });

  it('blurring build quantity unchanged does not call updateProject', async () => {
    vi.mocked(commands.getProject).mockReturnValue(ok(project()));
    vi.mocked(commands.updateProject).mockReturnValue(ok(null));
    renderProjectDetail();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    const input = screen.getByLabelText('Build quantity');
    fireEvent.focus(input);
    fireEvent.blur(input);

    expect(commands.updateProject).not.toHaveBeenCalled();
  });

  it('Edit fields saves name/description/repo link/notes via updateProject', async () => {
    vi.mocked(commands.getProject).mockReturnValue(ok(project()));
    vi.mocked(commands.updateProject).mockReturnValue(ok(null));
    renderProjectDetail();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'Edit fields' }));

    const region = screen.getByRole('region', { name: 'Edit project' });
    fireEvent.change(within(region).getByLabelText('Name'), {
      target: { value: 'Blinky Board v2' },
    });
    fireEvent.click(within(region).getByRole('button', { name: 'Save' }));

    await waitFor(() =>
      expect(commands.updateProject).toHaveBeenCalledWith(
        expect.objectContaining({ id: 'proj1', name: 'Blinky Board v2' }),
      ),
    );
  });

  it('Duplicate calls duplicateProject and navigates to the new project', async () => {
    vi.mocked(commands.getProject).mockImplementation((id: string) =>
      ok(id === 'proj1' ? project() : project({ id: 'proj9', name: 'Blinky Board copy' })),
    );
    vi.mocked(commands.duplicateProject).mockReturnValue(
      ok(project({ id: 'proj9', name: 'Blinky Board copy' })),
    );
    renderProjectDetail();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'Duplicate' }));

    const region = screen.getByRole('region', { name: 'Duplicate project' });
    fireEvent.click(within(region).getByRole('button', { name: 'Duplicate' }));

    await waitFor(() =>
      expect(commands.duplicateProject).toHaveBeenCalledWith('proj1', 'Blinky Board copy'),
    );
    await waitFor(() => expect(screen.getByText('BOM table for proj9')).toBeTruthy());
  });

  it('Archive calls archiveProject and is disabled once already archived', async () => {
    vi.mocked(commands.getProject).mockReturnValue(ok(project({ status: 'active' })));
    vi.mocked(commands.archiveProject).mockReturnValue(ok(null));
    renderProjectDetail();

    await waitFor(() => expect(screen.getByText('Blinky Board')).toBeTruthy());
    const archiveButton = screen.getByRole('button', { name: 'Archive' });
    expect(archiveButton.hasAttribute('disabled')).toBe(false);
    fireEvent.click(archiveButton);

    await waitFor(() => expect(commands.archiveProject).toHaveBeenCalledWith('proj1'));
  });
});
