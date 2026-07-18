/**
 * The project detail screen (Phase 4 Task 6, spec §"Projects and BOMs"):
 * header (name, status control, build quantity, repo link, notes, created/
 * completed dates), actions (Edit fields, Duplicate, Archive), and the BOM
 * section (`BomTable`). `/projects/$projectId`'s routed entry point is
 * `ProjectDetailPage`, which reads the route param and renders this with a
 * plain `projectId` prop — the same split `PartDetail`/`PartDetailPage` use,
 * kept here purely for testability (no router param context needed to
 * render this component directly), even though — unlike `PartDetail`,
 * which is also the part-inspector drawer's body — there's no second
 * caller today.
 *
 * Build quantity is inline-editable in the header (commits on blur via
 * `useUpdateProject`, not per keystroke — see `Field.tsx`'s `NumberField.
 * onBlur`): the BOM's `total_required`/`missing` columns recompute
 * server-side from the new `build_quantity` the moment `keys.bom` is
 * invalidated, so nothing here needs to redo that math client-side. Name/
 * description/repo link/notes are edited as a batch via the "Edit fields"
 * dialog instead of each being independently inline-editable — those rarely
 * change together with build quantity's "adjust the batch size as you go"
 * usage pattern.
 */

import { Link, useNavigate } from '@tanstack/react-router';
import { useEffect, useState, type FormEvent } from 'react';

import type { ProjectId, ProjectRecord, ProjectStatus } from '../../bindings.gen';
import { NumberField, SelectField, TextField } from '../../components/Field';
import { useToast } from '../../components/Toast';
import {
  useArchiveProject,
  useDuplicateProject,
  useProject,
  useSetProjectStatus,
  useUpdateProject,
} from '../../hooks/projects';
import { errorHint, errorMessage, formatTimestamp, type CommandError } from '../../lib/format';
import { BomTable } from './BomTable';
import { StatusChip } from './StatusChip';
import './ProjectDetail.css';

const STATUS_OPTIONS: { value: ProjectStatus; label: string }[] = [
  { value: 'planned', label: 'Planned' },
  { value: 'active', label: 'Active' },
  { value: 'completed', label: 'Completed' },
  { value: 'archived', label: 'Archived' },
];

export interface ProjectDetailProps {
  projectId: ProjectId;
}

