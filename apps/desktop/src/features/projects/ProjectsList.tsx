/**
 * The Projects list (Phase 4 Task 6, spec §"Projects and BOMs"): every
 * project (`useProjectsFull`), filterable by lifecycle status, each row
 * showing its status chip, build quantity, and BOM line count. A row
 * click/Enter navigates to `/projects/$projectId` (`ProjectDetail`); "New
 * project" opens a compact create form (`useCreateProjectFull`) and
 * navigates straight to the created project — the same "create, then land
 * on the thing you created" flow `CreatePartPage` uses.
 *
 * `ProjectRecord` has no denormalized BOM-line-count column, so each row
 * fetches its own `useBom(project.id)` to render one — a small per-row query
 * that's fine at this app's local, single-user scale (the same call
 * `DuplicatePanel` already makes per match candidate), and warms
 * `keys.bom(project.id)` for every visible row so opening one is instant.
 */

import { useNavigate } from '@tanstack/react-router';
import { useState, type FormEvent } from 'react';

import type { ProjectId, ProjectRecord, ProjectStatus } from '../../bindings.gen';
import { DataTable, type DataTableColumn } from '../../components/DataTable';
import { NumberField, TextField } from '../../components/Field';
import { useToast } from '../../components/Toast';
import { useBom, useCreateProjectFull, useProjectsFull } from '../../hooks/projects';
import { errorHint, errorMessage, type CommandError } from '../../lib/format';
import { StatusChip } from './StatusChip';
import './ProjectsList.css';

type StatusFilterValue = 'all' | ProjectStatus;

const STATUS_FILTERS: { value: StatusFilterValue; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'planned', label: 'Planned' },
  { value: 'active', label: 'Active' },
  { value: 'completed', label: 'Completed' },
  { value: 'archived', label: 'Archived' },
];

function BomLineCount({ projectId }: { projectId: ProjectId }) {
  const bomQuery = useBom(projectId);
  if (bomQuery.isPending) return <span>…</span>;
  if (bomQuery.isError) return <span>—</span>;
  return <span>{bomQuery.data.length}</span>;
}

const COLUMNS: DataTableColumn<ProjectRecord>[] = [
  {
    key: 'name',
    header: 'Name',
    width: 280,
    mono: true,
    render: (row) => row.name,
  },
  {
    key: 'status',
    header: 'Status',
    width: 110,
    render: (row) => <StatusChip status={row.status} />,
  },
  {
    key: 'build_quantity',
    header: 'Build qty',
    width: 90,
    mono: true,
    render: (row) => String(row.build_quantity),
  },
  {
    key: 'bom',
    header: 'BOM lines',
    width: 90,
    mono: true,
    render: (row) => <BomLineCount projectId={row.id} />,
  },
];

export function ProjectsList() {
  const [statusFilter, setStatusFilter] = useState<StatusFilterValue>('all');
  const [creating, setCreating] = useState(false);
  const navigate = useNavigate();
  const projectsQuery = useProjectsFull(statusFilter === 'all' ? undefined : statusFilter);

  function handleActivate(row: ProjectRecord) {
    void navigate({ to: '/projects/$projectId', params: { projectId: row.id } });
  }

  function handleCreated(id: ProjectId) {
    setCreating(false);
    void navigate({ to: '/projects/$projectId', params: { projectId: id } });
  }

  return (
    <section className="projects-list">
      <header className="projects-list-header">
        <div>
          <p className="projects-list-eyebrow">Projects</p>
          <h1 className="projects-list-title">Builds and their bills of materials</h1>
        </div>
        <button type="button" className="projects-list-new" onClick={() => setCreating(true)}>
          New project
        </button>
      </header>

      <div className="projects-list-filters" role="tablist" aria-label="Filter by status">
        {STATUS_FILTERS.map((f) => (
          <button
            key={f.value}
            type="button"
            role="tab"
            aria-selected={statusFilter === f.value}
            className={`projects-list-filter${
              statusFilter === f.value ? ' projects-list-filter-active' : ''
            }`}
            onClick={() => setStatusFilter(f.value)}
          >
            {f.label}
          </button>
        ))}
      </div>

      <div className="projects-list-table">
        {projectsQuery.isPending ? (
          <p className="projects-list-status">Loading projects…</p>
        ) : projectsQuery.isError ? (
          <p className="projects-list-status projects-list-status-error">
            Could not load projects: {errorMessage(projectsQuery.error)}
          </p>
        ) : (
          <DataTable
            rows={projectsQuery.data}
            columns={COLUMNS}
            getRowId={(row) => row.id}
            onActivate={handleActivate}
            emptyMessage={
              statusFilter === 'all'
                ? 'No projects yet — create one to start tracking a build.'
                : 'No projects with this status.'
            }
            aria-label="Projects"
          />
        )}
      </div>

      {creating ? (
        <CreateProjectDialog onClose={() => setCreating(false)} onCreated={handleCreated} />
      ) : null}
    </section>
  );
}

interface CreateProjectDialogProps {
  onClose: () => void;
  onCreated: (id: ProjectId) => void;
}

/** A compact create form — no Radix `Dialog` here (unlike most of this
 * feature's other dialogs) because it's the very first screen a brand-new
 * user lands on with zero projects; an inline panel that pushes the empty
 * table down reads as "fill this in to get started" rather than a modal
 * interrupting a list that has nothing to interrupt yet. Every later dialog
 * in this feature (add/edit BOM line, duplicate) *does* use Radix `Dialog`,
 * matching the rest of the app's row-action pattern (`RowActions.tsx`),
 * since those always open over an already-populated screen. */
function CreateProjectDialog({ onClose, onCreated }: CreateProjectDialogProps) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [buildQuantity, setBuildQuantity] = useState<number | ''>(1);
  const [repoLink, setRepoLink] = useState('');
  const [notes, setNotes] = useState('');
  const [submitError, setSubmitError] = useState<CommandError | null>(null);
  const { toast } = useToast();

  const createProject = useCreateProjectFull({
    onDone: (error, data) => {
      if (error) {
        setSubmitError(error);
        toast({
          title: 'Could not create project',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      if (data) {
        toast({ title: `Created ${data.name}`, kind: 'success' });
        onCreated(data.id);
      }
    },
  });

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName || buildQuantity === '' || buildQuantity < 1) return;
    setSubmitError(null);
    createProject.mutate({
      name: trimmedName,
      description: description.trim(),
      build_quantity: Math.round(buildQuantity),
      repo_link: repoLink.trim() === '' ? null : repoLink.trim(),
      notes: notes.trim(),
    });
  }

  const pending = createProject.isPending;
  const submitDisabled = pending || !name.trim() || buildQuantity === '' || buildQuantity < 1;

  return (
    <div className="projects-list-create" role="region" aria-label="New project">
      <form className="projects-list-create-form" onSubmit={handleSubmit}>
        <TextField
          label="Name"
          value={name}
          onChange={setName}
          autoFocus
          required
          disabled={pending}
        />
        <TextField
          label="Description"
          value={description}
          onChange={setDescription}
          placeholder="Optional"
          disabled={pending}
        />
        <NumberField
          label="Build quantity"
          value={buildQuantity}
          onChange={setBuildQuantity}
          min={1}
          step={1}
          required
          disabled={pending}
        />
        <TextField
          label="Repo / doc link"
          value={repoLink}
          onChange={setRepoLink}
          placeholder="Optional"
          disabled={pending}
        />
        <TextField
          label="Notes"
          value={notes}
          onChange={setNotes}
          placeholder="Optional"
          disabled={pending}
        />
        {submitError ? (
          <p className="projects-list-create-error">
            {errorHint(submitError.code) ?? submitError.message}
          </p>
        ) : null}
        <div className="projects-list-create-buttons">
          <button
            type="button"
            className="projects-list-create-cancel"
            onClick={onClose}
            disabled={pending}
          >
            Cancel
          </button>
          <button type="submit" className="projects-list-create-submit" disabled={submitDisabled}>
            {pending ? 'Creating…' : 'Create project'}
          </button>
        </div>
      </form>
    </div>
  );
}