export function ProjectDetail({ projectId }: ProjectDetailProps) {
  const projectQuery = useProject(projectId);
  const navigate = useNavigate();
  const { toast } = useToast();
  const [editing, setEditing] = useState(false);
  const [duplicating, setDuplicating] = useState(false);
  const [buildQuantityDraft, setBuildQuantityDraft] = useState<number | ''>('');

  const project = projectQuery.data ?? null;

  // Keep the draft in sync with the server value whenever it changes from
  // outside this field (a fresh load, or this same mutation's own
  // invalidation landing) — the same "resync on prop change" rule
  // `InventoryPage`'s search box follows for its route-driven `q`.
  useEffect(() => {
    if (project) setBuildQuantityDraft(project.build_quantity);
  }, [project?.build_quantity]);

  const setStatus = useSetProjectStatus({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not change status',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Status updated', kind: 'success' });
    },
  });

  const updateProject = useUpdateProject({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not update project',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Build quantity updated', kind: 'success' });
    },
  });

  const archiveProject = useArchiveProject({
    onDone: (error) => {
      if (error) {
        toast({
          title: 'Could not archive project',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Project archived', kind: 'success' });
    },
  });

  if (projectQuery.isPending) {
    return <p className="project-detail-status">Loading project…</p>;
  }
  if (projectQuery.isError) {
    return (
      <p className="project-detail-status project-detail-status-error">
        Could not load this project: {errorMessage(projectQuery.error)}
      </p>
    );
  }
  if (project === null) {
    return (
      <div className="project-detail project-detail-not-found">
        <h2 className="project-detail-not-found-title">Project not found</h2>
        <p className="project-detail-not-found-description">
          This project no longer exists — it may have been deleted, or the link is out of date.
        </p>
        <Link to="/projects" className="project-detail-action-secondary">
          Back to Projects
        </Link>
      </div>
    );
  }

  // TS doesn't carry the `project === null` narrowing above into these
  // nested function declarations, even though `project` is `const` — a
  // fresh, explicitly non-null binding sidesteps that rather than asserting
  // `project!` at every use site below.
  const currentProject: ProjectRecord = project;

  function handleStatusChange(next: string) {
    setStatus.mutate({ id: projectId, status: next as ProjectStatus });
  }

  function commitBuildQuantity() {
    if (buildQuantityDraft === '' || buildQuantityDraft < 1) {
      setBuildQuantityDraft(currentProject.build_quantity);
      return;
    }
    const rounded = Math.round(buildQuantityDraft);
    if (rounded === currentProject.build_quantity) return;
    updateProject.mutate({ ...currentProject, build_quantity: rounded });
  }

  function handleDuplicated(newProject: ProjectRecord) {
    setDuplicating(false);
    void navigate({ to: '/projects/$projectId', params: { projectId: newProject.id } });
  }

  return (
    <div className="project-detail">
      <header className="project-detail-header">
        <div className="project-detail-header-top">
          <div>
            <p className="project-detail-eyebrow">Projects</p>
            <h1 className="project-detail-title">{project.name}</h1>
            {project.description ? (
              <p className="project-detail-description">{project.description}</p>
            ) : null}
          </div>
          <StatusChip status={project.status} />
        </div>

        <div className="project-detail-fields">
          <SelectField
            label="Status"
            value={project.status}
            onChange={handleStatusChange}
            options={STATUS_OPTIONS}
            disabled={setStatus.isPending}
          />
          <NumberField
            label="Build quantity"
            value={buildQuantityDraft}
            onChange={setBuildQuantityDraft}
            onBlur={commitBuildQuantity}
            min={1}
            step={1}
            disabled={updateProject.isPending}
          />
        </div>

        <dl className="project-detail-meta">
          <div>
            <dt>Repo / doc link</dt>
            <dd>
              {project.repo_link ? (
                <a
                  href={project.repo_link}
                  target="_blank"
                  rel="noreferrer"
                  className="project-detail-repo-link"
                >
                  {project.repo_link}
                </a>
              ) : (
                'No link set'
              )}
            </dd>
          </div>
          <div>
            <dt>Created</dt>
            <dd>{formatTimestamp(project.created_at)}</dd>
          </div>
          <div>
            <dt>Completed</dt>
            <dd>{project.completed_at ? formatTimestamp(project.completed_at) : '—'}</dd>
          </div>
        </dl>

        {project.notes ? <p className="project-detail-notes">{project.notes}</p> : null}

        <div className="project-detail-actions">
          <button
            type="button"
            className="project-detail-action-secondary"
            onClick={() => setEditing(true)}
          >
            Edit fields
          </button>
          <button
            type="button"
            className="project-detail-action-secondary"
            onClick={() => setDuplicating(true)}
          >
            Duplicate
          </button>
          <button
            type="button"
            className="project-detail-action-secondary"
            onClick={() => archiveProject.mutate(projectId)}
            disabled={archiveProject.isPending || project.status === 'archived'}
          >
            {archiveProject.isPending ? 'Archiving…' : 'Archive'}
          </button>
        </div>
      </header>

      <section className="project-detail-bom">
        <BomTable projectId={projectId} />
      </section>

      {editing ? <EditProjectDialog project={project} onClose={() => setEditing(false)} /> : null}
      {duplicating ? (
        <DuplicateProjectDialog
          project={project}
          onClose={() => setDuplicating(false)}
          onDuplicated={handleDuplicated}
        />
      ) : null}
    </div>
  );
}

interface EditProjectDialogProps {
  project: ProjectRecord;
  onClose: () => void;
}

/** Name/description/repo link/notes as one batch save — deliberately not a
 * Radix `Dialog` (unlike this feature's BOM-line dialogs): it's a plain
 * inline panel that replaces the header's static text with the same fields
 * in edit form, which reads more like "editing this section" than a modal
 * interruption, and keeps the BOM table below it visible and scrollable
 * while editing. */
function EditProjectDialog({ project, onClose }: EditProjectDialogProps) {
  const [name, setName] = useState(project.name);
  const [description, setDescription] = useState(project.description);
  const [repoLink, setRepoLink] = useState(project.repo_link ?? '');
  const [notes, setNotes] = useState(project.notes);
  const [submitError, setSubmitError] = useState<CommandError | null>(null);
  const { toast } = useToast();

  const updateProject = useUpdateProject({
    onDone: (error) => {
      if (error) {
        setSubmitError(error);
        toast({
          title: 'Could not update project',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      toast({ title: 'Project updated', kind: 'success' });
      onClose();
    },
  });

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName) return;
    setSubmitError(null);
    updateProject.mutate({
      ...project,
      name: trimmedName,
      description: description.trim(),
      repo_link: repoLink.trim() === '' ? null : repoLink.trim(),
      notes: notes.trim(),
    });
  }

  const pending = updateProject.isPending;

  return (
    <div className="project-detail-edit" role="region" aria-label="Edit project">
      <form className="project-detail-edit-form" onSubmit={handleSubmit}>
        <TextField label="Name" value={name} onChange={setName} required disabled={pending} />
        <TextField
          label="Description"
          value={description}
          onChange={setDescription}
          disabled={pending}
        />
        <TextField
          label="Repo / doc link"
          value={repoLink}
          onChange={setRepoLink}
          disabled={pending}
        />
        <TextField label="Notes" value={notes} onChange={setNotes} disabled={pending} />
        {submitError ? (
          <p className="project-detail-edit-error">
            {errorHint(submitError.code) ?? submitError.message}
          </p>
        ) : null}
        <div className="project-detail-edit-buttons">
          <button
            type="button"
            className="project-detail-action-secondary"
            onClick={onClose}
            disabled={pending}
          >
            Cancel
          </button>
          <button type="submit" className="project-detail-action-primary" disabled={pending}>
            {pending ? 'Saving…' : 'Save'}
          </button>
        </div>
      </form>
    </div>
  );
}

interface DuplicateProjectDialogProps {
  project: ProjectRecord;
  onClose: () => void;
  onDuplicated: (project: ProjectRecord) => void;
}

function DuplicateProjectDialog({ project, onClose, onDuplicated }: DuplicateProjectDialogProps) {
  const [newName, setNewName] = useState(`${project.name} copy`);
  const [submitError, setSubmitError] = useState<CommandError | null>(null);
  const { toast } = useToast();

  const duplicateProject = useDuplicateProject({
    onDone: (error, data) => {
      if (error) {
        setSubmitError(error);
        toast({
          title: 'Could not duplicate project',
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      if (data) {
        toast({ title: `Duplicated as ${data.name}`, kind: 'success' });
        onDuplicated(data);
      }
    },
  });

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = newName.trim();
    if (!trimmed) return;
    setSubmitError(null);
    duplicateProject.mutate({ id: project.id, newName: trimmed });
  }

  const pending = duplicateProject.isPending;

  return (
    <div className="project-detail-edit" role="region" aria-label="Duplicate project">
      <form className="project-detail-edit-form" onSubmit={handleSubmit}>
        <TextField
          label="New project name"
          value={newName}
          onChange={setNewName}
          required
          autoFocus
          disabled={pending}
        />
        <p className="project-detail-edit-hint">
          Copies the BOM structure and fields onto a new planned project — no reservations or
          transactions carry over.
        </p>
        {submitError ? (
          <p className="project-detail-edit-error">
            {errorHint(submitError.code) ?? submitError.message}
          </p>
        ) : null}
        <div className="project-detail-edit-buttons">
          <button
            type="button"
            className="project-detail-action-secondary"
            onClick={onClose}
            disabled={pending}
          >
            Cancel
          </button>
          <button
            type="submit"
            className="project-detail-action-primary"
            disabled={pending || !newName.trim()}
          >
            {pending ? 'Duplicating…' : 'Duplicate'}
          </button>
        </div>
      </form>
    </div>
  );
}
